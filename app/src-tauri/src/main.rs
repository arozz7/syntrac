//! SYNTRAC desktop shell (unelevated). Reads telemetry from the collector service's database
//! and talks to it over a named pipe; it never collects anything itself.

// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default().run(tauri::generate_context!()).expect("failed to run the SYNTRAC application");
}
