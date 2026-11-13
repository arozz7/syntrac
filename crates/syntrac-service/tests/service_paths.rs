//! A Windows service starts in System32, so every path must be absolute and the config must live where only
//! administrators can write it (SYSTEM reads it; a user-writable config would be a privilege-escalation path).

use std::path::{Path, PathBuf};

use syntrac_service::{service_config_path, validate_service_paths, ServiceConfig};

fn config(database: Option<&str>, logs: Option<&str>) -> ServiceConfig {
    ServiceConfig {
        database_path: database.map(PathBuf::from),
        log_dir: logs.map(PathBuf::from),
        ..ServiceConfig::default()
    }
}

#[test]
fn the_config_file_sits_beside_the_bin_folder_not_in_the_user_writable_data_folder() {
    let exe_dir = Path::new(r"C:\Program Files\Syntrac\bin");
    assert_eq!(service_config_path(exe_dir), PathBuf::from(r"C:\Program Files\Syntrac\config\service.toml"));
}

#[test]
fn absolute_database_and_log_paths_are_accepted() {
    let paths = validate_service_paths(&config(
        Some(r"C:\ProgramData\Syntrac\data\syntrac.db"),
        Some(r"C:\ProgramData\Syntrac\logs"),
    ))
    .unwrap();
    assert_eq!(paths.database_path, PathBuf::from(r"C:\ProgramData\Syntrac\data\syntrac.db"));
    assert_eq!(paths.log_dir, PathBuf::from(r"C:\ProgramData\Syntrac\logs"));
}

#[test]
fn a_relative_database_path_is_refused_because_the_working_directory_is_system32() {
    let error = validate_service_paths(&config(Some(r"data\syntrac.db"), Some(r"C:\logs"))).unwrap_err();
    assert!(error.to_string().contains("database_path"), "{error}");
}

#[test]
fn a_relative_log_dir_is_refused() {
    let error = validate_service_paths(&config(Some(r"C:\data\syntrac.db"), Some("logs"))).unwrap_err();
    assert!(error.to_string().contains("log_dir"), "{error}");
}

#[test]
fn a_missing_database_or_log_dir_is_refused_rather_than_guessed() {
    assert!(validate_service_paths(&config(None, Some(r"C:\logs"))).is_err());
    assert!(validate_service_paths(&config(Some(r"C:\data\syntrac.db"), None)).is_err());
}
