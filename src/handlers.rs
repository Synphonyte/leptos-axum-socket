use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use tokio::sync::{broadcast::Receiver, Mutex};
use tracing::debug;

use crate::channel::{ChannelMsg, ServerSocket};

/// Use this to handle the incoming WebSocket connection. This is a shortcut for `handle_websocket_with_context` with an empty `()` context.
///
/// ```
/// #[cfg(feature = "ssr")]
/// pub async fn connect_to_websocket(
///     ws: WebSocketUpgrade,
///     State(socket): State<ServerSocket>,
/// ) -> Response {
///     ws.on_upgrade(|websocket| leptos_axum_socket::handlers::handle_websocket(websocket, socket))
/// }
/// ```
#[inline]
pub async fn handle_websocket(ws: WebSocket, socket: ServerSocket) {
    handle_websocket_with_context(ws, socket, ()).await;
}

/// Use this to handle the incoming WebSocket connection.
///
/// ```
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
///     ws.on_upgrade(|websocket| leptos_axum_socket::handlers::handle_websocket_with_context(websocket, socket, ctx))
/// }
/// ```
pub async fn handle_websocket_with_context<C>(ws: WebSocket, socket: ServerSocket, context: C)
where
    C: 'static,
{
    let (ws_tx, mut ws_rx) = ws.split();
    let ws_tx = Arc::new(Mutex::new(ws_tx));

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Close(_) => {
                return;
            }
            Message::Text(text) => {
                debug!("Received Text: {text}");

                let msg: ChannelMsg = serde_json::from_str(text.as_str()).unwrap();

                match msg {
                    ChannelMsg::Subscribe { key } => {
                        if socket.get_permission(key.clone(), &context).can_subscribe() {
                            let ws_tx = Arc::clone(&ws_tx);
                            let broadcast_rx = socket.subscribe(key.clone());

                            let handle = tokio::spawn(async move {
                                recv_broadcast(ws_tx.clone(), broadcast_rx).await;
                            });

                            socket.remember_handle(key, handle);
                        }
                    }
                    ChannelMsg::Unsubscribe { key } => {
                        socket.unsubscribe(key);
                    }
                    ChannelMsg::Msg { msg, key } => {
                        if socket.get_permission(key.clone(), &context).can_send() {
                            socket.send(key, msg);
                        }
                    }
                }
            }
            _ => (),
        }
    }
}

async fn recv_broadcast(
    client_tx: Arc<Mutex<SplitSink<WebSocket, Message>>>,
    mut broadcast_rx: Receiver<ChannelMsg>,
) {
    while let Ok(msg) = broadcast_rx.recv().await {
        if client_tx
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
