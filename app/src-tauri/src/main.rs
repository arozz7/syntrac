//! SYNTRAC desktop shell. Everything lives in the library; this only starts it.

// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    syntrac_app_lib::run();
}
