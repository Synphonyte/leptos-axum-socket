use std::sync::Arc;

use axum::{
    extract::{
        WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderValue, header},
    response::Response,
};
#[cfg(feature = "ssr")]
use cookie::{Cookie, SameSite};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::debug;
use uuid::Uuid;

use crate::{ChannelMsg, ServerSocket};

async fn handle_websocket_with_context<C>(
    ws: WebSocket,
    socket: ServerSocket,
    client_id: Uuid,
    context: C,
) where
    C: 'static,
{
    let (ws_tx, mut ws_rx) = ws.split();

    let ws_tx = Arc::new(Mutex::new(ws_tx));

    let (client_tx, client_rx) = mpsc::channel(16);

    socket
        .lock()
        .await
        .insert_client_sender(client_id, client_tx);

    tokio::spawn({
        let ws_tx = Arc::clone(&ws_tx);

        async move {
            recv_client_send(ws_tx, client_rx).await;
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Close(_) => {
                return;
            }
            Message::Text(text) => {
                debug!("Received Text: {text}");

                let mut socket = socket.lock().await;

                let msg: ChannelMsg = serde_json::from_str(text.as_str()).unwrap();

                match msg {
                    ChannelMsg::Subscribe { key } => {
                        if socket.can_subscribe(key.clone(), &context) {
                            let ws_tx = Arc::clone(&ws_tx);
                            let broadcast_rx = socket.subscribe(key.clone());

                            let handle = tokio::spawn(async move {
                                recv_broadcast(Arc::clone(&ws_tx), broadcast_rx).await;
                            });

                            socket.remember_handle(key, handle);
                        }
                    }
                    ChannelMsg::Unsubscribe { key } => {
                        socket.unsubscribe(key);
                    }
                    ChannelMsg::Msg { msg, key } => {
                        if let Some(msg) = socket.map_msg(key.clone(), msg.clone(), &context) {
                            socket.send(key, msg);
                        }
                    }
                }
            }
            _ => (),
        }
    }
}

async fn recv_client_send(
    ws_tx: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    mut client_rx: mpsc::Receiver<ChannelMsg>,
) {
    while let Some(msg) = client_rx.recv().await {
        if ws_tx
            .lock()
            .await
            .send(Message::text(serde_json::to_string(&msg).unwrap()))
            .await
            .is_err()
        {
            return; // disconnected.
        }
    }
}

async fn recv_broadcast(
    ws_tx: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    mut broadcast_rx: broadcast::Receiver<ChannelMsg>,
) {
    while let Ok(msg) = broadcast_rx.recv().await {
        if ws_tx
            .lock()
            .await
            .send(Message::text(serde_json::to_string(&msg).unwrap()))
            .await
            .is_err()
        {
            return; // disconnected.
        }
    }
}

/// This is used to handle the incoming WebSocket connection.
///
/// ```
/// # use axum::{extract::{State, WebSocketUpgrade}, response::Response};
/// # use leptos_axum_socket::{ServerSocket, handlers::upgrade_websocket};
/// #
/// #[cfg(feature = "ssr")]
/// pub async fn connect_to_websocket(
///     ws: WebSocketUpgrade,
///     State(socket): State<ServerSocket>,
/// ) -> Response {
///     // You could do authentication here
///
///     // Provide extra context like the user's ID for example that is passed to the permission filters
///     let ctx = ();
///
///     upgrade_websocket( ws, socket, ctx)
/// }
/// ```
pub fn upgrade_websocket<C>(ws: WebSocketUpgrade, socket: ServerSocket, context: C) -> Response
where
    C: Send + 'static,
{
    let client_id = uuid::Uuid::new_v4();

    let mut response = ws.on_upgrade(move |websocket| {
        handle_websocket_with_context(websocket, socket, client_id, context)
    });

    let headers = response.headers_mut();

    let cookie = Cookie::build(("socket_client_id", client_id.to_string()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .build();

    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie.to_string()).unwrap(),
    );

    response
}
