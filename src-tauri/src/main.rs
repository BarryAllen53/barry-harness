#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod catalog;
mod coalescer;
mod commands;
mod errors;
mod guard;
mod logging;
mod menu;
mod navigation;
mod providers;
mod session;
mod turn;

use std::process::ExitCode;

use tauri::{Manager, WindowEvent, webview::PageLoadEvent};

use crate::commands::AppState;

fn main() -> ExitCode {
    logging::init();
    let webview_guard = tauri::plugin::Builder::<tauri::Wry>::new("webview-guard")
        .on_navigation(|webview, url| {
            let dev_url = tauri::is_dev()
                .then(|| webview.config().build.dev_url.clone())
                .flatten();
            let allowed = navigation::is_app_url(url, dev_url.as_ref());
            if !allowed {
                tracing::warn!(
                    scheme = url.scheme(),
                    "blocked navigation away from the app"
                );
            }
            allowed
        })
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                webview.state::<AppState>().reset_window(webview.label());
            }
        })
        .build();
    let result = tauri::Builder::default()
        .manage(AppState::from_environment())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main")
                && let Err(error) = window.clear_all_browsing_data()
            {
                tracing::warn!(%error, "the webview cache could not be cleared");
            }
            Ok(())
        })
        .menu(menu::build)
        .on_menu_event(|app, event| menu::on_event(app, event.id().as_ref()))
        .plugin(webview_guard)
        .invoke_handler(tauri::generate_handler![
            commands::send_prompt,
            commands::cancel_turn,
            commands::new_chat,
            account::settings_get,
            account::settings_set,
            account::credential_state,
            account::credential_set,
            account::credential_delete,
            account::sign_in,
            account::sign_in_paste,
            account::sign_in_code,
            account::sign_out,
            catalog::models_list,
            catalog::project_pick,
            catalog::project_remove,
            catalog::projects_list
        ])
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                window.state::<AppState>().reset_window(window.label());
            }
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "Barry stopped with an error");
            ExitCode::FAILURE
        }
    }
}
