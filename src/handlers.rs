use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    extract::{
        WebSocketUpgrade,
        ws::{Message, Utf8Bytes, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use cookie::{Cookie, SameSite};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde_json::Value;
use tokio::{
    sync::{Mutex, broadcast},
    task::JoinHandle,
    time::{error::Elapsed, timeout},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace, warn};
use uuid::Uuid;

use crate::{
    ChannelMsg, ServerSocket, SocketOptions,
    channel::{client_id_from_headers, cookie_name},
};

type WsSink = Arc<Mutex<SplitSink<WebSocket, Message>>>;

/// One of a client's connections. Dropping it stops every task spawned for it, which removes
/// its receivers from the socket, so no exit path, a panic included, can leak them.
struct Connection {
    socket: ServerSocket,
    connection_id: Uuid,
    ws_tx: WsSink,
    options: SocketOptions,
    cancel: CancellationToken,
    subscriptions: HashMap<Arc<Value>, JoinHandle<()>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Connection {
    async fn handle_text<C>(&mut self, text: &str, context: &C)
    where
        C: Send + Sync + 'static,
    {
        let msg: ChannelMsg = match serde_json::from_str(text) {
            Ok(msg) => msg,
            Err(err) => {
                debug!(connection_id = %self.connection_id, %err, "Ignoring malformed message");
                return;
            }
        };

        match msg {
            ChannelMsg::Subscribe { key } => {
                if self.subscriptions.contains_key(&key) {
                    return;
                }

                // The key stays in memory for as long as the subscription lasts.
                if text.len() > self.options.max_key_size {
                    debug!(connection_id = %self.connection_id, "Subscription key too large");
                    return;
                }

                if self.subscriptions.len() >= self.options.max_subscriptions {
                    debug!(connection_id = %self.connection_id, "Subscription limit reached");
                    return;
                }

                if !self.socket.can_subscribe(&key, context).await {
                    debug!(connection_id = %self.connection_id, "Subscription denied");
                    return;
                }

                // The connection, the mailbox and its receiver share one copy of the key.
                let key = Arc::new(key);
                let mailbox = ChannelReceiver::mailbox(&self.socket, Arc::clone(&key));

                let handle = tokio::spawn(forward_broadcast(
                    Arc::clone(&self.ws_tx),
                    mailbox,
                    self.options.write_timeout,
                    self.cancel.clone(),
                ));

                self.subscriptions.insert(key, handle);
            }
            ChannelMsg::Unsubscribe { key } => {
                if let Some(handle) = self.subscriptions.remove(&key) {
                    handle.abort();
                }
            }
            ChannelMsg::Msg { msg, key } => {
                if let Some(msg) = self.socket.map_msg(&key, &msg, context) {
                    self.socket.lock().send_serialized(key, msg);
                }
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        for handle in self.subscriptions.values().chain(&self.tasks) {
            handle.abort();
        }
    }
}

enum Channel {
    Mailbox(Arc<Value>),
    Client(Uuid),
}

/// A connection's end of a mailbox or of its client's channel. Dropping it, which is what
/// aborting the forwarding task does, removes the channel once no receiver is left.
struct ChannelReceiver {
    socket: ServerSocket,
    channel: Channel,
    receiver: Option<broadcast::Receiver<Utf8Bytes>>,
}

impl ChannelReceiver {
    fn mailbox(socket: &ServerSocket, key: Arc<Value>) -> Self {
        let receiver = socket.lock().subscribe(Arc::clone(&key));

        Self {
            socket: socket.clone(),
            channel: Channel::Mailbox(key),
            receiver: Some(receiver),
        }
    }

    fn client(socket: &ServerSocket, client_id: Uuid) -> Self {
        let receiver = socket.lock().subscribe_client(client_id);

        Self {
            socket: socket.clone(),
            channel: Channel::Client(client_id),
            receiver: Some(receiver),
        }
    }

    async fn recv(&mut self) -> Result<Utf8Bytes, broadcast::error::RecvError> {
        self.receiver
            .as_mut()
            .expect("receiver is only taken in drop")
            .recv()
            .await
    }
}

impl Drop for ChannelReceiver {
    fn drop(&mut self) {
        // The receiver count has to go down before the check.
        drop(self.receiver.take());

        let mut socket = self.socket.lock();

        match &self.channel {
            Channel::Mailbox(key) => socket.remove_mailbox_if_unused(key),
            Channel::Client(client_id) => socket.remove_client_if_unused(*client_id),
        }
    }
}

async fn handle_websocket_with_context<C>(
    ws: WebSocket,
    socket: ServerSocket,
    client_id: Uuid,
    context: C,
    options: SocketOptions,
) where
    C: Send + Sync + 'static,
{
    let (ws_tx, mut ws_rx) = ws.split();
    let ws_tx = Arc::new(Mutex::new(ws_tx));

    let cancel = CancellationToken::new();

    let mut conn = Connection {
        socket,
        connection_id: Uuid::new_v4(),
        ws_tx: Arc::clone(&ws_tx),
        cancel: cancel.clone(),
        subscriptions: HashMap::new(),
        tasks: Vec::with_capacity(2),
        options,
    };

    let connection_id = conn.connection_id;

    conn.tasks.push(tokio::spawn(forward_broadcast(
        Arc::clone(&ws_tx),
        ChannelReceiver::client(&conn.socket, client_id),
        conn.options.write_timeout,
        cancel.clone(),
    )));

    if let Some(interval) = conn.options.ping_interval {
        conn.tasks.push(tokio::spawn(ping(
            ws_tx,
            interval,
            conn.options.write_timeout,
            cancel.clone(),
        )));
    }

    // Without pings an idle client sends nothing, so the timeout would close it for no reason.
    let read_timeout = conn.options.ping_interval.and(conn.options.read_timeout);

    loop {
        let msg = tokio::select! {
            () = cancel.cancelled() => break,
            next = maybe_timeout(read_timeout, ws_rx.next()) => match next {
                Ok(Some(Ok(msg))) => msg,
                Ok(Some(Err(err))) => {
                    debug!(%connection_id, %err, "WebSocket read failed, closing");
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    debug!(%connection_id, "Nothing received within the read timeout, closing");
                    break;
                }
            },
        };

        match msg {
            Message::Close(_) => break,
            Message::Text(text) => {
                trace!("Received Text: {text}");

                conn.handle_text(text.as_str(), &context).await;
            }
            _ => (),
        }
    }
}

/// [`timeout`] that can be turned off.
async fn maybe_timeout<F>(duration: Option<Duration>, future: F) -> Result<F::Output, Elapsed>
where
    F: Future,
{
    match duration {
        Some(duration) => timeout(duration, future).await,
        None => Ok(future.await),
    }
}

/// `false` when the write failed or timed out; the connection is unusable then.
///
/// Only the write is timed, not the wait for the sink: a slow client that still reads can keep
/// the sink busy with other subscriptions for longer than one write is allowed to take.
async fn write_message(ws_tx: &WsSink, msg: Message, write_timeout: Option<Duration>) -> bool {
    let mut sink = ws_tx.lock().await;

    match maybe_timeout(write_timeout, sink.send(msg)).await {
        Ok(Ok(())) => true,
        Ok(Err(err)) => {
            debug!(%err, "WebSocket write failed");
            false
        }
        Err(_) => {
            warn!("WebSocket write timed out");
            false
        }
    }
}

async fn forward_broadcast(
    ws_tx: WsSink,
    mut receiver: ChannelReceiver,
    write_timeout: Option<Duration>,
    cancel: CancellationToken,
) {
    loop {
        match receiver.recv().await {
            Ok(frame) => {
                if !write_message(&ws_tx, Message::Text(frame), write_timeout).await {
                    cancel.cancel();
                    return;
                }
            }
            // The client missed messages but stays subscribed.
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                debug!(missed, "Subscriber lagged behind");
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn ping(
    ws_tx: WsSink,
    interval: Duration,
    write_timeout: Option<Duration>,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(interval);
    // The first tick completes immediately.
    interval.tick().await;

    loop {
        interval.tick().await;

        if !write_message(&ws_tx, Message::Ping(Bytes::new()), write_timeout).await {
            cancel.cancel();
            return;
        }
    }
}

fn origin_allowed(headers: &HeaderMap, options: &SocketOptions) -> bool {
    let Some(allowed_origins) = &options.allowed_origins else {
        return true;
    };

    // Browsers always send `Origin` on a WebSocket handshake. A request without one isn't a
    // browser acting on a user's behalf, which is what the check is for.
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };

    origin.to_str().is_ok_and(|origin| {
        allowed_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(origin))
    })
}

fn page_served_over_http(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_some_and(|origin| origin.starts_with("http://"))
}

/// This is used to handle the incoming WebSocket connection.
///
/// Applies the [`SocketOptions`](crate::SocketOptions) of the `socket`: the request is answered
/// with `403 Forbidden` when its `Origin` isn't allowed, and the connection is limited, pinged
/// and timed out as configured.
///
/// ```
/// # use axum::{extract::{State, WebSocketUpgrade}, http::HeaderMap, response::Response};
/// # use leptos_axum_socket::{ServerSocket, handlers::upgrade_websocket};
/// #
/// #[cfg(feature = "ssr")]
/// pub async fn connect_to_websocket(
///     ws: WebSocketUpgrade,
///     headers: HeaderMap,
///     State(socket): State<ServerSocket>,
/// ) -> Response {
///     // You could do authentication here
///
///     // Provide extra context like the user's ID for example that is passed to the permission filters
///     let ctx = ();
///
///     upgrade_websocket(ws, &headers, socket, ctx)
/// }
/// ```
pub fn upgrade_websocket<C>(
    ws: WebSocketUpgrade,
    headers: &HeaderMap,
    socket: ServerSocket,
    context: C,
) -> Response
where
    C: Send + Sync + 'static,
{
    let options = socket.lock().options().clone();

    if !origin_allowed(headers, &options) {
        debug!(
            origin = ?headers.get(header::ORIGIN),
            "Rejecting WebSocket upgrade from disallowed origin"
        );
        return StatusCode::FORBIDDEN.into_response();
    }

    // One id per browser, shared by all its tabs, so `send_to_self` reaches every one of them.
    let (client_id, new_cookie) = match client_id_from_headers(headers, options.secure_cookie) {
        Ok(client_id) => (client_id, None),
        Err(_) => {
            if options.secure_cookie && page_served_over_http(headers) {
                warn!(
                    "Most browsers drop the secure client id cookie on plain HTTP, so `send_to_self` won't reach this browser. Turn off `SocketOptions::secure_cookie` when serving without HTTPS."
                );
            }

            let client_id = Uuid::new_v4();

            let cookie = Cookie::build((cookie_name(options.secure_cookie), client_id.to_string()))
                .path("/")
                .http_only(true)
                .secure(options.secure_cookie)
                .same_site(SameSite::Strict)
                .build();

            (client_id, Some(cookie))
        }
    };

    let mut response = ws
        .max_message_size(options.max_message_size)
        .max_frame_size(options.max_message_size)
        .on_upgrade(move |websocket| {
            handle_websocket_with_context(websocket, socket, client_id, context, options)
        });

    if let Some(cookie) = new_cookie {
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie.to_string()).expect("cookie is ASCII"),
        );
    }

    response
}
