use std::time::Duration;

use syntrac_core::{EventQuery, EventRepository, MetricsRepository, Pipeline, ProcessRepository};
use syntrac_store::SqliteStore;
use syntrac_testkit::contract::{
    event_repository_contract, flow_queries_contract, metrics_repository_contract, process_queries_contract,
    process_repository_contract, recent_flows_contract, retention_contract,
};
use syntrac_testkit::fixtures::{at_secs, beacon_scenario, metrics_sample, process_metric, POWERSHELL_PID};
use syntrac_testkit::MockTelemetrySource;

const WINDOW: Duration = Duration::from_secs(10);

fn in_memory_store() -> SqliteStore {
    SqliteStore::open_in_memory().expect("open in-memory store")
}

fn ingest_beacon_scenario(store: &mut SqliteStore) {
    let mut pipeline = Pipeline::new(WINDOW);
    pipeline.ingest(&mut MockTelemetrySource::new(beacon_scenario()), store).unwrap();
    pipeline.flush_all(store).unwrap();
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
fn sqlite_store_satisfies_the_metrics_contract() {
    metrics_repository_contract(in_memory_store);
}

#[test]
fn sqlite_store_satisfies_the_retention_contract() {
    retention_contract(in_memory_store);
}

#[test]
fn sqlite_store_satisfies_the_flow_queries_contract() {
    flow_queries_contract(in_memory_store);
}

#[test]
fn sqlite_store_satisfies_the_process_queries_contract() {
    process_queries_contract(in_memory_store);
}

#[test]
fn sqlite_store_satisfies_the_recent_flows_contract() {
    recent_flows_contract(in_memory_store);
}

#[test]
fn pid_reuse_is_attributed_correctly_through_sqlite() {
    let mut store = in_memory_store();
    ingest_beacon_scenario(&mut store);

    let first = store.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();
    let second = store.resolve(POWERSHELL_PID, at_secs(205)).unwrap().unwrap();
    assert_ne!(first, second);

    let first_events = store.query(&EventQuery { process: Some(first), ..EventQuery::default() }).unwrap();
    let second_events = store.query(&EventQuery { process: Some(second), ..EventQuery::default() }).unwrap();
    // start + 3 beacon flows + exit, versus start + 1 flow
    assert_eq!(first_events.len(), 5);
    assert_eq!(second_events.len(), 2);
}

#[test]
fn event_counts_by_kind_summarise_what_was_stored() {
    let mut store = in_memory_store();
    ingest_beacon_scenario(&mut store);

    let counts: std::collections::HashMap<String, u64> = store.event_counts_by_kind().unwrap().into_iter().collect();
    assert_eq!(counts.get("process_started"), Some(&4));
    assert_eq!(counts.get("process_exited"), Some(&1));
    assert_eq!(counts.get("network_flow"), Some(&4));
    assert_eq!(counts.get("network_activity"), None, "raw activity is never stored");
}

#[test]
fn data_survives_closing_and_reopening_the_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let expected = {
        let mut store = SqliteStore::open(&path).unwrap();
        ingest_beacon_scenario(&mut store);
        store.query(&EventQuery::default()).unwrap().len()
    };

    let reopened = SqliteStore::open(&path).unwrap();
    assert_eq!(reopened.query(&EventQuery::default()).unwrap().len(), expected);
    assert!(reopened.resolve(POWERSHELL_PID, at_secs(10)).unwrap().is_some());
}

#[test]
fn opening_an_existing_database_does_not_rerun_migrations_destructively() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");

    let expected = {
        let mut store = SqliteStore::open(&path).unwrap();
        ingest_beacon_scenario(&mut store);
        store.query(&EventQuery::default()).unwrap().len()
    };
    for _ in 0..3 {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SqliteStore::LATEST_SCHEMA_VERSION);
        assert_eq!(store.query(&EventQuery::default()).unwrap().len(), expected, "reopening must not alter data");
    }
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

#[test]
fn a_reader_sees_committed_data_while_the_writer_still_holds_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut writer = SqliteStore::open(&path).unwrap();
    ingest_beacon_scenario(&mut writer);

    let reader = SqliteStore::open_read_only(&path).expect("read-only open while a writer is active");
    assert_eq!(
        reader.query(&EventQuery::default()).unwrap().len(),
        writer.query(&EventQuery::default()).unwrap().len()
    );

    // The writer keeps working and the reader then sees the new data too.
    writer.append_metrics(&metrics_sample(500, 12.0, vec![process_metric(1, None, 3.0)], 0.0, 0)).unwrap();
    assert_eq!(reader.system_series(at_secs(0), at_secs(1000)).unwrap().len(), 1);
}

#[test]
fn a_reader_works_when_no_writer_is_running() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let mut writer = SqliteStore::open(&path).unwrap();
        ingest_beacon_scenario(&mut writer);
    } // writer closed: WAL files may be gone

    let reader = SqliteStore::open_read_only(&path).expect("read-only open with no writer");
    assert!(reader.resolve(POWERSHELL_PID, at_secs(10)).unwrap().is_some());
}

#[test]
fn a_read_only_store_refuses_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    drop(SqliteStore::open(&path).unwrap());

    let mut reader = SqliteStore::open_read_only(&path).unwrap();
    let result = reader.append_metrics(&metrics_sample(1, 1.0, vec![], 0.0, 0));
    assert!(result.is_err());
}

#[test]
fn opening_a_missing_database_read_only_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(SqliteStore::open_read_only(&dir.path().join("does-not-exist.db")).is_err());
}
