//! The v13 -> v14 migration (changes of a file's content) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{IdentityChangeRepository, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V13: usize = 13;

#[test]
fn upgrading_keeps_the_data_adds_an_empty_table_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        for sql in &MIGRATIONS[..V13] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", V13 as u32).unwrap();
        conn.execute(
            r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO detect_state (id, switched_on_at) VALUES (1, 77)", []).unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    let switched_on: i64 = store.conn.query_row("SELECT switched_on_at FROM detect_state", [], |r| r.get(0)).unwrap();
    assert_eq!((executables, switched_on), (2, 77));
    assert!(store.identity_changes_since(Timestamp::from_micros(0)).unwrap().is_empty());
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}
