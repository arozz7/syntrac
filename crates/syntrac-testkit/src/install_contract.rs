//! Contract suite for install records: every `InstallRepository` (in-memory, SQLite, ...) must pass these.

use syntrac_core::{InstallRecord, InstallRepository, Timestamp};

pub fn install_contract<R: InstallRepository>(make: impl Fn() -> R) {
    nothing_recorded_means_nothing_listed(make());
    a_record_is_stored_with_every_field_and_is_current(make());
    recording_again_keeps_the_first_sighting_and_takes_the_newest_text(make());
    a_record_missing_from_a_complete_read_is_no_longer_current_but_is_kept(make());
    the_same_key_in_two_registry_views_is_two_records(make());
    an_older_report_never_moves_last_seen_back_or_overwrites_newer_text(make());
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

fn record(hive: &str, key: &str, name: &str, version: &str) -> InstallRecord {
    InstallRecord {
        hive: hive.to_owned(),
        key_name: key.to_owned(),
        display_name: name.to_owned(),
        publisher: Some("Example, Inc.".to_owned()),
        version: Some(version.to_owned()),
        install_date: Some("20260504".to_owned()),
        location: Some(r"c:\program files\example".to_owned()),
        derived: false,
        system_component: false,
    }
}

fn nothing_recorded_means_nothing_listed<R: InstallRepository>(repo: R) {
    assert!(repo.installs().unwrap().is_empty());
}

fn a_record_is_stored_with_every_field_and_is_current<R: InstallRepository>(mut repo: R) {
    let mut stored = record("machine", "{A}", "Example", "1.0");
    stored.derived = true;
    stored.system_component = true;
    stored.location = None;

    let write = repo.record_installs(std::slice::from_ref(&stored), at(10)).unwrap();

    assert_eq!((write.written, write.new), (1, 1));
    let all = repo.installs().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].record, stored);
    assert_eq!((all[0].first_seen, all[0].last_seen, all[0].current), (at(10), at(10), true));
}

fn recording_again_keeps_the_first_sighting_and_takes_the_newest_text<R: InstallRepository>(mut repo: R) {
    repo.record_installs(&[record("machine", "{A}", "Example", "1.0")], at(10)).unwrap();

    let write = repo.record_installs(&[record("machine", "{A}", "Example", "2.0")], at(20)).unwrap();

    assert_eq!((write.written, write.new), (1, 0));
    let all = repo.installs().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].first_seen, all[0].last_seen), (at(10), at(20)));
    assert_eq!(all[0].record.version.as_deref(), Some("2.0"));
}

fn a_record_missing_from_a_complete_read_is_no_longer_current_but_is_kept<R: InstallRepository>(mut repo: R) {
    repo.record_installs(&[record("machine", "{A}", "A", "1"), record("machine", "{B}", "B", "1")], at(10)).unwrap();

    repo.record_installs(&[record("machine", "{A}", "A", "1")], at(20)).unwrap();

    let all = repo.installs().unwrap();
    let find = |name: &str| all.iter().find(|s| s.record.display_name == name).unwrap();
    assert!(find("A").current);
    assert!(!find("B").current);
    assert_eq!((find("B").first_seen, find("B").last_seen), (at(10), at(10)), "its history is kept");
}

fn the_same_key_in_two_registry_views_is_two_records<R: InstallRepository>(mut repo: R) {
    repo.record_installs(
        &[
            record("machine", "Tool", "Tool", "1"),
            record("machine32", "Tool", "Tool", "1"),
            record("user:S-1-5-21-1", "Tool", "Tool", "1"),
        ],
        at(10),
    )
    .unwrap();

    assert_eq!(repo.installs().unwrap().len(), 3);
}

fn an_older_report_never_moves_last_seen_back_or_overwrites_newer_text<R: InstallRepository>(mut repo: R) {
    repo.record_installs(&[record("machine", "{A}", "A", "2.0")], at(50)).unwrap();

    repo.record_installs(&[record("machine", "{A}", "A", "1.0")], at(30)).unwrap();

    let all = repo.installs().unwrap();
    assert_eq!((all[0].first_seen, all[0].last_seen), (at(30), at(50)));
    assert_eq!(all[0].record.version.as_deref(), Some("2.0"));
    assert!(all[0].current);
}
