use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::HeaderMap,
    response::Response,
};
use chat::{
    AllowedUsers, AppState,
    data::{ChatKey, ChatMsg},
};
use leptos_axum_socket::{ServerSocket, handlers::upgrade_websocket};
use serde::Deserialize;
use tracing::debug;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Q {
    user_id: Uuid,
}

#[derive(Clone)]
pub struct SocketCtx {
    user_id: Uuid,
    allowed_users: AllowedUsers,
}

#[axum::debug_handler(state = AppState)]
pub async fn connect_to_websocket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(socket): State<ServerSocket>,
    State(allowed_users): State<AllowedUsers>,
    Query(Q { user_id }): Query<Q>,
) -> Response {
    debug!("User {} connected", user_id);

    upgrade_websocket(
        ws,
        &headers,
        socket,
        SocketCtx {
            user_id,
            allowed_users,
        },
    )
}

pub async fn is_authenticated(key: ChatKey, socket_ctx: SocketCtx) -> bool {
    may_access_room(&socket_ctx, key.room_id)
}

pub fn sanitize_authenticated(
    key: ChatKey,
    msg: ChatMsg,
    socket_ctx: &SocketCtx,
) -> Option<ChatMsg> {
    may_access_room(socket_ctx, key.room_id).then(|| sanitize_message(msg))
}

fn may_access_room(socket_ctx: &SocketCtx, room_id: Uuid) -> bool {
    // Rooms without an entry are public
    socket_ctx
        .allowed_users
        .0
        .lock()
        .unwrap()
        .get(&room_id)
        .is_none_or(|users| users.contains(&socket_ctx.user_id))
}

fn sanitize_message(msg: ChatMsg) -> ChatMsg {
    // This is just a no op dummy implementation to show how this could work.
    msg
}
