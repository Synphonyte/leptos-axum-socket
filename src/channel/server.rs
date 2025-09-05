use axum::extract::FromRef;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc};
use std::{fmt::Debug, hash::Hash};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::task::JoinHandle;
use tracing::{debug, instrument};

use crate::{ChannelMsg, SocketMsg};

/// Permission for a socket connection.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SocketPermission {
    /// Don't allow anything
    Deny,
    /// Allow read-only access (subscribing)
    ReadOnly,

    /// Allow full access (sending and subscribing)
    Allow,
}

impl SocketPermission {
    pub fn can_subscribe(self) -> bool {
        self >= Self::ReadOnly
    }

    pub fn can_send(self) -> bool {
        self == Self::Allow
    }
}

type PermissionFilterFn = Arc<dyn Fn(Value, &dyn Any) -> SocketPermission + Send + Sync>;

/// This is used on the server to manage socket connections.
#[derive(Clone, Default)]
pub struct ServerSocket {
    sender_map: Arc<Mutex<HashMap<Value, Sender<ChannelMsg>>>>,
    permission_filters: Arc<Mutex<Vec<PermissionFilterFn>>>,
    handles: Arc<Mutex<HashMap<Value, JoinHandle<()>>>>,
}

impl std::fmt::Debug for ServerSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSocketChannels")
            .field("sender_map", &self.sender_map.lock().unwrap())
            .field("filters", &self.permission_filters.lock().unwrap().len())
            .finish()
    }
}

impl ServerSocket {
    pub fn new() -> Self {
        Self::default()
    }

    #[instrument]
    fn sender(&self, key: Value) -> Sender<ChannelMsg> {
        let mut sender_map = self
            .sender_map
            .lock()
            .expect("Failed to acquire lock on sender map");

        let sender = sender_map.entry(key).or_insert_with(|| {
            debug!("Creating new sender for key");

            Sender::new(16)
        });
        sender.clone()
    }

    #[instrument]
    pub(crate) fn send(&self, key: Value, msg: Value) {
        if let Err(err) = self.sender(key.clone()).send(ChannelMsg::Msg { msg, key }) {
            debug!(
                "Failed to send message because there are no receivers: {:?}",
                err
            );
        }
    }

    #[instrument]
    pub(crate) fn subscribe(&self, key: Value) -> Receiver<ChannelMsg> {
        self.sender(key).subscribe()
    }

    pub(crate) fn remember_handle(&self, key: Value, handle: JoinHandle<()>) {
        self.handles
            .lock()
            .expect("Failed to acquire lock on handle map")
            .insert(key, handle);
    }

    pub(crate) fn unsubscribe(&self, key: Value) {
        if let Some(handle) = self
            .handles
            .lock()
            .expect("Failed to acquire lock on handle map")
            .remove(&key)
        {
            handle.abort();
        }
    }

    /// Add a permission filter to the server. Whenever someone wants to subscribe or send a message,
    /// the filter will be called with the key and context.
    /// It can then return a [`SocketPermission`] to control what is allowed. If multiple filters
    /// are found for a given key, the most restrictive permission is returned.
    pub fn add_permission_filter<K, C, F>(&self, filter: F)
    where
        for<'de> K: Deserialize<'de>,
        F: Fn(K, &C) -> SocketPermission + Send + Sync + 'static,
        C: 'static,
    {
        self.permission_filters
            .lock()
            .expect("Failed to acquire lock on filter map")
            .push(Arc::new(move |key: Value, ctx: &dyn Any| {
                let ctx: &C = ctx.downcast_ref().expect("Invalid context type");

                match serde_json::from_value(key) {
                    Ok(key) => filter(key, ctx),
                    Err(_) => {
                        // This filter doesn't apply to the key
                        SocketPermission::Allow
                    }
                }
            }));
    }

    pub(crate) fn get_permission<C>(&self, key: Value, ctx: &C) -> SocketPermission
    where
        C: 'static,
    {
        let mut perm = SocketPermission::Allow;

        for filter in self
            .permission_filters
            .lock()
            .expect("Failed to acquire lock on filter map")
            .iter()
        {
            perm = perm.min(filter(key.clone(), ctx));
        }

        perm
    }
}

/// Broadcast a message from the server to the subscribers of the given key.
#[instrument]
pub fn send<Msg>(key: &Msg::Key, msg: &Msg)
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

    ServerSocket::from_ref(&state).send(key, msg);
}
