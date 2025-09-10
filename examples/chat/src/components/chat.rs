use leptos::{either::Either, ev::SubmitEvent, prelude::*};
use leptos_axum_socket::expect_socket_context;
use leptos_router::{hooks::use_params, params::Params};
use reactive_stores::Store;
use uuid::Uuid;

use crate::{
    User, UserStoreFields,
    components::Message,
    data::{ChatKey, ChatMsg},
    expect_user,
};

#[component]
pub fn Room() -> impl IntoView {
    #[derive(Params, PartialEq, Eq, Clone)]
    struct RoomParam {
        room_id: Option<String>,
    }

    let params = use_params::<RoomParam>();

    move || {
        params.get().map(|RoomParam { room_id }| {
            Chat(ChatProps {
                room_id: Uuid::parse_str(&room_id.unwrap()).unwrap(),
            })
        })
    }
}

#[component]
pub fn Chat(room_id: Uuid) -> impl IntoView {
    let user = expect_user();

    let authenticate = ServerAction::<Authenticate>::new();

    let auth_resource = Resource::new(
        move || authenticate.version().get(),
        move |_| async move { is_authenticated(room_id, user.id().get()).await },
    );

    view! {
        <Suspense>
            {move || Suspend::new(async move {
                auth_resource
                    .await
                    .map(|is_auth| {
                        if is_auth {
                            Either::Left(view! { <ChatInner room_id user /> })
                        } else {
                            Either::Right(view! { <PasswordInput room_id user authenticate /> })
                        }
                    })
            })}
        </Suspense>
    }
}

#[component]
pub fn ChatInner(room_id: Uuid, user: Store<User>) -> impl IntoView {
    let chat_key = ChatKey { room_id };

    let socket = expect_socket_context();

    let (messages, set_messages) = signal(Vec::<ChatMsg>::new());
    let (input_value, set_input_value) = signal(String::new());

    let handle_submit = move |event: SubmitEvent| {
        event.prevent_default();

        let message = input_value.get_untracked();
        if !message.trim().is_empty() {
            let user = user.read_untracked();

            let chat_msg = ChatMsg {
                id: Uuid::new_v4(),
                message: message.clone(),
                author: user.name.clone(),
                author_uuid: user.id,
            };

            socket.send(chat_key, chat_msg);

            set_input_value.set(String::new());
        }
    };

    let on_incoming_msg = move |chat_msg: &ChatMsg| {
        set_messages.write().push(chat_msg.clone());
    };

    socket.subscribe(chat_key, on_incoming_msg);

    view! {
        <div class="flex flex-col size-full">
            // Messages area
            <div class="overflow-y-auto flex-1 p-4 bg-gray-50">
                <div class="space-y-2">
                    <For
                        each=move || messages.get()
                        key=|msg| msg.id
                        children=move |msg| {
                            view! { <Message msg=msg /> }
                        }
                    />
                </div>
            </div>

            // Input area
            <form class="p-4 border-t border-gray-200" on:submit=handle_submit>
                <div class="flex space-x-2">
                    <input
                        type="text"
                        placeholder="Type a message..."
                        class="flex-1 py-2 px-3 rounded-md border border-gray-300 focus:border-transparent focus:ring-2 focus:ring-blue-500 focus:outline-none"
                        bind:value=(input_value, set_input_value)
                    />
                    <button
                        class="py-2 px-4 text-white bg-blue-500 rounded-md hover:bg-blue-600 focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 focus:outline-none"
                        class:opacity-50=move || input_value.get().trim().is_empty()
                        class:cursor-not-allowed=move || { input_value.get().trim().is_empty() }
                        type="submit"
                    >
                        "Send"
                    </button>
                </div>
            </form>
        </div>
    }
}

#[allow(unused_parens)]
#[component]
pub fn PasswordInput(
    room_id: Uuid,
    user: Store<User>,
    authenticate: ServerAction<Authenticate>,
) -> impl IntoView {
    view! {
        <div class="flex fixed inset-0 z-50 justify-center items-center bg-black bg-opacity-50">
            <div class="p-6 mx-4 w-96 max-w-sm bg-white rounded-lg shadow-xl">
                <h2 class="mb-4 text-xl font-semibold text-gray-900">"Private Room Access"</h2>
                <p class="mb-4 text-gray-600">
                    "This room is private. Please enter "<b>"anything"</b>" to continue."
                </p>

                <ActionForm action=(authenticate) {..} class="space-y-4">
                    <div>
                        <input
                            type="password"
                            name="password"
                            placeholder="Anything is valid"
                            class="py-2 px-3 w-full rounded-md border border-gray-300 focus:border-transparent focus:ring-2 focus:ring-blue-500 focus:outline-none"
                        />
                    </div>

                    <div class="flex justify-end space-x-3">
                        <button
                            type="submit"
                            class="py-2 px-4 text-gray-700 bg-gray-200 rounded-md hover:bg-gray-300 focus:ring-2 focus:ring-gray-500 focus:ring-offset-2 focus:outline-none"
                        >
                            "Cancel"
                        </button>
                        <button
                            type="submit"
                            class="py-2 px-4 text-white bg-blue-500 rounded-md hover:bg-blue-600 focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 focus:outline-none"
                        >
                            "Enter Room"
                        </button>
                    </div>

                    <input type="hidden" name="room_id" value=room_id.to_string() />
                    <input type="hidden" name="user_id" value=move || user.id().get().to_string() />
                </ActionForm>
            </div>
        </div>
    }
}

#[server]
pub async fn authenticate(
    password: String,
    room_id: Uuid,
    user_id: Uuid,
) -> Result<(), ServerFnError> {
    // Authenticate the user (always)

    let _ = password;

    let state: crate::AppState = expect_context();
    state
        .allowed_users
        .0
        .lock()
        .unwrap()
        .get_mut(&room_id)
        .unwrap()
        .insert(user_id);

    Ok(())
}

#[server]
pub async fn is_authenticated(room_id: Uuid, user_id: Uuid) -> Result<bool, ServerFnError> {
    let state: crate::AppState = expect_context();

    Ok(state
        .allowed_users
        .0
        .lock()
        .unwrap()
        .get(&room_id)
        .map(|users| users.contains(&user_id))
        // if room is public, allow always
        .unwrap_or(true))
}
