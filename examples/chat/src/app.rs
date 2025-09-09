use leptos::prelude::*;
use leptos_axum_socket::provide_socket_context_with_query;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    components::{ParentRoute, Route, Router, Routes},
    path,
};

use crate::{UserStoreFields, components::Room, expect_user, layout::Layout, provide_user};

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
    provide_user();
    provide_socket_context_with_query(&[("user_id", expect_user().read_untracked().id)]);

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
                    <ParentRoute path=path!("") view=Layout>
                        <Route path=path!("/") view=Start />
                        <Route path=path!("/rooms/:room_id") view=Room />
                    </ParentRoute>
                </Routes>
            </main>
        </Router>
    }
}

#[component]
pub fn Start() -> impl IntoView {
    let user = expect_user();

    view! {
        <div class="p-6 mx-auto mt-8 max-w-md bg-white rounded-lg shadow-lg">
            <h1 class="mb-4 text-2xl font-bold">Welcome, {user.read().name.clone()}!</h1>
            <p class="mb-6 text-gray-600">Your user ID is {user.read().id.to_string()}.</p>

            <div class="space-y-4">
                <div>
                    <label for="username" class="block mb-2 text-sm font-medium text-gray-700">
                        "Change Username:"
                    </label>
                    <input
                        id="username"
                        type="text"
                        placeholder="Enter new username..."
                        class="py-2 px-3 w-full rounded-md border border-gray-300 focus:border-transparent focus:ring-2 focus:ring-blue-500 focus:outline-none"
                        bind:value=user.name()
                    />
                </div>
            </div>
        </div>
    }
}
