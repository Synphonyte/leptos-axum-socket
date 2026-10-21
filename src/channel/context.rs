use std::{collections::HashMap, sync::Arc};

use leptos::prelude::*;
use leptos::reactive::owner::WeakOwner;
use leptos_use_websocket::{ConnectionReadyState, ReconnectLimit};
use serde::Serialize;
use serde_json::Value;

use crate::{ChannelMsg, SocketMsg};

type SendFn = StoredValue<Arc<dyn Fn(&ChannelMsg) + Send + Sync + 'static>>;
type SimpleFn = StoredValue<Arc<dyn Fn() + Send + Sync + 'static>>;
type Handler = Arc<dyn Fn(&Value) + Send + Sync + 'static>;

#[cfg_attr(feature = "ssr", allow(dead_code))]
struct Subscriber {
    id: u64,
    /// The owner that subscribed, so the handler can use `expect_context` and friends. Weak,
    /// because a strong clone would keep an owner that is disposed by being dropped (a route,
    /// a `<For>` row) alive, and with it the cleanup that removes this subscriber.
    owner: Option<WeakOwner>,
    handler: Handler,
}

/// The context to be used for sending and subscribing to messages in your component.
/// You probably don't want to use this directly, but rather use the `expect_socket_context` hook.
#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct SocketContext {
    pub(crate) ready_state: Signal<ConnectionReadyState>,
    pub(crate) send: SendFn,
    pub(crate) open: SimpleFn,
    pub(crate) close: SimpleFn,
    pub(crate) message: Signal<Option<ChannelMsg>>,
    /// Handlers per key in subscription order. A key is subscribed on the server while it has an entry.
    subscribers: StoredValue<HashMap<Value, Vec<Subscriber>>>,
    next_subscription_id: StoredValue<u64>,
}

/// Returned by [`SocketContext::subscribe`]. Dropping it does nothing: the subscription ends
/// when the reactive owner that created it is disposed, or when
/// [`unsubscribe`](SubscriptionHandle::unsubscribe) is called.
#[derive(Clone)]
pub struct SubscriptionHandle {
    #[cfg_attr(feature = "ssr", allow(dead_code))]
    ctx: SocketContext,
    key: Value,
    id: u64,
}

impl std::fmt::Debug for SubscriptionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscriptionHandle")
            .field("key", &self.key)
            .field("id", &self.id)
            .finish()
    }
}

impl SubscriptionHandle {
    /// Stop calling this subscription's handler. The key is unsubscribed on the server once no
    /// other handler for it remains.
    pub fn unsubscribe(&self) {
        #[cfg(not(feature = "ssr"))]
        self.ctx.remove_handler(&self.key, self.id);
    }
}

impl SocketContext {
    fn new(query: String) -> Self {
        use crate::WEBSOCKET_CHANNEL_URL;
        use leptos::server::codee::string::JsonSerdeCodec;
        use leptos_use_websocket::{
            UseWebSocketOptions, UseWebSocketReturn, use_websocket_with_options,
        };

        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let url = format!("{WEBSOCKET_CHANNEL_URL}{query}");

        let UseWebSocketReturn {
            message,
            send,
            ready_state,
            open,
            close,
            ..
        } = use_websocket_with_options::<ChannelMsg, ChannelMsg, JsonSerdeCodec, _, _>(
            &url,
            UseWebSocketOptions::default()
                .reconnect_limit(ReconnectLimit::Infinite)
                .on_error(|error| {
                    leptos::logging::error!("WebSocket error: {}", error);
                }),
        );

        let this = Self {
            message,
            send: StoredValue::new(Arc::new(send)),
            ready_state,
            open: StoredValue::new(Arc::new(open)),
            close: StoredValue::new(Arc::new(close)),
            subscribers: StoredValue::new(HashMap::new()),
            next_subscription_id: StoredValue::new(0),
        };

        #[cfg(not(feature = "ssr"))]
        {
            // The server forgets all subscriptions when a connection closes.
            Effect::new(move || {
                if this.ready_state.get() == ConnectionReadyState::Open {
                    for key in this.subscribers.read_value().keys() {
                        this.send_raw(&ChannelMsg::Subscribe { key: key.clone() });
                    }
                }
            });

            // `watch` runs the handlers untracked, so a handler that reads a signal doesn't
            // re-run when it changes, and `immediate = false` keeps a new subscriber from
            // receiving the previous message. It also runs them without an owner, which
            // `dispatch` restores.
            Effect::watch(
                move || this.message.track(),
                move |_, _, _| this.dispatch(),
                false,
            );
        }

        this
    }

    #[cfg(not(feature = "ssr"))]
    fn send_raw(&self, msg: &ChannelMsg) {
        self.send.get_value()(msg);
    }

    #[cfg(not(feature = "ssr"))]
    fn dispatch(&self) {
        let Some(ChannelMsg::Msg { key, msg }) = self.message.get_untracked() else {
            return;
        };

        // Handlers may subscribe or unsubscribe, so none runs while the map is borrowed.
        let subscribers: Vec<(Option<WeakOwner>, Handler)> = self
            .subscribers
            .read_value()
            .get(&key)
            .map(|subscribers| {
                subscribers
                    .iter()
                    .map(|subscriber| (subscriber.owner.clone(), Arc::clone(&subscriber.handler)))
                    .collect()
            })
            .unwrap_or_default();

        for (owner, handler) in subscribers {
            match owner {
                Some(owner) => {
                    // A disposed owner's cleanup removes its subscriber; nothing to run here.
                    if let Some(owner) = owner.upgrade() {
                        owner.with(|| handler(&msg));
                    }
                }
                None => handler(&msg),
            }
        }
    }

    #[cfg(not(feature = "ssr"))]
    fn remove_handler(&self, key: &Value, id: u64) {
        let last = {
            // Gone once the whole app is disposed.
            let Some(mut subscribers) = self.subscribers.try_write_value() else {
                return;
            };
            let Some(handlers) = subscribers.get_mut(key) else {
                return;
            };

            handlers.retain(|subscriber| subscriber.id != id);

            if handlers.is_empty() {
                subscribers.remove(key);
                true
            } else {
                false
            }
        };

        if last {
            self.send_unsubscribe(key);
        }
    }

    #[cfg(not(feature = "ssr"))]
    fn send_unsubscribe(&self, key: &Value) {
        if self.ready_state.get_untracked() == ConnectionReadyState::Open {
            self.send_raw(&ChannelMsg::Unsubscribe { key: key.clone() });
        }
    }

    /// Disconnects and re-connects the WebSocket. This helps if you want to reset the context on the server.
    /// For example, you can use this method to update the websocket handler context when the user logs out or in.
    ///
    /// Every subscription is re-sent once the new connection is open.
    pub fn reconnect(&self) {
        #[cfg(not(feature = "ssr"))]
        {
            self.close.get_value()();
            self.open.get_value()();
        }
    }

    /// When someone sends a message with the given key, the handler will be called.
    ///
    /// The subscription ends when the current reactive owner (usually the component) is
    /// disposed, or when [`SubscriptionHandle::unsubscribe`] is called on the returned handle.
    /// Dropping the handle does nothing. Several subscribers of one key each receive every
    /// message; the key is unsubscribed on the server when the last one leaves.
    pub fn subscribe<Msg>(
        self,
        key_value: Msg::Key,
        handler: impl Fn(&Msg) + Send + Sync + 'static,
    ) -> SubscriptionHandle
    where
        Msg: SocketMsg + serde::Serialize + Clone,
        for<'de> Msg: serde::Deserialize<'de>,
        Msg::Key: serde::Serialize,
        for<'de> Msg::Key: serde::Deserialize<'de>,
    {
        let key_value = serde_json::to_value(key_value)
            .map_err(|err| {
                leptos::logging::error!("Failed to serialize key: {}", err);
            })
            .unwrap();

        let id = self.next_subscription_id.get_value();
        self.next_subscription_id.set_value(id + 1);

        let handle = SubscriptionHandle {
            ctx: self,
            key: key_value,
            id,
        };

        #[cfg(feature = "ssr")]
        {
            let _ = handler;
        }

        #[cfg(not(feature = "ssr"))]
        {
            let handler: Handler =
                Arc::new(
                    move |msg: &Value| match <Msg as serde::Deserialize>::deserialize(msg) {
                        Ok(msg) => handler(&msg),
                        Err(err) => {
                            leptos::logging::error!("Failed to deserialize message: {}", err);
                        }
                    },
                );

            let first = {
                let mut subscribers = self.subscribers.write_value();
                let handlers = subscribers.entry(handle.key.clone()).or_default();
                handlers.push(Subscriber {
                    id,
                    owner: Owner::current().map(|owner| owner.downgrade()),
                    handler,
                });
                handlers.len() == 1
            };

            if first && self.ready_state.get_untracked() == ConnectionReadyState::Open {
                self.send_raw(&ChannelMsg::Subscribe {
                    key: handle.key.clone(),
                });
            }

            on_cleanup({
                let handle = handle.clone();

                move || handle.unsubscribe()
            });
        }

        handle
    }

    /// Stop listening for messages with the given key, for every subscriber of it.
    pub fn unsubscribe<Key>(self, key: Key)
    where
        Key: serde::Serialize,
    {
        #[cfg(feature = "ssr")]
        {
            let _ = key;
        }

        #[cfg(not(feature = "ssr"))]
        {
            let key_value = serde_json::to_value(key)
                .map_err(|err| {
                    leptos::logging::error!("Failed to serialize key: {}", err);
                })
                .unwrap();

            if self.subscribers.write_value().remove(&key_value).is_some() {
                self.send_unsubscribe(&key_value);
            }
        }
    }

    /// Broadcast a message to all subscribers of the given key.
    ///
    /// The server only forwards it if a send mapper for the message type accepts it. See
    /// `ServerSocketInner::add_send_mapper`.
    pub fn send<Msg>(self, key: Msg::Key, msg: Msg)
    where
        Msg: SocketMsg + serde::Serialize + Clone,
        for<'de> Msg: serde::Deserialize<'de>,
        Msg::Key: serde::Serialize,
        for<'de> Msg::Key: serde::Deserialize<'de>,
    {
        #[cfg(feature = "ssr")]
        {
            let _ = key;
            let _ = msg;
        }

        #[cfg(not(feature = "ssr"))]
        {
            let key_value = serde_json::to_value(key)
                .map_err(|err| {
                    leptos::logging::error!("Failed to serialize key: {}", err);
                })
                .unwrap();

            let msg_value = serde_json::to_value(msg)
                .map_err(|err| {
                    leptos::logging::error!("Failed to serialize message: {}", err);
                })
                .unwrap();

            self.send_raw(&ChannelMsg::Msg {
                msg: msg_value,
                key: key_value,
            });
        }
    }
}

/// Call this in your root component to provide the socket context.
#[inline(always)]
pub fn provide_socket_context() -> SocketContext {
    if let Some(ctx) = use_context::<SocketContext>() {
        ctx
    } else {
        let ctx = SocketContext::new("".to_string());
        provide_context(ctx);
        ctx
    }
}

/// Call this in your root component to provide the socket context.
///
/// ## Example
///
/// ```ignore
/// provide_socket_context_with_query(&[("user_id", "123456789")]);
/// ```
#[inline(always)]
pub fn provide_socket_context_with_query<T: Serialize + ?Sized>(query: &T) -> SocketContext {
    let query_string = serde_urlencoded::to_string(query).expect("Failed to serialize query");
    let ctx = SocketContext::new(query_string);
    provide_context(ctx);
    ctx
}

/// Call this when you want to subscribe or send a message in your component.
#[inline(always)]
pub fn expect_socket_context() -> SocketContext {
    expect_context()
}

/// Call this when you want to subscribe or send a message in your component.
#[inline(always)]
pub fn use_socket_context() -> Option<SocketContext> {
    use_context()
}
