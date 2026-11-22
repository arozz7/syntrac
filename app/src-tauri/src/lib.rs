//! SYNTRAC desktop app (unelevated). It never collects anything: it reads the collector's database read-only and
//! asks the collector's pipe how it is doing.

pub mod ai_commands;
pub mod ai_control;
pub mod ai_investigate;
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
pub mod history_tree;
pub mod identity_view;
pub mod investigate_evidence;
pub mod investigation_store;
pub mod known_view;
pub mod names;
pub mod owner_data_view;
pub mod prefs;
pub mod series;
pub mod server_process;
pub mod settings;
pub mod toast_watch;
pub mod tray;
pub mod tray_text;
pub mod views;

use std::sync::Arc;

use syntrac_core::SystemClock;

use crate::ai_control::{AiControl, ServiceFactory};
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
    // The optional AI help: nothing contacts a model server until the page asks, and nothing starts one until a button is pressed.
    let service: ServiceFactory =
        Arc::new(|endpoint| Box::new(syntrac_llm::HttpLlm::new(endpoint, syntrac_llm::HttpLimits::default())));
    // What a model server started by the app prints is saved next to the settings file, so a server that ends can be explained.
    let server_log = prefs_path(|key| std::env::var(key).ok()).map(|file| file.with_file_name("model-server.log"));
    let spawner = server_process::WindowsSpawner::with_optional_log(server_log.clone());
    let mut control = AiControl::new(prefs.clone(), Box::new(spawner), Arc::clone(&service));
    if let Some(log) = server_log {
        control = control.with_server_log(log);
    }
    let ai: ai_commands::SharedAi = Arc::new(control);
    let evidence_backend = Arc::clone(&backend);
    let evidence: ai_investigate::EvidenceProvider =
        Arc::new(move |subject| evidence_backend.investigation_evidence(subject));
    // Finished answers are kept next to the settings file, so a subject that was investigated before need not be asked about again.
    let store: Arc<dyn investigation_store::InvestigationStore> =
        match prefs_path(|key| std::env::var(key).ok()).map(|file| investigation_store::investigations_path(&file)) {
            Some(file) => Arc::new(investigation_store::JsonInvestigationStore::with_file(file)),
            None => Arc::new(investigation_store::JsonInvestigationStore::in_memory()),
        };
    let investigator: ai_commands::SharedInvestigator = Arc::new(
        ai_investigate::Investigator::new(
            prefs.clone(),
            Arc::clone(&ai),
            service,
            evidence,
            syntrac_core::investigate::PromptConfig::builtin(),
        )
        .with_store(store),
    );
    let exit_ai = Arc::clone(&ai);
    let (watch_backend, watch_prefs) = (Arc::clone(&backend), prefs.clone());
    let app = tauri::Builder::default()
        // Only one copy runs: starting SYNTRAC again opens the window of the one that is running (it may be in the tray).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::open_main_window(app)))
        // Windows toasts for new suspicious findings, sent by `tray::start_watcher` with or without a window. No network.
        .plugin(tauri_plugin_notification::init())
        .manage(backend)
        .manage(prefs.clone())
        .manage(ai)
        .manage(investigator)
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
            ai_commands::get_ai_settings,
            ai_commands::set_ai_settings,
            ai_commands::get_ai_status,
            ai_commands::start_ai_server,
            ai_commands::stop_ai_server,
            ai_commands::ai_preview_prompt,
            ai_commands::ai_start_investigation,
            ai_commands::ai_poll_investigation,
            ai_commands::ai_cancel_investigation,
            ai_commands::ai_saved_investigation,
            ai_commands::ai_saved_list,
            ai_commands::ai_forget_investigation,
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
            commands::get_process_tree,
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
    app.run(move |_app, event| {
        // Closing the last window is not the end: the program stays in the tray and goes on watching. Only Quit (an exit with a
        // code) ends it.
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        } else if let tauri::RunEvent::Exit = event {
            // The program is ending for good: stop the model server it started, if the user chose that.
            exit_ai.shutdown();
        }
    });
}
