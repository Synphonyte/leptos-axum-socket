#[cfg(feature = "ssr")]
use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
#[cfg(feature = "ssr")]
use leptos_axum_socket::{ServerSocket, handlers::upgrade_websocket};

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use basic::{
        app::{shell, App},
        AppState,
    };
    use leptos::prelude::*;
    use leptos_axum::{generate_route_list, LeptosRoutes};
    use leptos_axum_socket::{ServerSocket, SocketRoute};
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    // Generate the list of routes in your Leptos App
    let routes = generate_route_list(App);

    let state = AppState {
        leptos_options: conf.leptos_options,
        server_socket: ServerSocket::new(),
    };

    let app = Router::new()
        .leptos_routes(&state, routes, {
            use basic::app::shell;

            let leptos_options = state.leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .socket_route(connect_to_websocket)
        .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
        .with_state(state);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

#[cfg(feature = "ssr")]
pub async fn connect_to_websocket(
    ws: WebSocketUpgrade,
    State(socket): State<ServerSocket>,
) -> Response {
    upgrade_websocket(ws, socket, ())
}

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for pure client-side testing
    // see lib.rs for hydration function instead
}
