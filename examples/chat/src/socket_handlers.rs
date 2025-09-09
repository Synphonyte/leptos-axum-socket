use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use chat::{
    AllowedUsers, AppState,
    data::{ChatKey, ChatMsg},
};
use leptos_axum_socket::ServerSocket;
use serde::Deserialize;
use tracing::debug;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Q {
    user_id: Uuid,
}

pub struct SocketCtx {
    user_id: Uuid,
    allowed_users: AllowedUsers,
}

#[axum::debug_handler(state = AppState)]
pub async fn connect_to_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    // State(allowed_users): State<AllowedUsers>,
    Query(Q { user_id }): Query<Q>,
) -> Response {
    debug!("User {} connected", user_id);

    ws.on_upgrade(move |websocket| {
        leptos_axum_socket::handlers::handle_websocket_with_context(
            websocket,
            state.server_socket,
            SocketCtx {
                user_id,
                allowed_users: state.allowed_users,
            },
        )
    })
}

pub fn is_authenticated(key: ChatKey, socket_ctx: &SocketCtx) -> bool {
    // Check authentication
    if let Some(users) = socket_ctx.allowed_users.0.lock().unwrap().get(&key.room_id) {
        users.contains(&socket_ctx.user_id)
    } else {
        true
    }
}

pub fn sanitize_authenticated(
    key: ChatKey,
    msg: ChatMsg,
    socket_ctx: &SocketCtx,
) -> Option<ChatMsg> {
    // Check authentication
    if !is_authenticated(key, socket_ctx) {
        return None;
    }

    Some(sanitize_message(msg))
}

fn sanitize_message(msg: ChatMsg) -> ChatMsg {
    // This is just a no op dummy implementation to show how this could work.
    msg
}
