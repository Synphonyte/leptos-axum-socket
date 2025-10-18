//! Realtime pub/sub communication for Leptos + Axum applications.
//!
//! ## Usage
//!
//! ```
//! # use leptos::prelude::*;
//! # use leptos_axum_socket::{expect_socket_context, ServerSocket, SocketMsg};
//! # use serde::{Serialize, Deserialize};
//! # use axum::extract::{State, FromRef};
//! #
//! # #[derive(FromRef, Clone)]
//! # pub struct AppState {
//! #     pub socket: ServerSocket,
//! # }
//! #
//! // Define the key and message types
//! #[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
//! pub struct MyKey {
//!     pub bla: String,
//! }
//!
//! #[derive(Clone, Serialize, Deserialize, Debug)]
//! pub struct MyMsg {
//!     pub awesome_msg: String,
//! }
//!
//! // Implement the SocketMsg trait for MyMsg to link the key and message types
//! impl SocketMsg for MyMsg {
//!     type Key = MyKey;
//!     #[cfg(feature = "ssr")]
//!     type AppState = AppState;
//! }
//!
//! #[component]
//! pub fn MyComponent() -> impl IntoView {
//!     let socket = expect_socket_context();
//!
//!     // Subscribe to receive messages that are sent with the given key
//!     socket.subscribe(
//!         MyKey {
//!             bla: "bla".to_string(),
//!         },
//!         |msg: &MyMsg| {
//!             // Simply log the message
//!             leptos::logging::log!("message: {msg:#?}");
//!         },
//!     );
//!
//!     let on_click = move || {
//!         // Send a message with the given key
//!         socket.send(
//!             MyKey {
//!                 bla: "bla".to_string(),
//!             },
//!             MyMsg {
//!                 awesome_msg: "awesome message".to_string(),
//!             },
//!         );
//!     };
//!
//!     view! { "..." }
//! }
//!
//! #[server]
//! pub async fn my_server_function() -> Result<(), ServerFnError> {
//!     // Send from the server
//!     leptos_axum_socket::send(
//!        &MyKey {
//!            bla: "bla".to_string(),
//!        },
//!        &MyMsg {
//!            awesome_msg: "Hello, world!".to_string(),
//!        },
//!     ).await;
//!
//!     Ok(())
//! }
//! ```
//!
//! For this to work you have to prepare a little bit.
//!
//! Define your app state in your lib.rs:
//!
//! ```
//! use leptos::prelude::*;
//!
//! #[cfg(feature = "ssr")]
//! #[derive(Clone, axum::extract::FromRef)]
//! pub struct AppState {
//!     // This is required for Leptos Axum Socket to work
//!     pub socket: leptos_axum_socket::ServerSocket,
//!
//!     // this is required for Leptos to work with axum
//!     pub leptos_options: LeptosOptions,
//! }
//! ```
//!
//! Initialize your Axum app (probably in main.rs):
//!
//! ```
//! # use leptos::prelude::*;
//! # use leptos_axum_socket::{ServerSocket, SocketMsg, SocketRoute, handlers::upgrade_websocket};
//! # use serde::{Deserialize, Serialize};
//! # use axum::{Router, extract::{State, WebSocketUpgrade, FromRef}, response::Response};
//! # use leptos_axum::{generate_route_list, LeptosRoutes};
//! #
//! # #[derive(Clone, FromRef)]
//! # pub struct AppState {
//! #     pub server_socket: ServerSocket,
//! #     pub leptos_options: LeptosOptions,
//! # }
//! #
//! # fn shell(options: LeptosOptions) -> impl IntoView {
//! #     ()
//! # }
//! # fn App() -> impl IntoView {
//! #     ()
//! # }
//! #
//! # #[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
//! # pub struct MyKey {
//! #     pub bla: String,
//! # }
//! #
//! # #[derive(Clone, Serialize, Deserialize, Debug)]
//! # pub struct MyMsg {
//! #     pub awesome_msg: String,
//! # }
//! #
//! # impl SocketMsg for MyMsg {
//! #     type Key = MyKey;
//! #     #[cfg(feature = "ssr")]
//! #     type AppState = AppState;
//! # }
//! #
//! #[tokio::main]
//! async fn main() {
//!     let conf = get_configuration(None).unwrap();
//!     let addr = conf.leptos_options.site_addr;
//!
//!     let routes = generate_route_list(App);
//!
//!     // Construct the Axum app state
//!     let state = AppState {
//!         leptos_options: conf.leptos_options,
//!         server_socket: ServerSocket::new(),
//!     };
//!
//!     // Optional: add subscription filters and message mappers
//!     {
//!         let mut server_socket = state.server_socket.lock().await;
//!         server_socket.add_subscribe_filter(async |key: MyKey, _ctx: &()| { key.bla == "bla" });
//!         server_socket.add_send_mapper(|key: MyKey, msg: MyMsg, _ctx: &()| {
//!             if key.bla == "bla" {
//!                 Some(MyMsg {
//!                     awesome_msg: msg.awesome_msg.replace("old", "new"),
//!                 })
//!             } else {
//!                 None
//!             }
//!         });
//!     }
//!
//!     // Init the Axum app
//!     let app: Router<AppState> = Router::new()
//!         .leptos_routes(&state, routes, {
//!             let leptos_options = state.leptos_options.clone();
//!             move || shell(leptos_options.clone())
//!         })
//!         .socket_route(connect_to_websocket)    // Register the socket route (implementation below)
//!         .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
//!         .with_state(state);    // Register the state
//!
//!     let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
//!     // axum::serve(listener, app.into_make_service())
//!     //    .await
//!     //    .unwrap();
//! }
//!
//! // Implement the `connect_to_websocket` handler:
//! #[cfg(feature = "ssr")]
//! pub async fn connect_to_websocket(
//!     ws: WebSocketUpgrade,
//!     State(socket): State<ServerSocket>,
//! ) -> Response {
//!     // You could do authentication here
//!
//!     // Provide extra context like the user's ID for example that is passed to the permission filters
//!     let ctx = ();
//!
//!     upgrade_websocket(ws, socket, ctx)
//! }
//! ```
//!
//! And finally provide the context in your root Leptos component:
//!
//! ```
//! # use leptos::prelude::*;
//! # use leptos_axum_socket::provide_socket_context;
//! #
//! #[component]
//! pub fn App() -> impl IntoView {
//!     provide_socket_context();
//!
//!     view! { "..." }
//! }
//! ```

pub mod channel;
#[cfg(feature = "ssr")]
pub mod handlers;

pub use crate::channel::*;

/// Implement this trait to link your socket message types to your key types.
/// In order to use this crate you have to implement this trait for your socket messages.
///
/// On the server you have to provide the application state as well.
///
/// ```
/// # use leptos_axum_socket::{ServerSocket, SocketMsg};
/// # use serde::{Serialize, Deserialize};
/// # use axum::extract::FromRef;
/// #
/// # #[derive(FromRef, Clone)]
/// # pub struct AppState {
/// #     pub socket: ServerSocket,
/// # }
/// #
/// // Define the key and message types
/// #[derive(Clone, Serialize, Deserialize)]
/// pub struct MyKey {
///     pub bla: String,
/// }
///
/// #[derive(Clone, Serialize, Deserialize, Debug)]
/// pub struct MyMsg {
///     pub awesome_msg: String,
/// }
///
/// // Implement the SocketMsg trait for MyMsg to link the key and message types
/// impl SocketMsg for MyMsg {
///     type Key = MyKey;
///     #[cfg(feature = "ssr")]
///     type AppState = AppState;
/// }
/// ```
pub trait SocketMsg {
    type Key;
    #[cfg(feature = "ssr")]
    type AppState;
}

/// Trait to extend the Axum router
#[cfg(feature = "ssr")]
pub trait SocketRoute<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Add the necessary websocket route to the Axum router
    fn socket_route<H, T>(self, handler: H) -> Self
    where
        H: axum::handler::Handler<T, S>,
        T: 'static;
}

#[cfg(feature = "ssr")]
impl<S> SocketRoute<S> for axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
    ServerSocket: axum::extract::FromRef<S>,
{
    fn socket_route<H, T>(self, handler: H) -> Self
    where
        H: axum::handler::Handler<T, S>,
        T: 'static,
    {
        use axum::routing::get;
        use tracing::debug;

        debug!("Adding websocket route to {WEBSOCKET_CHANNEL_URL}");

        self.route(WEBSOCKET_CHANNEL_URL, get(handler))
    }
}
