use leptos::prelude::*;
use leptos_router::components::Outlet;

use crate::components::Rooms;

#[component]
pub fn Layout() -> impl IntoView {
    view! {
        <div class="flex min-h-screen bg-gray-100">
            // Sidebar with chat rooms
            <aside class="w-64 bg-white border-r border-gray-200">
                <Rooms />
            </aside>

            // Main content area where nested routes will be displayed
            <main class="flex-1">
                <ErrorBoundary fallback=|error| format!("Error: {:?}", error)>
                    <Outlet />
                </ErrorBoundary>
            </main>
        </div>
    }
}
