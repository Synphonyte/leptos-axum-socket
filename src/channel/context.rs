use std::sync::Arc;

use leptos::prelude::*;
use leptos_use::core::ConnectionReadyState;

use crate::{ChannelMsg, SocketMsg};

type SendFn = StoredValue<Arc<dyn Fn(&ChannelMsg) + Send + Sync + 'static>>;
type SimpleFn = StoredValue<Arc<dyn Fn() + Send + Sync + 'static>>;

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
}

// #[cfg(not(feature = "ssr"))]
impl SocketContext {
    fn new() -> Self {
        use leptos::server::codee::string::JsonSerdeCodec;
        use leptos_use::{
            use_websocket_with_options, ReconnectLimit, UseWebSocketOptions, UseWebSocketReturn,
        };

        let UseWebSocketReturn {
            message,
            send,
            ready_state,
            open,
            close,
            ..
        } = use_websocket_with_options::<ChannelMsg, ChannelMsg, JsonSerdeCodec, _, _>(
            crate::WEBSOCKET_CHANNEL_URL,
            UseWebSocketOptions::default()
                .reconnect_limit(ReconnectLimit::Infinite)
                .on_error(|error| {
                    leptos::logging::error!("WebSocket error: {}", error);
                }),
        );

        Self {
            message,
            send: StoredValue::new(Arc::new(send)),
            ready_state,
            open: StoredValue::new(Arc::new(open)),
            close: StoredValue::new(Arc::new(close)),
        }
    }

    /// Disconnects and re-connects the WebSocket. This helps if you want to reset the context on the server.
    /// For example, you can use this method to reset the context when the user logs out.
    pub fn reconnect(&self) {
        self.close.get_value()();
        self.open.get_value()();
    }

    /// When someone sends a message with the given key, the handler will be called.
    pub fn subscribe<Msg>(self, key_value: Msg::Key, handler: impl Fn(&Msg) + 'static)
    where
        Msg: SocketMsg + serde::Serialize + Clone,
        for<'de> Msg: serde::Deserialize<'de>,
        Msg::Key: serde::Serialize,
        for<'de> Msg::Key: serde::Deserialize<'de>,
    {
        #[cfg(feature = "ssr")]
        {
            let _ = key_value;
            let _ = handler;
        }

        #[cfg(not(feature = "ssr"))]
        {
            let key_value = serde_json::to_value(key_value)
                .map_err(|err| {
                    leptos::logging::error!("Failed to serialize key: {}", err);
                })
                .unwrap();

            Effect::new({
                let key_value = key_value.clone();

                move || {
                    if self.ready_state.get() == ConnectionReadyState::Open {
                        self.send.get_value()(&ChannelMsg::Subscribe {
                            key: key_value.clone(),
                        });
                    }
                }
            });

            on_cleanup({
                let key_value = key_value.clone();

                move || {
                    self.unsubscribe(key_value);
                }
            });

            Effect::new(move || {
                if let Some(msg) = self.message.read().as_ref() {
                    match msg {
                        ChannelMsg::Msg { msg, key } if &key_value == key => {
                            match serde_json::from_value(msg.clone()) {
                                Err(err) => {
                                    leptos::logging::error!(
                                        "Failed to deserialize message: {}",
                                        err
                                    );
                                }
                                Ok(msg) => {
                                    handler(&msg);
                                }
                            }
                        }
                        _ => (),
                    }
                }
            });
        }
    }

    /// Stop listening for messages with the given key.
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

            self.send.get_value()(&ChannelMsg::Unsubscribe { key: key_value });
        }
    }

    /// Broadcast a message to all subscribers of the given key.
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

            self.send.get_value()(&ChannelMsg::Msg {
                msg: msg_value,
                key: key_value,
            });
        }
    }
}

/// Call this in your root component to provide the socket context.
#[inline(always)]
pub fn provide_socket_context() {
    provide_context(SocketContext::new());
}

/// Call this when you want to subscribe or send a message in your component.
#[inline(always)]
pub fn expect_socket_context() -> SocketContext {
    expect_context()
}
