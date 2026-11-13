use syntrac_core::{ingest, EventQuery, EventRepository, ProcessRepository};
use syntrac_store::SqliteStore;
use syntrac_testkit::contract::{event_repository_contract, process_repository_contract};
use syntrac_testkit::fixtures::{at_secs, beacon_scenario, POWERSHELL_PID};
use syntrac_testkit::MockTelemetrySource;

fn in_memory_store() -> SqliteStore {
    SqliteStore::open_in_memory().expect("open in-memory store")
}

#[test]
fn sqlite_store_satisfies_the_process_contract() {
    process_repository_contract(in_memory_store);
}

#[test]
fn sqlite_store_satisfies_the_event_contract() {
    event_repository_contract(in_memory_store);
}

#[test]
fn pid_reuse_is_attributed_correctly_through_sqlite() {
    let mut store = in_memory_store();
    ingest(&mut MockTelemetrySource::new(beacon_scenario()), &mut store).unwrap();

    let first = store.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();
    let second = store.resolve(POWERSHELL_PID, at_secs(205)).unwrap().unwrap();
    assert_ne!(first, second);

    let first_events = store.query(&EventQuery { process: Some(first), ..EventQuery::default() }).unwrap();
    let second_events = store.query(&EventQuery { process: Some(second), ..EventQuery::default() }).unwrap();
    // start + 3 beacons + exit, versus start + 1 connection
    assert_eq!(first_events.len(), 5);
    assert_eq!(second_events.len(), 2);
}

#[test]
fn data_survives_closing_and_reopening_the_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");

    {
        let mut store = SqliteStore::open(&path).unwrap();
        ingest(&mut MockTelemetrySource::new(beacon_scenario()), &mut store).unwrap();
    }

    let reopened = SqliteStore::open(&path).unwrap();
    let stored = reopened.query(&EventQuery::default()).unwrap();
    assert_eq!(stored.len(), beacon_scenario().len());
    assert!(reopened.resolve(POWERSHELL_PID, at_secs(10)).unwrap().is_some());
}

#[test]
fn opening_an_existing_database_does_not_rerun_migrations_destructively() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");

    for _ in 0..3 {
        let mut store = SqliteStore::open(&path).unwrap();
        ingest(&mut MockTelemetrySource::new(Vec::new()), &mut store).unwrap();
    }
    assert_eq!(SqliteStore::open(&path).unwrap().schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
}

#[test]
fn a_database_from_a_newer_version_is_rejected_rather_than_corrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }
    assert!(SqliteStore::open(&path).is_err());
}
