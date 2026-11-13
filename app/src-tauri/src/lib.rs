//! SYNTRAC desktop app (unelevated). It never collects anything: it reads the collector's database read-only and
//! asks the collector's pipe how it is doing.

pub mod backend;
pub mod commands;
pub mod health;
pub mod names;
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
            commands::list_connections
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the SYNTRAC application");
}
