//! Service configuration: defaults, overrides, typo safety and validation.

use std::path::PathBuf;

use syntrac_service::ServiceConfig;

#[test]
fn an_empty_file_gives_the_documented_defaults() {
    let config = ServiceConfig::from_toml_str("").unwrap();
    assert_eq!(config.flow_window_secs, 10);
    assert_eq!(config.flow_flush_grace_secs, 3);
    assert_eq!(config.drain_interval_ms, 500);
    assert_eq!(config.metrics_interval_secs, 5);
    assert_eq!(config.top_processes, 25);
    assert_eq!(config.retention_days, 30);
    assert_eq!(config.purge_interval_minutes, 60);
    assert_eq!(config.status_interval_secs, 30);
    assert_eq!(config.database_path, None);
}

#[test]
fn listed_values_override_the_defaults_and_the_rest_stay_default() {
    let config = ServiceConfig::from_toml_str(
        r#"
        database_path = "D:/data/syntrac.db"
        retention_days = 7
        "#,
    )
    .unwrap();
    assert_eq!(config.database_path, Some(PathBuf::from("D:/data/syntrac.db")));
    assert_eq!(config.retention_days, 7);
    assert_eq!(config.flow_window_secs, 10);
}

#[test]
fn a_misspelled_key_is_an_error_not_silently_ignored() {
    let error = ServiceConfig::from_toml_str("retention_dayz = 7").unwrap_err();
    assert!(error.to_string().contains("retention_dayz"), "message should name the bad key: {error}");
}

#[test]
fn values_that_would_break_the_loop_are_rejected() {
    for bad in [
        "flow_window_secs = 0",
        "flow_flush_grace_secs = 0",
        "drain_interval_ms = 0",
        "metrics_interval_secs = 0",
        "retention_days = 0",
        "purge_interval_minutes = 0",
        "top_processes = 0",
    ] {
        assert!(ServiceConfig::from_toml_str(bad).is_err(), "{bad} must be rejected");
    }
}

#[test]
fn the_shipped_example_config_parses() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/service.toml")).unwrap();
    ServiceConfig::from_toml_str(&text).expect("config/service.toml must stay valid");
}
