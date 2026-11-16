//! SYNTRAC desktop app (unelevated). It never collects anything: it reads the collector's database read-only and
//! asks the collector's pipe how it is doing.

pub mod backend;
pub mod commands;
pub mod db_location;
pub mod health;
pub mod history;
pub mod history_network;
pub mod history_programs;
pub mod identity_view;
pub mod known_view;
pub mod names;
pub mod owner_data_view;
pub mod series;
pub mod settings;
pub mod views;

use std::sync::Arc;

use syntrac_core::SystemClock;

use crate::backend::Backend;
use crate::settings::AppSettings;

pub fn run() {
    let backend: commands::SharedBackend = Arc::new(Backend::new(AppSettings::from_env(), SystemClock));
    tauri::Builder::default()
        .manage(backend)
        .invoke_handler(tauri::generate_handler![
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
            commands::get_new_destinations,
            commands::get_destination_history,
            commands::get_owner_data_status,
            commands::refresh_owner_data
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the SYNTRAC application");
}
