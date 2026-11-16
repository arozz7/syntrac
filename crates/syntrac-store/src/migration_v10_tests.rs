//! The v9 -> v10 migration (the owner dataset) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{parse_ip2asn, DatasetInfo, OwnerRepository, Timestamp};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V9: usize = 9;

fn build_v9_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V9] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V9 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
}

#[test]
fn upgrading_keeps_the_data_adds_empty_tables_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v9_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2);
    assert_eq!(store.dataset_info().unwrap(), None);
    assert_eq!(store.lookup_owner("1.0.0.1".parse().unwrap()).unwrap(), None);
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn a_stored_dataset_survives_closing_and_reopening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v9_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let ranges = parse_ip2asn("1.0.0.0\t1.0.0.255\t13335\tUS\tCloudflare\n").ranges;
        let info = DatasetInfo {
            source: "test".into(),
            downloaded_at: Timestamp::from_micros(7),
            rows: 1,
            bad_lines: 0,
            sha256: "00".into(),
        };
        store.replace_dataset(&ranges, &info).unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.dataset_info().unwrap().map(|i| i.rows), Some(1));
    assert_eq!(store.lookup_owner("1.0.0.77".parse().unwrap()).unwrap().map(|o| o.asn), Some(13335));
}
