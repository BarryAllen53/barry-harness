mod app;
mod components;
mod ipc;
mod logic;

fn main() {
    leptos::mount::mount_to_body(app::App);
}
