//! The v6 -> v7 migration (known products) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{KnownProductRepository, NewKnownProduct, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V6: usize = 6;

fn build_v6_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V6] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V6 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
}

#[test]
fn upgrading_keeps_the_data_adds_the_table_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v6_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2, "the unattributed row and the executable that was there");
    assert!(store.known_products().unwrap().is_empty());
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn a_product_can_be_added_straight_after_the_upgrade_and_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v6_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let product = NewKnownProduct {
            label: "Acme".into(),
            publisher: Some("acme software, inc.".into()),
            root: r"c:\program files (x86)\acme".into(),
            product_name: None,
            note: None,
        };
        store.add_known_product(&product, Timestamp::from_micros(5)).unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.known_products().unwrap().len(), 1);
}
