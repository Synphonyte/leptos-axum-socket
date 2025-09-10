use std::fmt::Debug;

use serde::{Deserialize, Serialize};

mod context;
#[cfg(feature = "ssr")]
mod server;

pub use context::*;
use serde_json::Value;
#[cfg(feature = "ssr")]
pub use server::{ServerSocket, ServerSocketInner, send, send_to_self};

pub const WEBSOCKET_CHANNEL_URL: &str = "/socket-msg";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) enum ChannelMsg {
    Msg { key: Value, msg: Value },
    Subscribe { key: Value },
    Unsubscribe { key: Value },
}
