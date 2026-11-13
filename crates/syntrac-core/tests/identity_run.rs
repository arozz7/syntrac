//! When an executable is due for a look: pure rules, no store and no files.

use std::time::Duration;

use syntrac_core::identity_run::is_due;
use syntrac_core::{
    retry_after, Executable, FileFacts, IdentityEntry, IdentityFailure, IdentityRecord, IdentitySettings,
    InspectErrorKind, LedgerEntry, Timestamp,
};

const HOUR: i64 = 3_600_000_000;

fn at(hours: i64) -> Timestamp {
    Timestamp::from_micros(hours * HOUR)
}

fn entry(last_seen: i64, record: Option<IdentityRecord>) -> IdentityEntry {
    IdentityEntry {
        exe: Executable::from_path(r"C:\Apps\a.exe").unwrap(),
        seen: Some(LedgerEntry { first_seen: at(0), last_seen: at(last_seen) }),
        record,
    }
}

fn looked_at(last_attempt: i64, failure: Option<u32>) -> IdentityRecord {
    IdentityRecord {
        exe: Executable::from_path(r"C:\Apps\a.exe").unwrap(),
        facts: Some(FileFacts::default()),
        inspected_at: Some(at(last_attempt)),
        last_attempt: at(last_attempt),
        failure: failure.map(|attempts| IdentityFailure {
            kind: InspectErrorKind::AccessDenied,
            message: String::new(),
            attempts,
        }),
    }
}

fn settings() -> IdentitySettings {
    IdentitySettings { max_per_pass: 10, recheck_every: Duration::from_secs(24 * 3600) }
}

#[test]
fn the_back_off_grows_then_settles_at_a_week() {
    let hours = |attempts| retry_after(attempts).as_secs() / 3600;

    assert_eq!([hours(0), hours(1), hours(2), hours(3), hours(4), hours(40)], [0, 1, 6, 24, 168, 168]);
}

#[test]
fn an_executable_never_looked_at_is_always_due() {
    assert!(is_due(&entry(5, None), at(5), &settings()));
}

#[test]
fn a_failure_is_due_only_after_its_back_off() {
    let failed_once = entry(5, Some(looked_at(10, Some(1))));
    assert!(!is_due(&failed_once, at(10), &settings()));
    assert!(is_due(&failed_once, at(11), &settings()), "one hour after the first failure");

    let failed_thrice = entry(5, Some(looked_at(10, Some(3))));
    assert!(!is_due(&failed_thrice, at(33), &settings()));
    assert!(is_due(&failed_thrice, at(34), &settings()));
}

#[test]
fn a_failure_is_retried_even_if_the_program_has_not_run_since() {
    assert!(is_due(&entry(1, Some(looked_at(10, Some(1)))), at(12), &settings()));
}

#[test]
fn a_good_look_is_repeated_only_after_the_program_ran_and_the_interval_passed() {
    let record = looked_at(10, None);

    assert!(!is_due(&entry(5, Some(record.clone())), at(100), &settings()), "has not run since");
    assert!(!is_due(&entry(20, Some(record.clone())), at(30), &settings()), "ran, but looked at less than a day ago");
    assert!(is_due(&entry(20, Some(record)), at(34), &settings()), "ran and a day has passed");
}

#[test]
fn an_executable_with_no_sightings_is_never_rechecked() {
    let mut unseen = entry(0, Some(looked_at(10, None)));
    unseen.seen = None;

    assert!(!is_due(&unseen, at(1000), &settings()));
}
