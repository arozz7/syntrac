//! Where the UI looks for the collector's database and pipe. Defaults match the installed service; the
//! environment can point it at a console collector instead.

use std::collections::HashMap;
use std::path::PathBuf;

use syntrac_app_lib::settings::AppSettings;

fn settings(pairs: &[(&str, &str)]) -> AppSettings {
    let env: HashMap<String, String> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    AppSettings::from_lookup(|key| env.get(key).cloned())
}

#[test]
fn defaults_point_at_the_installed_service() {
    let s = settings(&[("ProgramData", r"C:\ProgramData")]);
    assert_eq!(s.database_path, PathBuf::from(r"C:\ProgramData").join("Syntrac").join("data").join("syntrac.db"));
    assert_eq!(s.pipe_name, "syntrac-collector");
}

#[test]
fn a_missing_program_data_falls_back_to_the_standard_location() {
    let s = settings(&[]);
    assert!(s.database_path.starts_with(r"C:\ProgramData"), "{:?}", s.database_path);
}

#[test]
fn the_environment_can_point_the_ui_at_a_console_collector() {
    let s = settings(&[("SYNTRAC_DB", r"D:\src\syntrac\data\try.db"), ("SYNTRAC_PIPE", "syntrac-console")]);
    assert_eq!(s.database_path, PathBuf::from(r"D:\src\syntrac\data\try.db"));
    assert_eq!(s.pipe_name, "syntrac-console");
}

#[test]
fn empty_overrides_are_ignored_rather_than_producing_an_empty_path() {
    let s = settings(&[("ProgramData", r"C:\ProgramData"), ("SYNTRAC_DB", ""), ("SYNTRAC_PIPE", "  ")]);
    assert!(s.database_path.ends_with("syntrac.db") && s.database_path.starts_with(r"C:\ProgramData"));
    assert_eq!(s.pipe_name, "syntrac-collector");
}
