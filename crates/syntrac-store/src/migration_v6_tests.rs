//! The v5 -> v6 migration (executable identity) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{IdentityRepository, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V5: usize = 5;

fn build_v5_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V5] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V5 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
}

#[test]
fn upgrading_keeps_the_executables_adds_the_table_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v5_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2, "the unattributed row and the executable that was there");
    let identities: i64 = store.conn.query_row("SELECT COUNT(*) FROM executable_identity", [], |r| r.get(0)).unwrap();
    assert_eq!(identities, 0);
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn the_executables_already_known_are_offered_for_inspection_straight_away() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v5_database(&path);
    let store = SqliteStore::open(&path).unwrap();

    let entries = store.identity_entries().unwrap();

    assert_eq!(entries.len(), 1, "the unattributed row is not an executable");
    assert_eq!(entries[0].exe.path, r"C:\Apps\A.exe");
    assert_eq!(entries[0].seen.unwrap().last_seen, Timestamp::from_micros(20));
    assert!(entries[0].record.is_none());
}

#[test]
fn opening_twice_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v5_database(&path);
    drop(SqliteStore::open(&path).unwrap());

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
}
