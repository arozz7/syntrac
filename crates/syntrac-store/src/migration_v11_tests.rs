//! The v10 -> v11 migration (launch records) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{Executable, LauncherKind, LauncherObservation, LauncherRepository, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V10: usize = 10;

fn build_v10_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V10] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V10 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
}

fn startup(path: &str) -> LauncherObservation {
    LauncherObservation {
        exe: Executable::from_path(path).unwrap(),
        kind: LauncherKind::RunKey,
        location: r"HKLM\Run\A".to_owned(),
        detail: String::new(),
        arguments: "/s".to_owned(),
    }
}

#[test]
fn upgrading_keeps_the_data_adds_empty_tables_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v10_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2);
    assert!(store.all_launchers().unwrap().is_empty());
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn a_program_the_rollups_know_gets_its_entry_straight_after_the_upgrade_and_keeps_it_across_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v10_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store
            .record_launchers(&[startup(r"C:\Apps\A.exe")], &[LauncherKind::RunKey], Timestamp::from_micros(50))
            .unwrap();
        let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
        assert_eq!(executables, 2, "the program already had its row");
    }

    let store = SqliteStore::open(&path).unwrap();

    let seen = store.launchers_of(r"C:\Apps\A.exe").unwrap();
    assert_eq!((seen.len(), seen[0].arguments.as_str(), seen[0].current), (1, "/s", true));
}

#[test]
fn the_purge_of_detail_does_not_touch_launch_records() {
    use syntrac_core::Retention;
    let mut store = SqliteStore::open_in_memory().unwrap();
    store.record_launchers(&[startup(r"C:\Apps\A.exe")], &[LauncherKind::RunKey], Timestamp::from_micros(50)).unwrap();

    store.purge_before(Timestamp::from_micros(1_000_000_000_000_000)).unwrap();

    assert_eq!(store.launchers_of(r"C:\Apps\A.exe").unwrap().len(), 1);
}
