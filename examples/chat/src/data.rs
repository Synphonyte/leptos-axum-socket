use leptos_axum_socket::SocketMsg;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMsg {
    pub id: Uuid,
    pub message: String,
    pub author: String,
    pub author_uuid: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChatKey {
    pub room_id: Uuid,
}

impl SocketMsg for ChatMsg {
    type Key = ChatKey;
    #[cfg(feature = "ssr")]
    type AppState = crate::AppState;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatRoom {
    pub id: Uuid,
    pub name: String,
    pub private: bool,
}

impl ChatRoom {
    pub fn new(name: &str, private: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            private,
        }
    }
}
