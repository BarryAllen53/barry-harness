use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{
    AppHandle, Emitter, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
};

pub const EVENT: &str = "menu";

static ZOOM: AtomicU64 = AtomicU64::new(0);

fn zoom_now() -> f64 {
    let bits = ZOOM.load(Ordering::Relaxed);
    if bits == 0 { 1.0 } else { f64::from_bits(bits) }
}

pub fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let new_chat = MenuItem::with_id(app, "new_chat", "New chat", true, Some("CmdOrCtrl+N"))?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, Some("CmdOrCtrl+,"))?;
    let quit = PredefinedMenuItem::quit(app, Some("Quit"))?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &new_chat,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let copy = PredefinedMenuItem::copy(app, Some("Copy"))?;
    let paste = PredefinedMenuItem::paste(app, Some("Paste"))?;
    let select_all = PredefinedMenuItem::select_all(app, Some("Select all"))?;
    let edit = Submenu::with_items(app, "Edit", true, &[&copy, &paste, &select_all])?;
    let zoom_in = MenuItem::with_id(app, "zoom_in", "Zoom in", true, Some("CmdOrCtrl+Plus"))?;
    let zoom_out = MenuItem::with_id(app, "zoom_out", "Zoom out", true, Some("CmdOrCtrl+-"))?;
    let zoom_reset =
        MenuItem::with_id(app, "zoom_reset", "Actual size", true, Some("CmdOrCtrl+0"))?;
    let view = Submenu::with_items(app, "View", true, &[&zoom_in, &zoom_out, &zoom_reset])?;
    let about = MenuItem::with_id(app, "about", "About Barry Harness", true, None::<&str>)?;
    let help = Submenu::with_items(app, "Help", true, &[&about])?;
    Menu::with_items(app, &[&file, &edit, &view, &help])
}

pub fn on_event(app: &AppHandle, id: &str) {
    match id {
        "zoom_in" => zoom(app, 0.1),
        "zoom_out" => zoom(app, -0.1),
        "zoom_reset" => set_zoom(app, 1.0),
        other => {
            if let Err(error) = app.emit(EVENT, other) {
                tracing::warn!(%error, action = other, "the menu event was not delivered");
            }
        }
    }
}

fn zoom(app: &AppHandle, delta: f64) {
    set_zoom(app, (zoom_now() + delta).clamp(0.5, 2.0));
}

fn set_zoom(app: &AppHandle, scale: f64) {
    ZOOM.store(scale.to_bits(), Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("main")
        && let Err(error) = window.set_zoom(scale)
    {
        tracing::warn!(%error, "the zoom level could not be applied");
    }
}
