use std::any::{Any, type_name};
use std::collections::HashMap;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use axum::extract::FromRef;
use axum::extract::ws::Utf8Bytes;
use axum::http::HeaderMap;
use axum::http::header::COOKIE;
use cookie::Cookie;
use leptos::prelude::*;
use leptos_use::utils::header;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::{debug, error, instrument};
use uuid::Uuid;

use crate::{ChannelMsg, SocketMsg};

/// Messages a mailbox buffers for a subscriber that is slower than the sender.
const MAILBOX_CAPACITY: usize = 16;
/// Messages a client's channel buffers for a connection that is slower than the sender.
const CLIENT_CHANNEL_CAPACITY: usize = 16;

/// Settings for the WebSocket connections. Pass them to [`ServerSocket::with_options`].
///
/// ```
/// # use std::time::Duration;
/// # use leptos_axum_socket::SocketOptions;
/// let options = SocketOptions::default()
///     .allowed_origins(["https://example.com"])
///     .ping_interval(Duration::from_secs(15));
/// ```
#[derive(Clone, Debug)]
pub struct SocketOptions {
    pub(crate) max_message_size: usize,
    pub(crate) max_key_size: usize,
    pub(crate) max_subscriptions: usize,
    pub(crate) allowed_origins: Option<Vec<String>>,
    pub(crate) ping_interval: Option<Duration>,
    pub(crate) read_timeout: Option<Duration>,
    pub(crate) write_timeout: Option<Duration>,
    pub(crate) secure_cookie: bool,
}

impl Default for SocketOptions {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024,
            max_key_size: 4 * 1024,
            max_subscriptions: 256,
            allowed_origins: None,
            ping_interval: Some(Duration::from_secs(30)),
            read_timeout: Some(Duration::from_secs(90)),
            write_timeout: Some(Duration::from_secs(30)),
            secure_cookie: true,
        }
    }
}

impl SocketOptions {
    /// Largest message accepted from a client, in bytes. A client that sends a bigger one is
    /// disconnected. Defaults to 1 MiB.
    pub fn max_message_size(self, bytes: usize) -> Self {
        Self {
            max_message_size: bytes,
            ..self
        }
    }

    /// Largest `Subscribe` message accepted, in bytes, which bounds the size of a key. A key is
    /// kept in memory for as long as a client is subscribed to it, so this stays small.
    /// Defaults to 4 KiB.
    pub fn max_key_size(self, bytes: usize) -> Self {
        Self {
            max_key_size: bytes,
            ..self
        }
    }

    /// Keys one connection can be subscribed to at a time. Each one costs memory for as long as
    /// the connection lasts, so the default is a low 256.
    pub fn max_subscriptions(self, count: usize) -> Self {
        Self {
            max_subscriptions: count,
            ..self
        }
    }

    /// Origins that may open a socket. Compared with the `Origin` header as the browser sends
    /// it: scheme and host without a trailing slash, like `"https://example.com"`. A request
    /// without an `Origin` header is accepted; browsers always send one. No check by default.
    pub fn allowed_origins<I, S>(self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed_origins: Some(origins.into_iter().map(Into::into).collect()),
            ..self
        }
    }

    /// How often the server pings each client. `None` or zero turns pings off.
    /// Defaults to 30 seconds.
    pub fn ping_interval(self, interval: impl Into<Option<Duration>>) -> Self {
        Self {
            ping_interval: non_zero(interval),
            ..self
        }
    }

    /// A connection on which nothing arrives for this long, not even a pong, is closed.
    /// `None` or zero turns the timeout off. Defaults to 90 seconds.
    ///
    /// Browsers send nothing on their own, so this only applies while pings are on, and it has
    /// to be longer than the ping interval. [`ServerSocket::with_options`] panics otherwise.
    pub fn read_timeout(self, timeout: impl Into<Option<Duration>>) -> Self {
        Self {
            read_timeout: non_zero(timeout),
            ..self
        }
    }

    /// A write to a client that doesn't finish within this time closes the connection.
    /// `None` or zero turns the timeout off. Defaults to 30 seconds.
    pub fn write_timeout(self, timeout: impl Into<Option<Duration>>) -> Self {
        Self {
            write_timeout: non_zero(timeout),
            ..self
        }
    }

    /// Set the client id cookie as `__Host-socket_client_id` with the `Secure` attribute, so
    /// that no other host or subdomain can set it and it's never sent over plain HTTP.
    /// On by default.
    ///
    /// Most browsers drop such a cookie on plain HTTP, even on `http://localhost`, and
    /// `send_to_self` then reaches no one. Turn it off when serving without HTTPS.
    pub fn secure_cookie(self, secure: bool) -> Self {
        Self {
            secure_cookie: secure,
            ..self
        }
    }
}

fn non_zero(duration: impl Into<Option<Duration>>) -> Option<Duration> {
    duration.into().filter(|duration| !duration.is_zero())
}

/// This has to be added to the axum state and is used to send and subscribe to channels.
#[derive(Clone, Debug, Default)]
pub struct ServerSocket(Arc<Mutex<ServerSocketInner>>);

impl ServerSocket {
    pub fn new() -> Self {
        Self::default()
    }

    /// # Panics
    ///
    /// When the read timeout isn't longer than the ping interval: every idle client would be
    /// disconnected and reconnect in a loop.
    pub fn with_options(options: SocketOptions) -> Self {
        if let (Some(interval), Some(timeout)) = (options.ping_interval, options.read_timeout) {
            assert!(
                timeout > interval,
                "SocketOptions: read_timeout ({timeout:?}) has to be longer than ping_interval ({interval:?})"
            );
        }

        Self(Arc::new(Mutex::new(ServerSocketInner {
            options,
            ..Default::default()
        })))
    }

    /// Locks the server socket for exclusive access. With this you can then send messages to the socket.
    ///
    /// Don't hold the guard across an `.await`: every socket operation in the process waits for it.
    ///
    /// See [`ServerSocketInner::send`].
    #[inline]
    pub fn lock(&self) -> MutexGuard<'_, ServerSocketInner> {
        // The maps stay consistent across a panic in a caller, so poisoning carries no information.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs the subscribe filters without holding the lock: a filter may take a while, or lock
    /// the socket itself.
    pub(crate) async fn can_subscribe<C>(&self, key: &Value, ctx: &C) -> bool
    where
        C: Send + Sync + 'static,
    {
        let filters = self.lock().subscribe_filters.clone();

        for filter in &filters {
            if !filter(key, ctx).await {
                return false;
            }
        }

        true
    }

    /// Runs the send mappers without holding the lock: a mapper may lock the socket itself.
    /// `None` when no mapper accepts the message, which drops it.
    pub(crate) fn map_msg<C>(&self, key: &Value, msg: &Value, ctx: &C) -> Option<Value>
    where
        C: 'static,
    {
        let mappers = self.lock().send_mappers.clone();

        mappers
            .iter()
            .find_map(|mapper| mapper(key, msg, ctx))
            .flatten()
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type SubscribeFilterFn = Arc<dyn Fn(&Value, &dyn Any) -> BoxFuture<bool> + Send + Sync>;
/// `None`: the mapper is for another message type. `Some(None)`: the mapper denied the message.
type SendMapFn = Arc<dyn Fn(&Value, &Value, &dyn Any) -> Option<Option<Value>> + Send + Sync>;

/// This is used on the server to manage socket connections.
#[derive(Default)]
pub struct ServerSocketInner {
    options: SocketOptions,
    sender_map: HashMap<Arc<Value>, broadcast::Sender<Utf8Bytes>>,
    /// One channel per client (browser) that each of its connections (tabs) receives from.
    client_to_sender: HashMap<Uuid, broadcast::Sender<Utf8Bytes>>,
    subscribe_filters: Vec<SubscribeFilterFn>,
    send_mappers: Vec<SendMapFn>,
}

impl std::fmt::Debug for ServerSocketInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSocketInner")
            .field("options", &self.options)
            .field("mailboxes", &self.sender_map.len())
            .field("clients", &self.client_to_sender.len())
            .field("subscribe_filters", &self.subscribe_filters.len())
            .field("send_mappers", &self.send_mappers.len())
            .finish()
    }
}

impl ServerSocketInner {
    pub(crate) fn options(&self) -> &SocketOptions {
        &self.options
    }

    /// Broadcast a message from the server to the subscribers of the given key.
    ///
    /// This is used to send messages from an axum handler.
    /// If you want to send from a server function, use the module level [`send`] function.
    ///
    /// ## Example
    ///
    /// ```
    /// # use leptos_axum_socket::{ServerSocket, SocketMsg};
    /// # use serde::{Serialize, Deserialize};
    /// # use axum::extract::{State, FromRef};
    /// #
    /// # #[derive(FromRef, Clone)]
    /// # pub struct AppState {
    /// #     pub socket: ServerSocket,
    /// # }
    /// #
    /// # #[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
    /// # struct TheKey;
    /// #
    /// # #[derive(Clone, Serialize, Deserialize, Debug)]
    /// # struct TheMessage;
    /// #
    /// # impl SocketMsg for TheMessage {
    /// #     type Key = TheKey;
    /// #     #[cfg(feature = "ssr")]
    /// #     type AppState = AppState;
    /// # }
    ///
    /// async fn axum_handler(State(socket): State<ServerSocket>) {
    ///     socket.lock().send(&TheKey, &TheMessage);
    /// }
    /// ```
    #[instrument(level = "debug", skip_all)]
    pub fn send<Msg>(&self, key: &Msg::Key, msg: &Msg)
    where
        Msg: SocketMsg + Serialize + Clone + Send + Sync + 'static,
        for<'de> Msg: Deserialize<'de>,
        Msg::Key: Hash + Eq + Serialize + Clone + Send + Sync + 'static,
        for<'de> Msg::Key: Deserialize<'de>,
    {
        let key = serde_json::to_value(key).unwrap();
        let msg = serde_json::to_value(msg).unwrap();

        self.send_serialized(key, msg);
    }

    /// Send a message from the server to every tab of the browser whose client id cookie is in
    /// `headers`, and to no one else.
    ///
    /// This is used to send messages from an axum handler.
    /// If you want to send from a server function, use the module level [`send_to_self`] function.
    ///
    /// A tab that falls behind loses its oldest messages.
    ///
    /// ## Example
    ///
    /// ```
    /// # use leptos_axum_socket::{ServerSocket, SocketMsg};
    /// # use serde::{Serialize, Deserialize};
    /// # use axum::{extract::{State, FromRef}, http::header::HeaderMap};
    /// #
    /// # #[derive(FromRef, Clone)]
    /// # pub struct AppState {
    /// #     pub socket: ServerSocket,
    /// # }
    /// #
    /// # #[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
    /// # struct TheKey;
    /// #
    /// # #[derive(Clone, Serialize, Deserialize, Debug)]
    /// # struct TheMessage;
    /// #
    /// # impl SocketMsg for TheMessage {
    /// #     type Key = TheKey;
    /// #     #[cfg(feature = "ssr")]
    /// #     type AppState = AppState;
    /// # }
    ///
    /// async fn axum_handler(State(socket): State<ServerSocket>, headers: HeaderMap) {
    ///     socket.lock().send_to_self(&TheKey, &TheMessage, &headers);
    /// }
    /// ```
    #[instrument(level = "debug", skip_all)]
    pub fn send_to_self<Msg>(&self, key: &Msg::Key, msg: &Msg, headers: &HeaderMap)
    where
        Msg: SocketMsg + Serialize + Clone + Send + Sync + 'static,
        for<'de> Msg: Deserialize<'de>,
        Msg::Key: Hash + Eq + Serialize + Clone + Send + Sync + 'static,
        for<'de> Msg::Key: Deserialize<'de>,
    {
        match client_id_from_headers(headers, self.options.secure_cookie) {
            Ok(client_id) => {
                let key = serde_json::to_value(key).unwrap();
                let msg = serde_json::to_value(msg).unwrap();

                self.send_serialized_to_self(client_id, key, msg);
            }
            Err(err) => error!("Can't send to self: {err}"),
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub(crate) fn send_serialized(&self, key: Value, msg: Value) {
        let Some(sender) = self.sender_map.get(&key) else {
            debug!("No subscribers for this key, dropping message");
            return;
        };

        if sender.send(encode(&ChannelMsg::Msg { key, msg })).is_err() {
            debug!("No receivers left for this key, dropping message");
        }
    }

    #[instrument(level = "debug", skip_all)]
    pub(crate) fn send_serialized_to_self(&self, client_id: Uuid, key: Value, msg: Value) {
        let Some(sender) = self.client_to_sender.get(&client_id) else {
            error!("WebSocket transmitter for this client not found");
            return;
        };

        if sender.send(encode(&ChannelMsg::Msg { key, msg })).is_err() {
            debug!("Client disconnected, dropping message");
        }
    }

    pub(crate) fn subscribe_client(&mut self, client_id: Uuid) -> broadcast::Receiver<Utf8Bytes> {
        self.client_to_sender
            .entry(client_id)
            .or_insert_with(|| broadcast::Sender::new(CLIENT_CHANNEL_CAPACITY))
            .subscribe()
    }

    /// Call after a connection dropped its receiver.
    pub(crate) fn remove_client_if_unused(&mut self, client_id: Uuid) {
        if self
            .client_to_sender
            .get(&client_id)
            .is_some_and(|sender| sender.receiver_count() == 0)
        {
            self.client_to_sender.remove(&client_id);
        }
    }

    pub(crate) fn subscribe(&mut self, key: Arc<Value>) -> broadcast::Receiver<Utf8Bytes> {
        self.sender_map
            .entry(key)
            .or_insert_with(|| {
                debug!("Creating new mailbox");

                broadcast::Sender::new(MAILBOX_CAPACITY)
            })
            .subscribe()
    }

    /// Call after a subscriber dropped its receiver.
    pub(crate) fn remove_mailbox_if_unused(&mut self, key: &Value) {
        if self
            .sender_map
            .get(key)
            .is_some_and(|sender| sender.receiver_count() == 0)
        {
            debug!("Removing unused mailbox");

            self.sender_map.remove(key);
        }
    }

    /// Add a subscribe filter to the server. Whenever someone wants to subscribe,
    /// the filter will be called with the key and context.
    /// It can then return `true` to allow the subscription or `false` to deny it.
    /// If multiple filters are found for a given key,
    /// the subscription will only be allowed if all filters return `true`.
    ///
    /// A filter only applies to keys that deserialize as its key type. A key that no filter
    /// applies to can be subscribed by anyone, so don't broadcast anything secret on such a key.
    ///
    /// The filter runs without the socket lock, so it may take its time and may lock the socket
    /// itself. `C` has to be the context type given to
    /// [`upgrade_websocket`](crate::handlers::upgrade_websocket); a filter with another context
    /// type logs an error and denies every subscription it applies to.
    pub fn add_subscribe_filter<K, C, F, Fut>(&mut self, filter: F)
    where
        K: Send + Sync + 'static,
        for<'de> K: Deserialize<'de>,
        F: Fn(K, C) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send,
        C: Clone + Send + Sync + 'static,
    {
        self.subscribe_filters.push(Arc::new(
            move |key: &Value, ctx: &dyn Any| -> BoxFuture<bool> {
                let Ok(key) = K::deserialize(key) else {
                    // This filter doesn't apply to the key
                    return Box::pin(async { true });
                };

                let Some(ctx) = ctx.downcast_ref::<C>() else {
                    error!(
                        "Subscribe filter expects context type `{}` but the socket was upgraded with another type. Denying subscription.",
                        type_name::<C>()
                    );
                    return Box::pin(async { false });
                };

                let ctx = ctx.clone();
                let filter = filter.clone();

                Box::pin(async move { filter(key, ctx).await })
            },
        ));
    }

    /// Add a send mapper to the server. Whenever a client sends a message,
    /// the mapper will be called with the key, message, and context.
    /// It can then return `Some(message)` to allow the message to be sent or `None` to deny it.
    /// It can also modify the message before sending it.
    ///
    /// A message from a client is only broadcast if a mapper accepts it. Without a mapper for a
    /// message type, clients can't send that type at all; only the server can.
    ///
    /// Make sure you only add one mapper per message type (the message type also specifies the key type).
    /// If you add multiple mappers for the same message type,
    /// the first one added will be used and all subsequent ones will be ignored.
    ///
    /// Mappers are matched by whether key and message deserialize as their types, and mailboxes
    /// are keyed by the key's JSON value alone. Key types with the same JSON shape (every unit
    /// struct is `null`, for example) share a mailbox, and a mapper for one of them admits
    /// messages into it that also parse as its message type.
    ///
    /// The mapper runs without the socket lock, so it may lock the socket itself, for example to
    /// send a message of its own. `C` has to be the context type given to
    /// [`upgrade_websocket`](crate::handlers::upgrade_websocket); a mapper with another context
    /// type logs an error and denies every message it applies to.
    pub fn add_send_mapper<M, C, F>(&mut self, mapper: F)
    where
        M: SocketMsg + Serialize,
        for<'de> M: Deserialize<'de>,
        for<'de> M::Key: Deserialize<'de>,
        F: Fn(M::Key, M, &C) -> Option<M> + Send + Sync + 'static,
        C: 'static,
    {
        self.send_mappers.push(Arc::new(
            move |key: &Value, msg: &Value, ctx: &dyn Any| -> Option<Option<Value>> {
                let key = <M::Key as Deserialize>::deserialize(key).ok()?;
                let msg = M::deserialize(msg).ok()?;

                let Some(ctx) = ctx.downcast_ref::<C>() else {
                    error!(
                        "Send mapper expects context type `{}` but the socket was upgraded with another type. Denying message.",
                        type_name::<C>()
                    );
                    return Some(None);
                };

                let mapped = mapper(key, msg, ctx).and_then(|msg| match serde_json::to_value(msg) {
                    Ok(msg) => Some(msg),
                    Err(err) => {
                        error!("Failed to serialize mapped message: {err}");
                        None
                    }
                });

                Some(mapped)
            },
        ));
    }
}

fn encode(msg: &ChannelMsg) -> Utf8Bytes {
    serde_json::to_string(msg)
        .expect("ChannelMsg holds only JSON values")
        .into()
}

/// Broadcast a message from a server function to the subscribers of the given key.
///
/// You can call this function only from a server function.
/// If you want to call this from an axum handler, use `ServerSocketInner::send` instead.
#[instrument(level = "debug", skip_all)]
pub async fn send<Msg>(key: &Msg::Key, msg: &Msg)
where
    Msg: SocketMsg + Serialize + Clone + Send + Sync + 'static,
    for<'de> Msg: Deserialize<'de>,
    Msg::Key: Hash + Eq + Serialize + Clone + Send + Sync + 'static,
    for<'de> Msg::Key: Deserialize<'de>,
    Msg::AppState: Clone,
    ServerSocket: FromRef<Msg::AppState>,
{
    let state: Msg::AppState = match use_context() {
        Some(state) => state,
        None => {
            error!(
                "Failed to get the app state context. You can call this function only from a server function. If you want to call this from an axum handler, use `ServerSocketInner::send` instead."
            );
            return;
        }
    };

    ServerSocket::from_ref(&state).lock().send(key, msg);
}

/// Send a message from a server function to every tab of the calling browser, and to no one else.
///
/// You can call this function only from a server function.
/// If you want to call this from an axum handler use [`ServerSocketInner::send_to_self`] instead.
///
/// A tab that falls behind loses its oldest messages.
#[instrument(level = "debug", skip_all)]
pub async fn send_to_self<Msg>(key: &Msg::Key, msg: &Msg)
where
    Msg: SocketMsg + Serialize + Clone + Send + Sync + 'static,
    for<'de> Msg: Deserialize<'de>,
    Msg::Key: Hash + Eq + Serialize + Clone + Send + Sync + 'static,
    for<'de> Msg::Key: Deserialize<'de>,
    Msg::AppState: Clone,
    ServerSocket: FromRef<Msg::AppState>,
{
    let state: Msg::AppState = match use_context() {
        Some(state) => state,
        None => {
            error!(
                "Failed to get the app state context. You can call this function only from a server function. If you want to call this from an axum handler use `ServerSocketInner::send_to_self` instead."
            );
            return;
        }
    };

    let socket = ServerSocket::from_ref(&state);
    let secure_cookie = socket.lock().options.secure_cookie;

    let client_id = match extract_client_id_server_fn(secure_cookie) {
        Ok(id) => id,
        Err(err) => {
            error!(
                "Failed to extract client ID: {err}. You can call this function only from a server function. If you want to call this from an axum handler use `ServerSocketInner::send_to_self` instead."
            );
            return;
        }
    };

    let key = serde_json::to_value(key).unwrap();
    let msg = serde_json::to_value(msg).unwrap();

    socket.lock().send_serialized_to_self(client_id, key, msg);
}

/// Name of the cookie that carries the client id. The `__Host-` prefix makes browsers reject the
/// cookie from any other host or subdomain.
pub(crate) fn cookie_name(secure: bool) -> &'static str {
    if secure {
        "__Host-socket_client_id"
    } else {
        "socket_client_id"
    }
}

fn extract_client_id_server_fn(secure: bool) -> Result<Uuid, String> {
    let cookie_header = header(COOKIE).ok_or("No cookie header found")?;

    read_client_id_from_cookie_header(&cookie_header, secure)
}

pub(crate) fn client_id_from_headers(headers: &HeaderMap, secure: bool) -> Result<Uuid, String> {
    let cookie_header = headers.get(COOKIE).ok_or("No cookie header found")?;
    let cookie_header = cookie_header
        .to_str()
        .map_err(|err| format!("Failed to parse cookie header: {err}"))?;

    read_client_id_from_cookie_header(cookie_header, secure)
}

fn read_client_id_from_cookie_header(cookie_header: &str, secure: bool) -> Result<Uuid, String> {
    let name = cookie_name(secure);

    let cookie = Cookie::split_parse(cookie_header)
        .filter_map(Result::ok)
        .find(|cookie| cookie.name() == name)
        .ok_or_else(|| format!("{name} cookie not found"))?;

    Uuid::parse_str(cookie.value()).map_err(|err| format!("Invalid UUID in {name} cookie: {err}"))
}
