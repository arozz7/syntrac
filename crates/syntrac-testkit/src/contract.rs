//! Repository contract suites. Every implementation (in-memory, SQLite, ...) must pass these.
//!
//! Each suite takes a factory so every check starts from an empty repository.

use syntrac_core::{EventCategory, EventQuery, EventRepository, ProcessInstanceId, ProcessRepository, RepoError};

use crate::fixtures::{
    at_secs, process_exited, process_started, running_process, tcp_connect, EventFactory, BEACON_TARGET,
};

pub fn process_repository_contract<R: ProcessRepository>(make: impl Fn() -> R) {
    start_then_get_returns_the_record(make());
    resolve_respects_the_process_lifetime(make());
    resolve_distinguishes_reused_pids(make());
    a_running_process_resolves_at_any_later_time(make());
    exit_of_an_unknown_process_is_not_found(make());
    exit_records_end_time_and_exit_code(make());
    starting_the_same_instance_twice_keeps_one_record(make());
}

fn start_then_get_returns_the_record<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    repo.record_start(record.clone()).unwrap();
    assert_eq!(repo.get(record.id).unwrap(), Some(record));
}

fn resolve_respects_the_process_lifetime<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    let id = record.id;
    repo.record_start(record).unwrap();
    repo.record_exit(id, at_secs(15), Some(0)).unwrap();

    assert_eq!(repo.resolve(10, at_secs(4)).unwrap(), None, "before start");
    assert_eq!(repo.resolve(10, at_secs(5)).unwrap(), Some(id), "at start");
    assert_eq!(repo.resolve(10, at_secs(10)).unwrap(), Some(id), "while running");
    assert_eq!(repo.resolve(10, at_secs(15)).unwrap(), Some(id), "at exit");
    assert_eq!(repo.resolve(10, at_secs(16)).unwrap(), None, "after exit");
    assert_eq!(repo.resolve(11, at_secs(10)).unwrap(), None, "other pid");
}

fn resolve_distinguishes_reused_pids<R: ProcessRepository>(mut repo: R) {
    let first = running_process(10, at_secs(5));
    let second = running_process(10, at_secs(50));
    let (first_id, second_id) = (first.id, second.id);
    assert_ne!(first_id, second_id);

    repo.record_start(first).unwrap();
    repo.record_exit(first_id, at_secs(20), None).unwrap();
    repo.record_start(second).unwrap();

    assert_eq!(repo.resolve(10, at_secs(10)).unwrap(), Some(first_id));
    assert_eq!(repo.resolve(10, at_secs(30)).unwrap(), None, "gap between instances");
    assert_eq!(repo.resolve(10, at_secs(60)).unwrap(), Some(second_id));
}

fn a_running_process_resolves_at_any_later_time<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    let id = record.id;
    repo.record_start(record).unwrap();
    assert_eq!(repo.resolve(10, at_secs(100_000)).unwrap(), Some(id));
}

fn exit_of_an_unknown_process_is_not_found<R: ProcessRepository>(mut repo: R) {
    let unknown = ProcessInstanceId::from_pid_and_creation(99, at_secs(1));
    let result = repo.record_exit(unknown, at_secs(2), None);
    assert!(matches!(result, Err(RepoError::NotFound)), "got {result:?}");
}

fn exit_records_end_time_and_exit_code<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    let id = record.id;
    repo.record_start(record).unwrap();
    repo.record_exit(id, at_secs(9), Some(3)).unwrap();

    let stored = repo.get(id).unwrap().expect("record exists");
    assert_eq!(stored.ended, Some(at_secs(9)));
    assert_eq!(stored.exit_code, Some(3));
}

fn starting_the_same_instance_twice_keeps_one_record<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    repo.record_start(record.clone()).unwrap();
    repo.record_start(record.clone()).unwrap();
    assert_eq!(repo.get(record.id).unwrap(), Some(record));
    assert_eq!(repo.resolve(10, at_secs(6)).unwrap(), Some(ProcessInstanceId::from_pid_and_creation(10, at_secs(5))));
}

pub fn event_repository_contract<R: EventRepository>(make: impl Fn() -> R) {
    query_returns_events_ordered_by_time(make());
    query_filters_by_time_range_inclusively(make());
    query_filters_by_category(make());
    query_filters_by_process(make());
    query_applies_the_limit_after_ordering(make());
    appended_events_are_returned_unchanged(make());
}

fn sample_events() -> Vec<syntrac_core::EventEnvelope> {
    let mut f = EventFactory::default();
    let owner = ProcessInstanceId::from_pid_and_creation(10, at_secs(1));
    let mut connect = f.make(at_secs(30), tcp_connect(10, BEACON_TARGET, 443));
    connect.process_instance_id = Some(owner);
    vec![connect, f.make(at_secs(10), process_started(10, 4, r"C:\a.exe")), f.make(at_secs(20), process_exited(10))]
}

fn query_returns_events_ordered_by_time<R: EventRepository>(mut repo: R) {
    repo.append(&sample_events()).unwrap();
    let times: Vec<_> = repo.query(&EventQuery::default()).unwrap().iter().map(|e| e.ts).collect();
    assert_eq!(times, vec![at_secs(10), at_secs(20), at_secs(30)]);
}

fn query_filters_by_time_range_inclusively<R: EventRepository>(mut repo: R) {
    repo.append(&sample_events()).unwrap();
    let query = EventQuery { from: Some(at_secs(20)), to: Some(at_secs(30)), ..EventQuery::default() };
    let times: Vec<_> = repo.query(&query).unwrap().iter().map(|e| e.ts).collect();
    assert_eq!(times, vec![at_secs(20), at_secs(30)]);
}

fn query_filters_by_category<R: EventRepository>(mut repo: R) {
    repo.append(&sample_events()).unwrap();
    let query = EventQuery { category: Some(EventCategory::Network), ..EventQuery::default() };
    let found = repo.query(&query).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].payload.category(), EventCategory::Network);
}

fn query_filters_by_process<R: EventRepository>(mut repo: R) {
    repo.append(&sample_events()).unwrap();
    let owner = ProcessInstanceId::from_pid_and_creation(10, at_secs(1));
    let query = EventQuery { process: Some(owner), ..EventQuery::default() };
    let found = repo.query(&query).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].process_instance_id, Some(owner));
}

fn query_applies_the_limit_after_ordering<R: EventRepository>(mut repo: R) {
    repo.append(&sample_events()).unwrap();
    let query = EventQuery { limit: Some(2), ..EventQuery::default() };
    let times: Vec<_> = repo.query(&query).unwrap().iter().map(|e| e.ts).collect();
    assert_eq!(times, vec![at_secs(10), at_secs(20)]);
}

fn appended_events_are_returned_unchanged<R: EventRepository>(mut repo: R) {
    let events = sample_events();
    repo.append(&events).unwrap();
    let mut expected = events;
    expected.sort_by_key(|e| e.ts);
    assert_eq!(repo.query(&EventQuery::default()).unwrap(), expected);
}
