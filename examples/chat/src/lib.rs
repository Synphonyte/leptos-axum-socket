#[cfg(feature = "ssr")]
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

#[cfg(feature = "ssr")]
use lazy_static::lazy_static;
use leptos::prelude::*;
use reactive_stores::Store;
use uuid::Uuid;

#[cfg(feature = "ssr")]
use crate::data::ChatRoom;

pub mod app;
mod components;
pub mod data;
mod layout;

#[cfg(feature = "ssr")]
#[derive(Clone, axum::extract::FromRef)]
pub struct AppState {
    pub leptos_options: leptos::prelude::LeptosOptions,
    pub server_socket: leptos_axum_socket::ServerSocket,
    pub allowed_users: AllowedUsers,
}

#[cfg(feature = "ssr")]
#[derive(Clone, Default)]
pub struct AllowedUsers(pub Arc<Mutex<HashMap<Uuid, HashSet<Uuid>>>>);

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}

#[derive(Store, Clone, Debug)]
pub struct User {
    pub id: Uuid,
    pub name: String,
}

pub fn provide_user() {
    provide_context(Store::new(User {
        id: Uuid::new_v4(),
        name: "Some User".to_string(),
    }));
}

pub fn expect_user() -> Store<User> {
    expect_context()
}

#[cfg(feature = "ssr")]
lazy_static! {
    pub static ref ROOMS: Vec<ChatRoom> = vec![
        ChatRoom::new("Private", true),
        ChatRoom::new("General", false),
        ChatRoom::new("Development", false),
        ChatRoom::new("Marketing", false),
        ChatRoom::new("Sales", false),
    ];
}
