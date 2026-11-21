//! The small settings file of the app (docs/ALERTS-PLAN.md, slice T1): whether notifications are on and the last finding announced.

use syntrac_app_lib::prefs::{prefs_path, Prefs, PrefsHandle};
use syntrac_core::llm::{AiSettings, DetailLevel};

#[test]
fn the_defaults_are_notifications_on_and_nothing_announced_yet() {
    let prefs = Prefs::default();
    assert!(prefs.notifications);
    assert_eq!(prefs.last_announced, None);
}

#[test]
fn a_missing_or_damaged_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(Prefs::load(&dir.path().join("missing.json")), Prefs::default());

    for damaged in ["", "not json", "{\"notifications\": 7}", "[1, 2]"] {
        let path = dir.path().join("damaged.json");
        std::fs::write(&path, damaged).unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default(), "{damaged:?}");
    }
}

#[test]
fn what_is_saved_comes_back_and_the_folder_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Syntrac").join("app.json");
    let prefs = Prefs { notifications: false, last_announced: Some(42), ..Prefs::default() };

    prefs.save(&path).unwrap();

    assert_eq!(Prefs::load(&path), prefs);
}

#[test]
fn a_field_a_newer_version_added_is_ignored_and_a_missing_field_takes_its_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.json");
    std::fs::write(&path, "{\"notifications\": false, \"something_new\": 1}").unwrap();
    assert_eq!(Prefs::load(&path), Prefs { notifications: false, last_announced: None, ..Prefs::default() });
}

#[test]
fn the_file_lives_next_to_the_other_per_user_data_and_is_not_guessed_without_a_folder() {
    let path = prefs_path(|key| (key == "LOCALAPPDATA").then(|| r"C:\Users\someone\AppData\Local".to_owned()));
    assert_eq!(
        path.unwrap(),
        std::path::PathBuf::from(r"C:\Users\someone\AppData\Local").join("Syntrac").join("app.json")
    );
    assert_eq!(prefs_path(|_| None), None);
    assert_eq!(prefs_path(|_| Some("  ".to_owned())), None);
}

#[test]
fn a_handle_is_shared_and_every_change_is_written_to_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.json");
    let handle = PrefsHandle::with_file(path.clone());
    let other = handle.clone();

    handle.set_notifications_on(false);
    other.set_last_announced(9);

    assert!(!other.notifications_on());
    assert_eq!(handle.last_announced(), Some(9));
    assert_eq!(Prefs::load(&path), Prefs { notifications: false, last_announced: Some(9), ..Prefs::default() });
}

#[test]
fn ai_help_is_off_by_default_and_an_older_file_without_it_still_loads() {
    assert_eq!(Prefs::default().ai, AiSettings::default());
    assert!(!Prefs::default().ai.enabled);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.json");
    std::fs::write(&path, "{\"notifications\": false, \"last_announced\": 5}").unwrap();
    let prefs = Prefs::load(&path);
    assert_eq!((prefs.notifications, prefs.last_announced), (false, Some(5)));
    assert_eq!(prefs.ai, AiSettings::default());
}

#[test]
fn the_ai_settings_are_kept_with_the_other_settings_and_written_on_every_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.json");
    let handle = PrefsHandle::with_file(path.clone());
    let settings = AiSettings {
        enabled: true,
        endpoint: "http://127.0.0.1:1234/v1".into(),
        model: "some-model".into(),
        detail_level: DetailLevel::Full,
    };

    handle.set_ai_settings(settings.clone());
    handle.set_notifications_on(false);

    assert_eq!(handle.ai_settings(), settings);
    let saved = Prefs::load(&path);
    assert_eq!(saved.ai, settings);
    assert!(!saved.notifications, "a later change to another setting does not lose the AI settings");
}
