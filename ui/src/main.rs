//! Naute's interface — Rust compiled to WebAssembly, hosted by Tauri.

mod api;
mod app;
mod dictate;
mod editors;
mod home;
mod i18n;
mod settings;
mod state;
mod util;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
