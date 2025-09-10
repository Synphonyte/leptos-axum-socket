use leptos::{prelude::*, task::spawn_local};
use leptos_axum_socket::{expect_socket_context, provide_socket_context, SocketMsg};
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};
use serde::{Deserialize, Serialize};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    // Provides context that manages stylesheets, titles, meta tags, etc.
    provide_meta_context();
    provide_socket_context();

    view! {
        // injects a stylesheet into the document <head>
        // id=leptos means cargo-leptos will hot-reload this stylesheet
        <Stylesheet id="leptos" href="/pkg/lyte-admin-socket.css" />

        // sets the document title
        <Title text="Welcome to Leptos" />

        // content for this welcome page
        <Router>
            <main>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=HomePage />
                </Routes>
            </main>
        </Router>
    }
}

/// Renders the home page of your application.
#[component]
fn HomePage() -> impl IntoView {
    // Creates a reactive value to update the button
    let count = RwSignal::new(0);
    let on_click = move |_| {
        spawn_local(async move {
            bla().await.ok();
        })
    };

    expect_socket_context().subscribe(
        ToastKey {
            session_id: "bla".to_string(),
        },
        |msg: &ToastMsg| {
            leptos::logging::log!("message: {msg:#?}");
        },
    );

    view! {
        <h1>"Welcome to Leptos!"</h1>
        <button on:click=on_click>"Click Me: " {count}</button>
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ToastMsg {
    message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
struct ToastKey {
    session_id: String,
}

impl SocketMsg for ToastMsg {
    type Key = ToastKey;
    #[cfg(feature = "ssr")]
    type AppState = crate::AppState;
}

#[server]
pub async fn bla() -> Result<(), ServerFnError> {
    leptos::logging::log!("sending");

    leptos_axum_socket::send_to_self(
        &ToastKey {
            session_id: "bla".to_string(),
        },
        &ToastMsg {
            message: "Hello, world!".to_string(),
        },
    ).await;

    Ok(())
}
