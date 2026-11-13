//! The v4 -> v5 migration (rollup tables) is purely additive: nothing existing is touched, and no backup is needed.

use rusqlite::Connection;
use syntrac_core::{roll_up_due, RollupRepository, RollupSettings};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

const V4: usize = 4;
const HOUR: i64 = 3_600_000_000;

fn build_v4_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    for sql in &MIGRATIONS[..V4] {
        conn.execute_batch(sql).unwrap();
    }
    conn.pragma_update(None, "user_version", V4 as u32).unwrap();
    // Two samples in hour 0 of the epoch, so there is something to roll up.
    for ts in [10 * 1_000_000_i64, 20 * 1_000_000] {
        conn.execute("INSERT INTO system_metrics VALUES (?1, 30.0, 8000, 16000, 1, 2, 3, 4, 0, 0.0, 0)", [ts]).unwrap();
    }
}

#[test]
fn upgrading_keeps_the_data_adds_the_tables_and_makes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v4_database(&path);

    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
    let samples: i64 = store.conn.query_row("SELECT COUNT(*) FROM system_metrics", [], |r| r.get(0)).unwrap();
    assert_eq!(samples, 2, "existing data is untouched");
    let unattributed: String =
        store.conn.query_row("SELECT path FROM executables WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(unattributed, "(unattributed)");
    assert_eq!(store.rolled_up_before().unwrap(), None, "no rollup has happened yet");
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("backup"))
        .count();
    assert_eq!(backups, 0, "an additive migration needs no backup copy");
}

#[test]
fn the_existing_detail_can_be_rolled_up_straight_after_the_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    build_v4_database(&path);
    let mut store = SqliteStore::open(&path).unwrap();

    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    let pass = roll_up_due(&mut store, syntrac_core::Timestamp::from_micros(3 * HOUR), &settings).unwrap();

    assert_eq!(
        pass.hours_rolled, 2,
        "hour 0 holds the two samples; hour 1 is empty but complete; hour 2 is not over yet"
    );
    let hours = store
        .system_hours(syntrac_core::Timestamp::from_micros(0), syntrac_core::Timestamp::from_micros(HOUR))
        .unwrap();
    assert_eq!(hours.len(), 1);
    assert_eq!(hours[0].samples, 2);
}
