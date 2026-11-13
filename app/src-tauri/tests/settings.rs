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
fn only_an_explicit_database_override_pins_the_location() {
    assert!(
        !settings(&[("ProgramData", r"C:\ProgramData")]).database_pinned,
        "the default can be updated by the service"
    );
    assert!(settings(&[("SYNTRAC_DB", r"J:\x\try.db")]).database_pinned, "a developer's choice must not be overridden");
    assert!(!settings(&[("SYNTRAC_DB", "  ")]).database_pinned, "an empty override is no override");
}

#[test]
fn the_last_known_database_path_is_remembered_in_the_users_local_app_data() {
    let s = settings(&[("LOCALAPPDATA", r"C:\Users\someone\AppData\Local")]);
    assert_eq!(
        s.memory_file,
        Some(PathBuf::from(r"C:\Users\someone\AppData\Local").join("Syntrac").join("database-path.txt"))
    );
    assert_eq!(settings(&[]).memory_file, None, "without a local app data folder nothing is remembered");
}

#[test]
fn a_settings_value_for_one_known_database_is_pinned_and_remembers_nothing() {
    let s = AppSettings::for_database(PathBuf::from(r"D:\x\a.db"), "p");
    assert_eq!(
        (s.database_path, s.pipe_name, s.database_pinned, s.memory_file),
        (PathBuf::from(r"D:\x\a.db"), "p".to_owned(), true, None)
    );
}

#[test]
fn empty_overrides_are_ignored_rather_than_producing_an_empty_path() {
    let s = settings(&[("ProgramData", r"C:\ProgramData"), ("SYNTRAC_DB", ""), ("SYNTRAC_PIPE", "  ")]);
    assert!(s.database_path.ends_with("syntrac.db") && s.database_path.starts_with(r"C:\ProgramData"));
    assert_eq!(s.pipe_name, "syntrac-collector");
}
