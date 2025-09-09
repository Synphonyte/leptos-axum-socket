#[cfg(feature = "ssr")]
mod socket_handlers;

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use std::collections::HashSet;

    use axum::Router;
    use chat::{
        AllowedUsers, AppState, ROOMS,
        app::{App, shell},
    };
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use leptos_axum_socket::{ServerSocket, SocketRoute};
    use tracing::debug;
    use tracing_subscriber::EnvFilter;

    use crate::socket_handlers::{connect_to_websocket, is_authenticated, sanitize_authenticated};

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    // Generate the list of routes in your Leptos App
    let routes = generate_route_list(App);

    let allowed_users = AllowedUsers::default();
    // Handle private rooms
    for room in ROOMS.iter() {
        if room.private {
            allowed_users
                .0
                .lock()
                .unwrap()
                .insert(room.id, HashSet::new());
        }
    }

    let state = AppState {
        leptos_options: conf.leptos_options,
        server_socket: ServerSocket::new(),
        allowed_users,
    };

    state
        .server_socket
        .lock()
        .add_subscribe_filter(is_authenticated);

    state
        .server_socket
        .lock()
        .add_send_mapper(sanitize_authenticated);

    let app = Router::new()
        .leptos_routes(&state, routes, {
            use chat::app::shell;

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

#[cfg(not(feature = "ssr"))]
pub fn main() {
    // no client-side main function
    // unless we want this to work with e.g., Trunk for pure client-side testing
    // see lib.rs for hydration function instead
}
