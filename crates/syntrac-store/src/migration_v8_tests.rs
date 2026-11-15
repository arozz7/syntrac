//! The v7 -> v8 migration (address names) is purely additive: nothing existing is touched, and no backup is needed.

use std::net::{IpAddr, Ipv4Addr};

use rusqlite::Connection;
use syntrac_core::{backfill_names, EventEnvelope, EventPayload, RollupRepository, Timestamp};
use uuid::Uuid;

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V7: usize = 7;
const HOUR: i64 = 3_600_000_000;
const ADDR: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 10);

fn build_v7_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V7] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V7 as u32).unwrap();
    conn.execute(
        r"INSERT INTO executables (id, path_key, path, first_seen, last_seen) VALUES (2, 'c:\apps\a.exe', 'C:\Apps\A.exe', 10, 20)",
        [],
    )
    .unwrap();
    // A DNS lookup the way a v7 database holds it: only in the events table, no names anywhere.
    let payload = EventPayload::DnsQuery { pid: 7, query: "Example.COM.".into(), addresses: vec![IpAddr::V4(ADDR)] };
    let event = EventEnvelope {
        id: Uuid::from_u128(1),
        ts: Timestamp::from_micros(HOUR + 5),
        process_instance_id: None,
        severity: None,
        payload,
    };
    conn.execute(
        "INSERT INTO events (id, ts, category, kind, process_instance_id, severity, payload) VALUES (?1, ?2, 'dns', 'dns_query', NULL, NULL, ?3)",
        rusqlite::params![event.id.to_string(), event.ts.as_micros(), serde_json::to_string(&event.payload).unwrap()],
    )
    .unwrap();
}

#[test]
fn upgrading_keeps_the_data_adds_the_tables_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v7_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let executables: i64 = store.conn.query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0)).unwrap();
    assert_eq!(executables, 2, "the unattributed row and the executable that was there");
    let events: i64 = store.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap();
    assert_eq!(events, 1);
    assert!(store.address_names(IpAddr::V4(ADDR)).unwrap().is_empty(), "no names until the backfill runs");
    assert!(!store.names_backfilled().unwrap());
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn the_backfill_after_the_upgrade_finds_the_old_lookup_once_and_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v7_database(&path);
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let done = backfill_names(&mut store, Timestamp::from_micros(2 * HOUR)).unwrap();
        assert!(done.ran);
        assert_eq!(done.names, 1);
    }

    let mut store = SqliteStore::open(&path).unwrap();

    let names = store.address_names(IpAddr::V4(ADDR)).unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].name, "example.com");
    assert!(!backfill_names(&mut store, Timestamp::from_micros(2 * HOUR)).unwrap().ran, "done once, stays done");
}
