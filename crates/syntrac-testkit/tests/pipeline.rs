//! Proof of modularity: the whole ingest pipeline runs offline against mocks.

use std::net::Ipv4Addr;
use std::time::Duration;

use syntrac_core::{
    EventCategory, EventEnvelope, EventPayload, EventQuery, EventRepository, NetAction, Pipeline, ProcessInstanceId,
    ProcessRepository,
};
use syntrac_testkit::fixtures::{
    at_secs, beacon_scenario, dns_query, net_activity, process_exited, process_observed, process_started, EventFactory,
    BEACON_TARGET, DROPPER_PID, EXPLORER_PID, POWERSHELL_PID,
};
use syntrac_testkit::{InMemoryRepository, MockTelemetrySource};

const WINDOW: Duration = Duration::from_secs(10);

fn run(events: Vec<EventEnvelope>) -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let mut pipeline = Pipeline::new(WINDOW);
    pipeline.ingest(&mut MockTelemetrySource::new(events), &mut repo).expect("ingest succeeds");
    pipeline.flush_all(&mut repo).expect("flush succeeds");
    repo
}

fn all_events(repo: &InMemoryRepository) -> Vec<EventEnvelope> {
    repo.query(&EventQuery::default()).unwrap()
}

fn flows_of(repo: &InMemoryRepository, process: Option<ProcessInstanceId>) -> Vec<EventEnvelope> {
    all_events(repo)
        .into_iter()
        .filter(|e| matches!(e.payload, EventPayload::NetworkFlow { .. }) && e.process_instance_id == process)
        .collect()
}

#[test]
fn child_processes_link_to_their_parent_instance() {
    let repo = run(beacon_scenario());
    let dropper = repo.resolve(DROPPER_PID, at_secs(10)).unwrap().expect("dropper known");
    let powershell = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().expect("powershell known");
    let explorer = repo.resolve(EXPLORER_PID, at_secs(10)).unwrap().unwrap();

    assert_eq!(repo.get(powershell).unwrap().unwrap().parent_id, Some(dropper));
    assert_eq!(repo.get(dropper).unwrap().unwrap().parent_id, Some(explorer));
}

#[test]
fn beacon_traffic_becomes_one_flow_per_window_attributed_to_the_first_powershell() {
    let repo = run(beacon_scenario());
    let powershell = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();

    let flows = flows_of(&repo, Some(powershell));
    assert_eq!(flows.len(), 3, "beats at t=4, 34, 64 fall in three different 10s windows");
    for flow in flows {
        match flow.payload {
            EventPayload::NetworkFlow { remote_addr, bytes_sent, opened, .. } => {
                assert_eq!(remote_addr, std::net::IpAddr::V4(BEACON_TARGET));
                assert_eq!((bytes_sent, opened), (200, 1));
            }
            other => panic!("unexpected {}", other.kind()),
        }
    }
}

#[test]
fn a_reused_pid_does_not_inherit_the_old_processes_flows() {
    let repo = run(beacon_scenario());
    let first = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();
    let second = repo.resolve(POWERSHELL_PID, at_secs(205)).unwrap().unwrap();
    assert_ne!(first, second);

    let late = flows_of(&repo, Some(second));
    assert_eq!(late.len(), 1);
    assert_eq!(late[0].ts, at_secs(210));
}

#[test]
fn raw_network_activity_is_never_stored() {
    let repo = run(beacon_scenario());
    assert!(all_events(&repo).iter().all(|e| e.payload.kind() != "network_activity"));
}

#[test]
fn exit_closes_the_process_lifetime() {
    let repo = run(beacon_scenario());
    let first = repo.resolve(POWERSHELL_PID, at_secs(10)).unwrap().unwrap();
    assert_eq!(repo.get(first).unwrap().unwrap().ended, Some(at_secs(100)));
}

#[test]
fn a_connection_from_an_unknown_process_is_kept_but_unattributed() {
    let mut f = EventFactory::default();
    let repo = run(vec![f.make(at_secs(5), net_activity(777, NetAction::Connect, BEACON_TARGET, 0))]);

    let flows = flows_of(&repo, None);
    assert_eq!(flows.len(), 1);
}

#[test]
fn the_exit_of_a_process_we_never_saw_does_not_lose_the_rest_of_the_batch() {
    let mut f = EventFactory::default();
    let repo = run(vec![
        f.make(at_secs(5), dns_query(1, "before.example", &[])),
        f.make(at_secs(6), process_exited(999, 1)), // never started under our watch
        f.make(at_secs(7), dns_query(1, "after.example", &[])),
    ]);

    let events = all_events(&repo);
    assert_eq!(events.len(), 3);
    let exit = events.iter().find(|e| e.payload.kind() == "process_exited").expect("exit stored");
    assert_eq!(exit.process_instance_id, None);
}

#[test]
fn identity_comes_from_the_real_creation_time_not_the_event_time() {
    let mut f = EventFactory::default();
    // Heard about at t=5, but the OS says it was created at t=2.
    let repo = run(vec![
        f.make(at_secs(5), process_started(60, 4, 2, r"C:\late.exe")),
        f.make(at_secs(9), process_exited(60, 2)),
    ]);

    let id = ProcessInstanceId::from_pid_and_creation(60, at_secs(2));
    let record = repo.get(id).unwrap().expect("record keyed by creation time");
    assert_eq!(record.started, at_secs(2));
    assert_eq!(record.ended, Some(at_secs(9)));
    assert_eq!(repo.resolve(60, at_secs(3)).unwrap(), Some(id), "resolvable from its creation time on");
}

#[test]
fn a_parent_is_resolved_as_of_the_childs_creation_time() {
    let mut f = EventFactory::default();
    let repo = run(vec![
        f.make(at_secs(1), process_started(50, 4, 1, r"C:\parent-one.exe")),
        f.make(at_secs(10), process_exited(50, 1)),
        f.make(at_secs(20), process_started(50, 4, 20, r"C:\parent-two.exe")), // PID 50 reused
        // The child was really created at t=5 (so its parent was the FIRST pid 50) but we hear about it late.
        f.make(at_secs(25), process_started(70, 50, 5, r"C:\child.exe")),
    ]);

    let child = repo.get(ProcessInstanceId::from_pid_and_creation(70, at_secs(5))).unwrap().unwrap();
    assert_eq!(child.parent_id, Some(ProcessInstanceId::from_pid_and_creation(50, at_secs(1))));
}

#[test]
fn a_snapshotted_process_and_its_live_start_make_one_record_and_keep_distinct_event_kinds() {
    let mut f = EventFactory::default();
    let repo = run(vec![
        f.make(at_secs(5), process_started(80, 4, 5, r"C:\both.exe")),
        f.make(at_secs(6), process_observed(80, 4, 5, r"C:\both.exe")),
    ]);

    let id = ProcessInstanceId::from_pid_and_creation(80, at_secs(5));
    assert!(repo.get(id).unwrap().is_some());
    let kinds: Vec<_> = all_events(&repo).iter().map(|e| e.payload.kind()).collect();
    assert_eq!(kinds, vec!["process_started", "process_observed"]);
}

#[test]
fn a_late_snapshot_cannot_resurrect_a_process_that_already_exited() {
    let mut f = EventFactory::default();
    let repo = run(vec![
        f.make(at_secs(5), process_started(90, 4, 5, r"C:\quick.exe")),
        f.make(at_secs(9), process_exited(90, 5)),
        f.make(at_secs(10), process_observed(90, 4, 5, r"C:\quick.exe")),
    ]);

    let id = ProcessInstanceId::from_pid_and_creation(90, at_secs(5));
    assert_eq!(repo.get(id).unwrap().unwrap().ended, Some(at_secs(9)));
}

#[test]
fn a_process_that_exits_after_the_snapshot_began_is_closed_even_if_its_exit_is_delivered_first() {
    // The snapshot is stamped with the time it BEGAN (t=5); the process then exits at t=10.
    let mut f = EventFactory::default();
    let exit = f.make(at_secs(10), process_exited(95, 2));
    let observed = f.make(at_secs(5), process_observed(95, 4, 2, r"C:\already-running.exe"));
    let repo = run(vec![exit, observed]);

    let id = ProcessInstanceId::from_pid_and_creation(95, at_secs(2));
    assert_eq!(repo.get(id).unwrap().unwrap().ended, Some(at_secs(10)), "must not stay 'running' forever");
}

#[test]
fn after_a_restart_processes_that_exited_while_the_collector_was_down_are_closed_not_left_running_forever() {
    // Previous session recorded two running processes; one exited while we were down.
    let mut repo = InMemoryRepository::default();
    let survivor = syntrac_testkit::fixtures::running_process(10, at_secs(5));
    let vanished = syntrac_testkit::fixtures::running_process(11, at_secs(6));
    repo.record_start(survivor.clone()).unwrap();
    repo.record_start(vanished.clone()).unwrap();

    // New session: the snapshot (stamped t=100) sees only the survivor; another process starts later.
    let mut f = EventFactory::default();
    let mut pipeline = Pipeline::new(WINDOW);
    let batch = vec![
        f.make(at_secs(100), process_observed(10, 4, 5, r"C:\test\proc10.exe")),
        f.make(at_secs(120), process_started(12, 4, 120, r"C:\new.exe")),
    ];
    pipeline.ingest(&mut MockTelemetrySource::new(batch), &mut repo).unwrap();

    assert_eq!(repo.get(survivor.id).unwrap().unwrap().ended, None);
    let closed = repo.get(vanished.id).unwrap().unwrap();
    assert_eq!((closed.ended, closed.ended_approximate), (Some(at_secs(100)), true));
    let new = ProcessInstanceId::from_pid_and_creation(12, at_secs(120));
    assert_eq!(repo.get(new).unwrap().unwrap().ended, None, "started after the snapshot: untouched");
}

#[test]
fn events_delivered_out_of_order_are_processed_by_timestamp() {
    let mut f = EventFactory::default();
    let connect = f.make(at_secs(5), net_activity(50, NetAction::Connect, BEACON_TARGET, 0));
    let start = f.make(at_secs(1), process_started(50, 4, 1, r"C:\late-delivery.exe"));
    let repo = run(vec![connect, start]);

    let id = ProcessInstanceId::from_pid_and_creation(50, at_secs(1));
    assert_eq!(flows_of(&repo, Some(id)).len(), 1);
}

#[test]
fn a_second_drain_with_no_new_events_stores_nothing() {
    let mut source = MockTelemetrySource::new(beacon_scenario());
    let mut repo = InMemoryRepository::default();
    let mut pipeline = Pipeline::new(WINDOW);
    let first = pipeline.ingest(&mut source, &mut repo).unwrap();
    let second = pipeline.ingest(&mut source, &mut repo).unwrap();
    assert!(first > 0);
    assert_eq!(second, 0);
}

#[test]
fn flows_are_stored_only_after_their_window_ends_and_everything_is_flushed_at_shutdown() {
    let mut f = EventFactory::default();
    let mut source = MockTelemetrySource::new(vec![
        f.make(at_secs(1), process_started(40, 4, 1, r"C:\a.exe")),
        f.make(at_secs(4), net_activity(40, NetAction::Send, BEACON_TARGET, 10)),
        f.make(at_secs(14), net_activity(40, NetAction::Send, BEACON_TARGET, 10)),
    ]);
    let mut repo = InMemoryRepository::default();
    let mut pipeline = Pipeline::new(WINDOW);
    pipeline.ingest(&mut source, &mut repo).unwrap();

    let flow_count = |repo: &InMemoryRepository| {
        repo.query(&EventQuery { category: Some(EventCategory::Network), ..EventQuery::default() }).unwrap().len()
    };
    assert_eq!(flow_count(&repo), 0, "nothing stored while windows are open");

    pipeline.flush_due(at_secs(10), &mut repo).unwrap();
    assert_eq!(flow_count(&repo), 1, "first window ended at t=10");

    pipeline.flush_all(&mut repo).unwrap();
    assert_eq!(flow_count(&repo), 2, "shutdown flushes the still-open window");
}

#[test]
fn dns_events_pass_through_attributed_to_the_calling_process() {
    let mut f = EventFactory::default();
    let repo = run(vec![
        f.make(at_secs(1), process_started(40, 4, 1, r"C:\browser.exe")),
        f.make(at_secs(3), dns_query(40, "example.org", &[Ipv4Addr::new(93, 184, 216, 34)])),
    ]);

    let id = ProcessInstanceId::from_pid_and_creation(40, at_secs(1));
    let dns: Vec<_> = repo
        .query(&EventQuery { category: Some(EventCategory::Dns), process: Some(id), ..EventQuery::default() })
        .unwrap();
    assert_eq!(dns.len(), 1);
}
