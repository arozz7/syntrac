//! The v12 -> v13 migration (findings, expectations, detection state) is purely additive: nothing existing is touched, and no backup
//! is needed.

use rusqlite::Connection;
use syntrac_core::findings::{Candidate, FindingWrite, NewExpectation, Tier};
use syntrac_core::{DetectionStateRepository, ExpectationRepository, FindingRepository, Retention, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V12: usize = 12;

fn build_v12_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V12] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V12 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO launcher_sweeps (kind, completed_at) VALUES ('task', 5)", []).unwrap();
}

fn candidate() -> Candidate {
    Candidate {
        kind: "new_program".into(),
        subject: r"c:\apps\a.exe".into(),
        subject_label: "A.exe".into(),
        tier: Tier::Unusual,
        title: "New program A.exe".into(),
        why: "Because.".into(),
        evidence: vec!["one".into()],
        dedupe_key: r"new_program:c:\apps\a.exe".into(),
        level: None,
        at: Timestamp::from_micros(50),
    }
}

#[test]
fn upgrading_keeps_the_data_adds_empty_tables_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v12_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    let sweeps: i64 = store.conn.query_row("SELECT COUNT(*) FROM launcher_sweeps", [], |r| r.get(0)).unwrap();
    assert_eq!((executables, sweeps), (2, 1));
    assert!(store.findings(None, 10).unwrap().is_empty());
    assert!(store.expectations().unwrap().is_empty());
    assert_eq!(store.detection_state().unwrap(), None);
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn what_was_stored_survives_reopening_and_the_purge_of_detail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v12_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.apply_writes(&[FindingWrite::Insert(candidate())]).unwrap();
        store
            .add_expectation(
                &NewExpectation {
                    kind: "new_program".into(),
                    subject: "x".into(),
                    note: Some("mine".into()),
                    source_finding: Some(1),
                    ceiling: Some(1.5),
                },
                Timestamp::from_micros(60),
            )
            .unwrap();
        store.switch_on(Timestamp::from_micros(40)).unwrap();
        store.purge_before(Timestamp::from_micros(1_000_000_000_000_000)).unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();

    let findings = store.findings(None, 10).unwrap();
    assert_eq!((findings.len(), findings[0].hits), (1, 1));
    let expectations = store.expectations().unwrap();
    assert_eq!((expectations[0].note.as_deref(), expectations[0].ceiling), (Some("mine"), Some(1.5)));
    assert_eq!(store.detection_state().unwrap().unwrap().switched_on_at, Timestamp::from_micros(40));
}

#[test]
fn the_same_finding_cannot_be_stored_twice() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    store.apply_writes(&[FindingWrite::Insert(candidate())]).unwrap();

    let second = store.apply_writes(&[FindingWrite::Insert(candidate())]);

    assert!(second.is_err(), "the dedupe key is unique, so a second insert of the same thing is refused");
    assert_eq!(store.findings(None, 10).unwrap().len(), 1);
}

#[test]
fn a_failed_batch_changes_nothing() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    let mut other = candidate();
    other.dedupe_key = "new_program:other".into();
    store.apply_writes(&[FindingWrite::Insert(candidate())]).unwrap();

    // The second write of this batch breaks the unique key, so the first must be rolled back too.
    let result = store.apply_writes(&[FindingWrite::Insert(other), FindingWrite::Insert(candidate())]);

    assert!(result.is_err());
    assert_eq!(store.findings(None, 10).unwrap().len(), 1, "the batch is one transaction");
}
