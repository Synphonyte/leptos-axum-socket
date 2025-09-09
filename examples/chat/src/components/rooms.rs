use leptos::prelude::*;
use leptos_router::components::A;

use crate::data::ChatRoom;

#[component]
pub fn Rooms() -> impl IntoView {
    view! {
        <Await
            future=get_rooms()
            children=|rooms| {
                let rooms_view = rooms
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|room| {
                        let name = room.name.clone();
                        let private = room.private;

                        view! {
                            <li class="mb-2">
                                <A
                                    href=format!("rooms/{}", room.id)
                                    {..}
                                    class="block py-2 px-4 font-medium text-gray-700 rounded-lg transition-colors duration-200 hover:text-blue-600 hover:bg-blue-50 aria-[current=page]:text-white aria-[current=page]:bg-blue-600"
                                >
                                    {name}

                                    <Show when=move || private>
                                        <svg
                                            class="inline ml-2 w-4 h-4 text-gray-500"
                                            fill="currentColor"
                                            viewBox="0 0 20 20"
                                            xmlns="http://www.w3.org/2000/svg"
                                        >
                                            <path
                                                fill-rule="evenodd"
                                                d="M5 9V7a5 5 0 0110 0v2a2 2 0 012 2v5a2 2 0 01-2 2H5a2 2 0 01-2-2v-5a2 2 0 012-2zm8-2v2H7V7a3 3 0 016 0z"
                                                clip-rule="evenodd"
                                            ></path>
                                        </svg>
                                    </Show>
                                </A>
                            </li>
                        }
                    })
                    .collect_view();

                view! {
                    <nav class="p-6 w-full min-h-screen">
                        <h2 class="pb-3 mb-6 text-xl font-bold text-gray-800 border-b border-gray-200">
                            "Chat Rooms"
                        </h2>
                        <ul class="space-y-1">{rooms_view}</ul>
                    </nav>
                }
            }
        />
    }
}

#[server]
pub async fn get_rooms() -> Result<Vec<ChatRoom>, ServerFnError> {
    // Simulate fetching rooms from a database
    Ok(crate::ROOMS.clone())
}
