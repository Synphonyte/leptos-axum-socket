use axum::extract::FromRef;
use axum::http::header::COOKIE;
use leptos::prelude::*;
use leptos_use::utils::header;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::{collections::HashMap, sync::Arc};
use std::{fmt::Debug, hash::Hash};
use tokio::sync::broadcast::{self, Receiver};
use tokio::sync::mpsc;
use tokio::sync::{Mutex, MutexGuard};
use tokio::task::JoinHandle;
use tracing::{debug, error, instrument};
use uuid::Uuid;

use crate::{ChannelMsg, SocketMsg};

#[derive(Clone, Debug, Default)]
pub struct ServerSocket(Arc<Mutex<ServerSocketInner>>);

impl ServerSocket {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub async fn lock(&self) -> MutexGuard<'_, ServerSocketInner> {
        self.0.lock().await
    }
}

type SubscribeFilterFn = Arc<dyn Fn(Value, &dyn Any) -> bool + Send + Sync>;
type SendMapFn =
    Arc<dyn Fn(Value, Value, &dyn Any) -> serde_json::Result<Option<Value>> + Send + Sync>;

/// This is used on the server to manage socket connections.
#[derive(Default)]
pub struct ServerSocketInner {
    sender_map: HashMap<Value, broadcast::Sender<ChannelMsg>>,
    client_to_sender: HashMap<Uuid, mpsc::Sender<ChannelMsg>>,
    subscribe_filters: Vec<SubscribeFilterFn>,
    send_mappers: Vec<SendMapFn>,
    handles: HashMap<Value, JoinHandle<()>>,
}

impl std::fmt::Debug for ServerSocketInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSocketChannels")
            .field("sender_map", &self.sender_map)
            .field("subscribe_filters", &self.subscribe_filters.len())
            .field("send_mappers", &self.send_mappers.len())
            .finish()
    }
}

impl ServerSocketInner {
    #[instrument]
    fn sender(&mut self, key: Value) -> broadcast::Sender<ChannelMsg> {
        let sender = self.sender_map.entry(key).or_insert_with(|| {
            debug!("Creating new sender for key");

            broadcast::Sender::new(16)
        });
        sender.clone()
    }

    #[instrument]
    pub(crate) fn send(&mut self, key: Value, msg: Value) {
        if let Err(err) = self.sender(key.clone()).send(ChannelMsg::Msg { msg, key }) {
            debug!(
                "Failed to send message because there are no receivers: {:?}",
                err
            );
        }
    }

    #[instrument]
    pub(crate) async fn send_to_self(&self, client_id: Uuid, key: Value, msg: Value) {
        if let Some(sender) = self.client_to_sender.get(&client_id) {
            if let Err(err) = sender.send(ChannelMsg::Msg { key, msg }).await {
                debug!("Failed to send websocket message: {:?}", err);
            }
        } else {
            error!(
                "WebSocket transmitter for client ID {} not found",
                client_id
            );
        }
    }

    pub(crate) fn insert_client_sender(
        &mut self,
        client_id: Uuid,
        sender: mpsc::Sender<ChannelMsg>,
    ) {
        self.client_to_sender.insert(client_id, sender);
    }

    #[instrument]
    pub(crate) fn subscribe(&mut self, key: Value) -> Receiver<ChannelMsg> {
        self.sender(key).subscribe()
    }

    pub(crate) fn remember_handle(&mut self, key: Value, handle: JoinHandle<()>) {
        self.handles.insert(key, handle);
    }

    pub(crate) fn unsubscribe(&mut self, key: Value) {
        if let Some(handle) = self.handles.remove(&key) {
            handle.abort();
        }
    }

    /// Add a subscribe filter to the server. Whenever someone wants to subscribe ,
    /// the filter will be called with the key and context.
    /// It can then return `true` to allow the subscription or `false` to deny it.
    /// If multiple filters are found for a given key,
    /// the subscription will only be allowed if all filters return `true`.
    pub fn add_subscribe_filter<K, C, F>(&mut self, filter: F)
    where
        for<'de> K: Deserialize<'de>,
        F: Fn(K, &C) -> bool + Send + Sync + 'static,
        C: 'static,
    {
        self.subscribe_filters
            .push(Arc::new(move |key: Value, ctx: &dyn Any| {
                let ctx: &C = ctx.downcast_ref().expect("Invalid context type");

                match serde_json::from_value(key) {
                    Ok(key) => filter(key, ctx),
                    Err(_) => {
                        // This filter doesn't apply to the key
                        true
                    }
                }
            }));
    }

    /// Add a send mapper to the server. Whenever someone wants to send a message,
    /// the mapper will be called with the key, message, and context.
    /// It can then return `Some(message)` to allow the message to be sent or `None` to deny it.
    /// It can also modify the message before sending it.
    ///
    /// Make sure you only add one mapper per message type (the message type also specifies the key type).
    /// If you add multiple mappers for the same message type,
    /// the first one added will be used and all subsequent ones will be ignored.
    pub fn add_send_mapper<M, C, F>(&mut self, mapper: F)
    where
        M: SocketMsg + Serialize,
        for<'de> M: Deserialize<'de>,
        for<'de> M::Key: Deserialize<'de>,
        F: Fn(M::Key, M, &C) -> Option<M> + Send + Sync + 'static,
        C: 'static,
    {
        self.send_mappers
            .push(Arc::new(move |key: Value, msg: Value, ctx: &dyn Any| {
                let key: M::Key = serde_json::from_value(key)?;
                let msg: M = serde_json::from_value(msg)?;

                let ctx: &C = ctx.downcast_ref().expect("Invalid context type");

                mapper(key, msg, ctx).map(serde_json::to_value).transpose()
            }));
    }

    pub(crate) fn can_subscribe<C>(&self, key: Value, ctx: &C) -> bool
    where
        C: 'static,
    {
        let mut can_subscribe = true;

        for filter in &self.subscribe_filters {
            can_subscribe = can_subscribe && filter(key.clone(), ctx);
        }

        can_subscribe
    }

    pub(crate) fn map_msg<C>(&self, key: Value, msg: Value, ctx: &C) -> Option<Value>
    where
        C: 'static,
    {
        for mapper in &self.send_mappers {
            if let Ok(mapped_msg) = mapper(key.clone(), msg.clone(), ctx) {
                return mapped_msg;
            }
        }

        Some(msg)
    }
}

/// Broadcast a message from the server to the subscribers of the given key.
#[instrument]
pub async fn send<Msg>(key: &Msg::Key, msg: &Msg)
where
    Msg: SocketMsg + Serialize + Clone + Send + Sync + Debug + 'static,
    for<'de> Msg: Deserialize<'de>,
    Msg::Key: Hash + Eq + Serialize + Clone + Send + Sync + Debug + 'static,
    for<'de> Msg::Key: Deserialize<'de>,
    Msg::AppState: Clone,
    ServerSocket: FromRef<Msg::AppState>,
{
    let key = serde_json::to_value(key).unwrap();
    let msg = serde_json::to_value(msg).unwrap();

    let state: Msg::AppState = expect_context();

    ServerSocket::from_ref(&state).lock().await.send(key, msg);
}

/// Send a message from the server only to the connection that called this server function.
#[instrument]
pub async fn send_to_self<Msg>(key: &Msg::Key, msg: &Msg)
where
    Msg: SocketMsg + Serialize + Clone + Send + Sync + Debug + 'static,
    for<'de> Msg: Deserialize<'de>,
    Msg::Key: Hash + Eq + Serialize + Clone + Send + Sync + Debug + 'static,
    for<'de> Msg::Key: Deserialize<'de>,
    Msg::AppState: Clone,
    ServerSocket: FromRef<Msg::AppState>,
{
    let client_id = match extract_client_id() {
        Ok(id) => id,
        Err(err) => {
            error!("Failed to extract client ID: {}", err);
            return;
        }
    };

    let key = serde_json::to_value(key).unwrap();
    let msg = serde_json::to_value(msg).unwrap();

    let state: Msg::AppState = expect_context();

    ServerSocket::from_ref(&state)
        .lock()
        .await
        .send_to_self(client_id, key, msg)
        .await;
}

fn extract_client_id() -> Result<Uuid, String> {
    let cookie_header = header(COOKIE).ok_or("No cookie header found")?;

    // Parse value of cookie called socket_client_id
    let re = Regex::new(r"socket_client_id=([^;]+)").unwrap();
    let caps = re
        .captures(&cookie_header)
        .ok_or("socket_client_id cookie not found")?;
    let client_id_str = caps
        .get(1)
        .ok_or("socket_client_id cookie value not found")?;

    Uuid::parse_str(client_id_str.as_str())
        .map_err(|err| format!("Invalid UUID in socket_client_id cookie: {}", err))
}
