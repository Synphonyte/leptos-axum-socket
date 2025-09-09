use leptos::prelude::*;

use crate::{data::ChatMsg, expect_user};

#[component]
pub fn Message(msg: ChatMsg) -> impl IntoView {
    let user_id = expect_user().read_untracked().id;

    let is_my_message = user_id == msg.author_uuid;

    view! {
        <div class=format!(
            "flex mb-4 {}",
            if is_my_message { "justify-end" } else { "justify-start" },
        )>
            <div class=format!(
                "max-w-xs lg:max-w-md px-4 py-2 rounded-lg break-words {}",
                if is_my_message {
                    "bg-blue-500 text-white rounded-br-none"
                } else {
                    "bg-gray-200 text-gray-800 rounded-bl-none"
                },
            )>
                <Show when=move || !is_my_message>
                    <div class="mb-1 text-xs font-semibold text-gray-600">{msg.author.clone()}</div>
                </Show>

                <div class="text-sm leading-relaxed">{msg.message}</div>
            </div>
        </div>
    }
}
