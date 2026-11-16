//! The v11 -> v12 migration (install records) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{InstallRecord, InstallRepository, Retention, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V11: usize = 11;

fn build_v11_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V11] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V11 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO launcher_sweeps (kind, completed_at) VALUES ('task', 5)", []).unwrap();
}

fn product() -> InstallRecord {
    InstallRecord {
        hive: "machine".to_owned(),
        key_name: "{A}".to_owned(),
        display_name: "Example".to_owned(),
        publisher: None,
        version: Some("1.0".to_owned()),
        install_date: None,
        location: Some(r"c:\apps".to_owned()),
        derived: true,
        system_component: false,
    }
}

#[test]
fn upgrading_keeps_the_data_adds_an_empty_table_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v11_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    let sweeps: i64 = store.conn.query_row("SELECT COUNT(*) FROM launcher_sweeps", [], |r| r.get(0)).unwrap();
    assert_eq!((executables, sweeps), (2, 1));
    assert!(store.installs().unwrap().is_empty());
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn records_survive_reopening_and_the_purge_of_detail_and_do_not_disturb_the_launcher_sweeps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v11_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.record_installs(&[product()], Timestamp::from_micros(50)).unwrap();
        store.purge_before(Timestamp::from_micros(1_000_000_000_000_000)).unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();

    let all = store.installs().unwrap();
    assert_eq!((all.len(), all[0].current, all[0].record.derived), (1, true, true));
    let task_sweep: i64 =
        store.conn.query_row("SELECT completed_at FROM launcher_sweeps WHERE kind = 'task'", [], |r| r.get(0)).unwrap();
    assert_eq!(task_sweep, 5, "recording installs does not move another kind's sweep");
}
