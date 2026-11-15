//! The v8 -> v9 migration (how programs have been seen running) is purely additive: nothing existing is touched, and no backup is
//! needed.

use rusqlite::Connection;
use syntrac_core::{classify_sid, ContextObservation, ContextRepository, Executable, ServiceInfo, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V8: usize = 8;
const SYSTEM: &str = "S-1-5-18";

fn build_v8_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V8] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V8 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
}

fn seen_as_system(path: &str) -> ContextObservation {
    ContextObservation {
        exe: Executable::from_path(path).unwrap(),
        sid: SYSTEM.to_owned(),
        account_name: Some(r"NT AUTHORITY\SYSTEM".to_owned()),
        kind: classify_sid(SYSTEM),
        services: vec![ServiceInfo { name: "acmeserv".into(), display_name: Some("Acme Service".into()) }],
    }
}

#[test]
fn upgrading_keeps_the_data_adds_the_tables_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v8_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2, "the unattributed row and the executable that was there");
    assert!(store.all_contexts().unwrap().is_empty());
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn a_program_the_rollups_know_gets_its_account_straight_after_the_upgrade_and_keeps_it_across_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v8_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.record_context(&[seen_as_system(r"C:\Apps\A.exe")], Timestamp::from_micros(30)).unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();

    let context = store.context_of(r"c:\apps\a.exe").unwrap();
    assert_eq!(context.accounts.len(), 1);
    assert_eq!(context.services[0].name, "acmeserv");
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2, "the existing executable row was used, not duplicated");
}
