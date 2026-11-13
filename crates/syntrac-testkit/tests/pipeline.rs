//! Proof of modularity: the whole ingest pipeline runs offline against mocks.

use syntrac_core::{ingest, EventPayload, EventQuery, EventRepository, ProcessRepository};
use syntrac_testkit::fixtures::{
    at_secs, beacon_scenario, process_started, tcp_connect, EventFactory, BEACON_TARGET, DROPPER_PID, EXPLORER_PID,
    POWERSHELL_PID,
};
use syntrac_testkit::{InMemoryRepository, MockTelemetrySource};

fn ingested_beacon_scenario() -> InMemoryRepository {
    let mut source = MockTelemetrySource::new(beacon_scenario());
    let mut repo = InMemoryRepository::default();
    ingest(&mut source, &mut repo).expect("ingest succeeds");
    repo
}

#[test]
fn every_scenario_event_is_stored() {
    let repo = ingested_beacon_scenario();
    let stored = repo.query(&EventQuery::default()).unwrap();
    assert_eq!(stored.len(), beacon_scenario().len());
}

#[test]
fn child_processes_link_to_their_parent_instance() {
    let repo = ingested_beacon_scenario();
    let dropper = repo.resolve(DROPPER_PID, at_secs(10)).unwrap().expect("dropper known");
    let powershell = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().expect("powershell known");

    let powershell_record = repo.get(powershell).unwrap().unwrap();
    assert_eq!(powershell_record.parent_id, Some(dropper));

    let dropper_record = repo.get(dropper).unwrap().unwrap();
    let explorer = repo.resolve(EXPLORER_PID, at_secs(10)).unwrap().unwrap();
    assert_eq!(dropper_record.parent_id, Some(explorer));
}

#[test]
fn beacon_connections_are_attributed_to_the_first_powershell_instance() {
    let repo = ingested_beacon_scenario();
    let powershell = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();

    let beacons: Vec<_> = repo
        .query(&EventQuery { process: Some(powershell), ..EventQuery::default() })
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.payload, EventPayload::NetworkConnection { remote_addr, .. } if remote_addr == BEACON_TARGET))
        .collect();
    assert_eq!(beacons.len(), 3);
}

#[test]
fn a_reused_pid_does_not_inherit_the_old_processes_connections() {
    let repo = ingested_beacon_scenario();
    let first = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();
    let second = repo.resolve(POWERSHELL_PID, at_secs(205)).unwrap().unwrap();
    assert_ne!(first, second);

    let late_connection = repo
        .query(&EventQuery { process: Some(second), ..EventQuery::default() })
        .unwrap()
        .into_iter()
        .find(|e| matches!(e.payload, EventPayload::NetworkConnection { .. }))
        .expect("second instance has its own connection");
    assert_eq!(late_connection.ts, at_secs(210));
}

#[test]
fn exit_closes_the_process_lifetime() {
    let repo = ingested_beacon_scenario();
    let first = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();
    assert_eq!(repo.get(first).unwrap().unwrap().ended, Some(at_secs(100)));
}

#[test]
fn a_connection_from_an_unknown_process_is_kept_but_unattributed() {
    let mut f = EventFactory::default();
    let mut source = MockTelemetrySource::new(vec![f.make(at_secs(5), tcp_connect(777, BEACON_TARGET, 443))]);
    let mut repo = InMemoryRepository::default();
    ingest(&mut source, &mut repo).unwrap();

    let stored = repo.query(&EventQuery::default()).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].process_instance_id, None);
}

#[test]
fn events_delivered_out_of_order_are_processed_by_timestamp() {
    let mut f = EventFactory::default();
    let connect = f.make(at_secs(5), tcp_connect(50, BEACON_TARGET, 443));
    let start = f.make(at_secs(1), process_started(50, 4, r"C:\late-delivery.exe"));
    let mut source = MockTelemetrySource::new(vec![connect, start]);
    let mut repo = InMemoryRepository::default();
    ingest(&mut source, &mut repo).unwrap();

    let attributed = repo.query(&EventQuery::default()).unwrap().into_iter().last().unwrap();
    assert!(attributed.process_instance_id.is_some());
}

#[test]
fn a_second_drain_with_no_new_events_stores_nothing() {
    let mut source = MockTelemetrySource::new(beacon_scenario());
    let mut repo = InMemoryRepository::default();
    let first = ingest(&mut source, &mut repo).unwrap();
    let second = ingest(&mut source, &mut repo).unwrap();
    assert_eq!(first, beacon_scenario().len());
    assert_eq!(second, 0);
}
