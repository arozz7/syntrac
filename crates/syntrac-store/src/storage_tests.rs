//! Space recovery on a real database file: incremental auto-vacuum, and the file actually shrinking after a purge.

use rusqlite::Connection;
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{EventRepository, Retention, Timestamp};
use syntrac_testkit::fixtures::{at_secs, dns_query, EventFactory};

use crate::migrations::MIGRATIONS;
use crate::SqliteStore;

fn bulk_events(count: usize) -> Vec<syntrac_core::EventEnvelope> {
    let mut f = EventFactory::default();
    (0..count)
        .map(|i| f.make(at_secs(10 + i as i64), dns_query(1, &format!("host-number-{i}.example.org"), &[])))
        .collect()
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

#[test]
fn a_new_database_uses_incremental_auto_vacuum() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("syntrac.db")).unwrap();
    assert_eq!(store.auto_vacuum_mode().unwrap(), 2);
}

#[test]
fn an_existing_database_is_converted_once_and_keeps_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        // A database exactly as the previous version left it: current schema, auto-vacuum off.
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        for sql in MIGRATIONS {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as u32).unwrap();
        assert_eq!(conn.pragma_query_value(None, "auto_vacuum", |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.append(&bulk_events(50)).unwrap();
        assert_eq!(store.auto_vacuum_mode().unwrap(), 2, "converted by the first open");
        let notices = store.take_notices();
        assert!(
            notices.iter().any(|n| n.contains("incremental auto-vacuum")),
            "the host can log what happened: {notices:?}"
        );
        assert!(store.take_notices().is_empty(), "taking the notices clears them");
    }
    let mut again = SqliteStore::open(&path).unwrap();
    assert!(again.take_notices().is_empty(), "already converted: nothing to say and no second rewrite");
    assert_eq!(again.query_len(), 50);
}

impl SqliteStore {
    fn query_len(&self) -> usize {
        syntrac_core::EventRepository::query(self, &syntrac_core::EventQuery::default()).unwrap().len()
    }
}

#[test]
fn the_file_shrinks_after_a_purge_and_a_reclaim() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut store = SqliteStore::open(&path).unwrap();
    store.append(&bulk_events(20_000)).unwrap();
    let _ = store.reclaim_space(); // fold the log into the file so the size is the data's size
    let full = file_size(&path);

    store.purge_before(Timestamp::from_micros(i64::MAX)).unwrap();
    let usage = store.usage().unwrap();
    assert!(usage.reclaimable_bytes > full / 2, "the purge left free pages inside the file: {usage:?}");
    store.reclaim_space().unwrap();

    let after = file_size(&path);
    assert!(after < full / 4, "the file went from {full} to {after} bytes");
    assert_eq!(store.usage().unwrap().reclaimable_bytes, 0);
}

#[test]
fn reclaiming_works_while_a_reader_is_connected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut store = SqliteStore::open(&path).unwrap();
    store.append(&bulk_events(20_000)).unwrap();
    let _ = store.reclaim_space();
    let full = file_size(&path);

    // The UI's connection, idle between queries (the normal state).
    let reader = SqliteStore::open_read_only(&path).unwrap();
    let _ = reader.query_len();

    store.purge_before(Timestamp::from_micros(i64::MAX)).unwrap();
    store.reclaim_space().unwrap();

    assert!(file_size(&path) < full / 4, "an idle reader does not stop the file shrinking");
    assert_eq!(reader.query_len(), 0, "and the reader sees the purged state");
}

#[test]
fn a_reader_in_the_middle_of_a_read_never_makes_reclaiming_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut store = SqliteStore::open(&path).unwrap();
    store.append(&bulk_events(2_000)).unwrap();

    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN; SELECT COUNT(*) FROM events;").unwrap(); // holds a read snapshot open

    store.purge_before(Timestamp::from_micros(i64::MAX)).unwrap();
    assert!(store.reclaim_space().is_ok(), "the truncation may be delayed, but the call is not an error");

    reader.execute_batch("COMMIT").unwrap();
    store.reclaim_space().unwrap();
}

#[test]
fn a_conversion_that_cannot_happen_now_is_a_notice_not_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        for sql in MIGRATIONS {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as u32).unwrap();
    }
    // Another connection holds a read snapshot while the service starts.
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("BEGIN; SELECT COUNT(*) FROM events;").unwrap();

    let mut store = SqliteStore::open(&path).expect("opening must not fail because the conversion could not run");
    let notices = store.take_notices();
    let converted = store.auto_vacuum_mode().unwrap() == 2;
    assert!(
        converted || notices.iter().any(|n| n.contains("try again")),
        "either it worked or it said it will retry: {notices:?}"
    );
    assert!(store.query_len() == 0, "the store is usable either way");

    reader.execute_batch("COMMIT").unwrap();
    drop(store);
    let mut later = SqliteStore::open(&path).unwrap();
    assert_eq!(later.auto_vacuum_mode().unwrap(), 2, "the next start finishes the job");
    let _ = later.take_notices();
}
