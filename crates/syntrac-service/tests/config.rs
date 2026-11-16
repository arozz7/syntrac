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
    assert_eq!(config.detail_retention_days, 14, "the approved default");
    assert_eq!((config.max_database_gb, config.min_free_space_gb, config.hard_floor_free_gb), (10, 20, 5));
    assert_eq!(config.purge_interval_minutes, 60);
    assert_eq!(config.rollup_interval_minutes, 5);
    assert_eq!(config.status_interval_secs, 30);
    assert_eq!(config.etw_silent_restart_secs, 180);
    assert_eq!(config.etw_max_restarts, 3);
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
    assert_eq!(config.detail_retention_days, 7, "the old key name still works");
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
        "detail_retention_days = 0",
        "max_database_gb = 0",
        "min_free_space_gb = 0",
        "hard_floor_free_gb = 0",
        "min_free_space_gb = 5
hard_floor_free_gb = 5",
        "min_free_space_gb = 4
hard_floor_free_gb = 5",
        "purge_interval_minutes = 0",
        "rollup_interval_minutes = 0",
        "top_processes = 0",
        "etw_silent_restart_secs = 0",
        "identity_interval_minutes = 0",
        "identity_files_per_pass = 0",
        "identity_backlog_pause_secs = 0",
        "identity_max_hash_mb = 0",
    ] {
        assert!(ServiceConfig::from_toml_str(bad).is_err(), "{bad} must be rejected");
    }
}

#[test]
fn the_shipped_example_config_parses() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/service.toml")).unwrap();
    ServiceConfig::from_toml_str(&text).expect("config/service.toml must stay valid");
}

#[test]
fn the_pipe_user_is_optional_and_a_valid_sid_is_accepted() {
    assert_eq!(ServiceConfig::from_toml_str("").unwrap().pipe_allowed_sid, None);
    let config = ServiceConfig::from_toml_str("pipe_allowed_sid = \"S-1-5-21-1-2-3-1001\"").unwrap();
    assert_eq!(config.pipe_allowed_sid.as_deref(), Some("S-1-5-21-1-2-3-1001"));
}

#[test]
fn a_malformed_pipe_sid_is_refused_at_load_time_so_it_can_never_reach_the_access_list() {
    for bad in ["x", "S-1-5-21-1)(A;;GA;;;WD", ""] {
        let text = format!("pipe_allowed_sid = \"{bad}\"");
        assert!(ServiceConfig::from_toml_str(&text).is_err(), "{bad:?} must be rejected");
    }
}

#[test]
fn the_silence_restart_can_be_tuned_or_switched_off_with_a_zero_budget() {
    let config = ServiceConfig::from_toml_str("etw_silent_restart_secs = 90\netw_max_restarts = 0").unwrap();
    assert_eq!((config.etw_silent_restart_secs, config.etw_max_restarts), (90, 0));
}

#[test]
fn the_new_retention_key_and_the_old_one_are_the_same_setting_and_cannot_both_be_given() {
    assert_eq!(ServiceConfig::from_toml_str("detail_retention_days = 21").unwrap().detail_retention_days, 21);
    assert!(
        ServiceConfig::from_toml_str("retention_days = 30\ndetail_retention_days = 14").is_err(),
        "two spellings of one setting would be ambiguous"
    );
}

#[test]
fn the_storage_limits_can_be_tuned() {
    let config =
        ServiceConfig::from_toml_str("max_database_gb = 4\nmin_free_space_gb = 12\nhard_floor_free_gb = 3").unwrap();
    assert_eq!((config.max_database_gb, config.min_free_space_gb, config.hard_floor_free_gb), (4, 12, 3));
}

#[test]
fn the_identity_worker_has_gentle_defaults_and_can_be_tuned() {
    let defaults = ServiceConfig::default();
    assert_eq!(
        (
            defaults.identity_interval_minutes,
            defaults.identity_files_per_pass,
            defaults.identity_backlog_pause_secs,
            defaults.identity_max_hash_mb
        ),
        (5, 5, 30, 512)
    );

    let tuned = ServiceConfig::from_toml_str("identity_files_per_pass = 2\nidentity_max_hash_mb = 64").unwrap();
    assert_eq!((tuned.identity_files_per_pass, tuned.identity_max_hash_mb), (2, 64));
    assert_eq!(tuned.identity_interval_minutes, 5, "the rest stay default");
}

#[test]
fn the_context_worker_is_on_by_default_every_thirty_seconds_and_can_be_tuned_or_switched_off() {
    let defaults = ServiceConfig::default();
    assert_eq!(
        (defaults.context_enabled, defaults.context_interval_secs, defaults.context_refresh_minutes),
        (true, 30, 10)
    );

    let tuned = ServiceConfig::from_toml_str("context_interval_secs = 60\ncontext_refresh_minutes = 30").unwrap();
    assert_eq!((tuned.context_interval_secs, tuned.context_refresh_minutes), (60, 30));

    let off = ServiceConfig::from_toml_str("context_enabled = false").unwrap();
    assert!(!off.context_enabled);
    assert_eq!(off.context_interval_secs, 30, "the rest stay default");
}

#[test]
fn a_zero_context_interval_or_refresh_is_refused() {
    for bad in ["context_interval_secs = 0", "context_refresh_minutes = 0"] {
        assert!(ServiceConfig::from_toml_str(bad).is_err(), "{bad} must be rejected");
    }
}

#[test]
fn downloading_owner_data_is_allowed_by_default_and_can_be_switched_off() {
    assert!(ServiceConfig::from_toml_str("").unwrap().owner_data_allowed);
    assert!(!ServiceConfig::from_toml_str("owner_data_allowed = false").unwrap().owner_data_allowed);
}
