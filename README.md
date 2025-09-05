# Leptos Axum Socket

[![Crates.io](https://img.shields.io/crates/v/leptos-axum-socket.svg)](https://crates.io/crates/leptos-axum-socket)
[![Docs](https://docs.rs/leptos-axum-socket/badge.svg)](https://docs.rs/leptos-axum-socket/)
[![MIT/Apache 2.0](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/synphonyte/leptos-axum-socket#license)
[![Build Status](https://github.com/synphonyte/leptos-axum-socket/actions/workflows/cd.yml/badge.svg)](https://github.com/synphonyte/leptos-axum-socket/actions/workflows/cd.yml)

<!-- cargo-rdme start -->

Realtime pub/sub communication for Leptos + Axum applications.

### Usage

```rust
// Define the key and message types
#[derive(Clone, Serialize, Deserialize)]
pub struct MyKey {
    pub bla: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct MyMsg {
    pub awesome_msg: String,
}

// Implement the SocketMsg trait for MyMsg to link the key and message types
impl SocketMsg for MyMsg {
    type Key = MyKey;
    #[cfg(feature = "ssr")]
    type AppState = AppState;
}

#[component]
pub fn MyComponent() -> impl IntoView {
    let socket = expect_socket_context();

    // Subscribe to receive messages that are sent with the given key
    socket.subscribe(
        MyKey {
            bla: "bla".to_string(),
        },
        |msg: &MyMsg| {
            // Simply log the message
            leptos::logging::log!("message: {msg:#?}");
        },
    );

    let on_click = move || {
        // Send a message with the given key
        socket.send(
            MyKey {
                bla: "bla".to_string(),
            },
            MyMsg {
                awesome_msg: "awesome message".to_string(),
            },
        );
    };

    view! { "..." }
}

#[server]
pub async fn my_server_function() -> Result<(), ServerFnError> {
    // Send from the server
    leptos_axum_socket::send(
       &MyKey {
           bla: "bla".to_string(),
       },
       &MyMsg {
           awesome_msg: "Hello, world!".to_string(),
       },
    );

    Ok(())
}
```

For this to work you have to prepare a little bit.

Define your app state in your lib.rs:

```rust
use leptos::prelude::*;

#[cfg(feature = "ssr")]
#[derive(Clone, axum::extract::FromRef)]
pub struct AppState {
    // This is required for Leptos Axum Socket to work
    pub socket: leptos_axum_socket::ServerSocket,

    // this is required for Leptos to work with axum
    pub leptos_options: LeptosOptions,
}
```

Initialize your Axum app (probably in main.rs):

```rust
// Construct the Axum app state
let state = AppState {
    leptos_options: conf.leptos_options,
    server_socket: ServerSocket::new(),
};

// Optional: add permissions filters
state.server_socket.add_permission_filter(|key: MyKey, _ctx: &()| {
    if key.bla == "bla" {
        SocketPermission::Allow
    } else if key.bla == "bla2" {
        SocketPermission::ReadOnly
    } else {
        SocketPermission::Deny
    }
});

// Init the Axum app
let app = Router::new()
    .leptos_routes(&state, routes, {
        use basic::app::shell;

        let leptos_options = state.leptos_options.clone();
        move || shell(leptos_options.clone())
    })
    .socket_route(connect_to_websocket)    // Register the socket route (implementation below)
    .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
    .with_state(state);    // Register the state
```

Implement the `connect_to_websocket` handler:

```rust
#[cfg(feature = "ssr")]
pub async fn connect_to_websocket(
    ws: WebSocketUpgrade,
    State(socket): State<ServerSocket>,
) -> Response {
    // You could do authentication here

    // Provide extra context like the user's ID for example that is passed to the permission filters
    let ctx = ();

    ws.on_upgrade(|websocket| leptos_axum_socket::handlers::handle_websocket_with_context(websocket, socket, ctx))
}
```

And finally provide the context in your root Leptos component:

```rust
#[component]
pub fn App() -> impl IntoView {
    provide_socket_context();

    view! { "..." }
}
```

<!-- cargo-rdme end -->
