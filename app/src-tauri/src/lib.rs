//! SYNTRAC desktop app (unelevated). It never collects anything: it reads the collector's database read-only and
//! asks the collector's pipe how it is doing.

pub mod autostart;
pub mod autostart_registry;
pub mod backend;
pub mod commands;
pub mod db_location;
pub mod health;
pub mod history;
pub mod history_findings;
pub mod history_network;
pub mod history_programs;
pub mod identity_view;
pub mod known_view;
pub mod names;
pub mod owner_data_view;
pub mod prefs;
pub mod series;
pub mod settings;
pub mod toast_watch;
pub mod tray;
pub mod tray_text;
pub mod views;

use std::sync::Arc;

use syntrac_core::SystemClock;

use crate::autostart::StartMode;
use crate::backend::Backend;
use crate::prefs::{prefs_path, Prefs, PrefsHandle};
use crate::settings::AppSettings;

/// The settings file of the program (`%LOCALAPPDATA%\Syntrac\app.json`); kept in memory only when that folder is not known.
fn load_prefs() -> PrefsHandle {
    match prefs_path(|key| std::env::var(key).ok()) {
        Some(path) => PrefsHandle::with_file(path),
        None => PrefsHandle::in_memory(Prefs::default()),
    }
}

pub fn run() {
    let backend: commands::SharedBackend = Arc::new(Backend::new(AppSettings::from_env(), SystemClock));
    let prefs = load_prefs();
    let (watch_backend, watch_prefs) = (Arc::clone(&backend), prefs.clone());
    let app = tauri::Builder::default()
        // Only one copy runs: starting SYNTRAC again opens the window of the one that is running (it may be in the tray).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::open_main_window(app)))
        // Windows toasts for new suspicious findings, sent by `tray::start_watcher` with or without a window. No network.
        .plugin(tauri_plugin_notification::init())
        .manage(backend)
        .manage(prefs.clone())
        .setup(move |app| {
            tray::install(app.handle(), &prefs)?;
            tray::start_watcher(app.handle().clone(), watch_backend, watch_prefs);
            tray::repair_autostart();
            // Started by Windows at sign-in (`--tray`): only the tray icon, no window and no web view in memory.
            if StartMode::from_args(std::env::args()) == StartMode::Window {
                tray::open_main_window(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_notifications_on,
            commands::set_notifications_on,
            commands::get_health,
            commands::get_overview,
            commands::list_processes,
            commands::list_connections,
            commands::get_system_history,
            commands::get_top_programs,
            commands::get_known_programs,
            commands::mark_known_program,
            commands::unmark_known_program,
            commands::get_program_history,
            commands::get_top_destinations,
            commands::get_top_owners,
            commands::get_findings,
            commands::decide_finding,
            commands::remove_expectation,
            commands::get_review,
            commands::confirm_review,
            commands::confirm_review_programs,
            commands::get_program_destinations,
            commands::get_new_destinations,
            commands::get_destination_history,
            commands::get_owner_data_status,
            commands::refresh_owner_data
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the SYNTRAC application");
    app.run(|_app, event| {
        // Closing the last window is not the end: the program stays in the tray and goes on watching. Only Quit (an exit with a
        // code) ends it.
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
