//! Repository contract suites. Every implementation (in-memory, SQLite, ...) must pass these.
//!
//! Each suite takes a factory so every check starts from an empty repository.

use syntrac_core::{
    Destination, EventCategory, EventQuery, EventRepository, FlowQueries, MetricsRepository, ProcessFilter,
    ProcessInstanceId, ProcessQueries, ProcessRecord, ProcessRepository, RepoError, Retention, Timestamp,
};

use crate::fixtures::{
    at_secs, dns_query, metrics_sample, net_flow, process_exited, process_metric, process_started, running_process,
    EventFactory, BEACON_TARGET,
};

pub fn process_repository_contract<R: ProcessRepository>(make: impl Fn() -> R) {
    start_then_get_returns_the_record(make());
    resolve_respects_the_process_lifetime(make());
    resolve_distinguishes_reused_pids(make());
    a_running_process_resolves_at_any_later_time(make());
    exit_of_an_unknown_process_is_not_found(make());
    exit_records_end_time_and_exit_code(make());
    starting_the_same_instance_twice_keeps_one_record(make());
    a_late_start_never_clears_a_recorded_exit(make());
    reconcile_closes_processes_that_are_no_longer_running(make());
    reconcile_leaves_live_new_and_ended_processes_alone(make());
    reconcile_is_idempotent(make());
    an_exact_exit_replaces_an_approximate_one_flag(make());
    a_parent_link_round_trips(make());
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

/// A startup snapshot can arrive after the process already exited; it must not bring it back to life.
fn a_late_start_never_clears_a_recorded_exit<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    let id = record.id;
    repo.record_start(record.clone()).unwrap();
    repo.record_exit(id, at_secs(9), Some(3)).unwrap();

    repo.record_start(record).unwrap(); // late snapshot of the same instance

    let stored = repo.get(id).unwrap().expect("record exists");
    assert_eq!(stored.ended, Some(at_secs(9)));
    assert_eq!(stored.exit_code, Some(3));
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
    let mut flow = f.make(at_secs(30), net_flow(10, BEACON_TARGET, 100, 2000, 30));
    flow.process_instance_id = Some(owner);
    vec![
        flow,
        f.make(at_secs(10), process_started(10, 4, 10, r"C:\a.exe")),
        f.make(at_secs(20), process_exited(10, 10)),
    ]
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
    let mut f = EventFactory::after(100); // sample_events() uses ids 1..=3
    let mut events = sample_events();
    events.push(f.make(at_secs(40), dns_query(10, "example.org", &[BEACON_TARGET])));
    repo.append(&events).unwrap();
    let mut expected = events;
    expected.sort_by_key(|e| e.ts);
    assert_eq!(repo.query(&EventQuery::default()).unwrap(), expected);
}

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 1e-3, "expected {expected}, got {actual}");
}

pub fn metrics_repository_contract<R: MetricsRepository>(make: impl Fn() -> R) {
    system_series_is_ordered_and_bounded(make());
    breakdown_averages_over_every_sample_in_the_window(make());
    breakdown_respects_the_window_bounds(make());
    breakdown_ranks_heaviest_first_and_applies_the_limit(make());
    breakdown_reports_the_long_tail_separately(make());
    breakdown_of_an_empty_window_is_empty(make());
    breakdown_reports_cpu_that_no_process_explains(make());
    the_unaccounted_share_never_goes_negative(make());
    a_sample_without_an_owner_is_kept(make());
}

fn fill_metrics<R: MetricsRepository>(repo: &mut R) -> (ProcessInstanceId, ProcessInstanceId) {
    let a = ProcessInstanceId::from_pid_and_creation(1, at_secs(0));
    let b = ProcessInstanceId::from_pid_and_creation(2, at_secs(0));
    // t=10,20: A=20% B=10%; t=30,40: A=20% only. Long tail: 4% in 50 other processes. System: 40%.
    for (at, with_b) in [(10, true), (20, true), (30, false), (40, false)] {
        let mut processes = vec![process_metric(1, Some(a), 20.0)];
        if with_b {
            processes.push(process_metric(2, Some(b), 10.0));
        }
        repo.append_metrics(&metrics_sample(at, 40.0, processes, 4.0, 50)).unwrap();
    }
    (a, b)
}

fn system_series_is_ordered_and_bounded<R: MetricsRepository>(mut repo: R) {
    for at in [30, 10, 20] {
        repo.append_metrics(&metrics_sample(at, at as f32, vec![], 0.0, 0)).unwrap();
    }
    let series = repo.system_series(at_secs(10), at_secs(20)).unwrap();
    let times: Vec<_> = series.iter().map(|p| p.ts).collect();
    assert_eq!(times, vec![at_secs(10), at_secs(20)]);
    assert_close(series[1].system.cpu_percent, 20.0);
}

fn breakdown_averages_over_every_sample_in_the_window<R: MetricsRepository>(mut repo: R) {
    let (a, b) = fill_metrics(&mut repo);
    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();

    assert_eq!(breakdown.window_samples, 4);
    assert_close(breakdown.system_avg_cpu_percent, 40.0);

    let process_a = breakdown.processes.iter().find(|p| p.instance == Some(a)).expect("A present");
    assert_close(process_a.avg_cpu_percent, 20.0);
    assert_close(process_a.peak_cpu_percent, 20.0);
    assert_eq!(process_a.samples_present, 4);

    // B used 10% in 2 of 4 samples: absent samples count as zero, so the average is 5%.
    let process_b = breakdown.processes.iter().find(|p| p.instance == Some(b)).expect("B present");
    assert_close(process_b.avg_cpu_percent, 5.0);
    assert_close(process_b.peak_cpu_percent, 10.0);
    assert_eq!(process_b.samples_present, 2);
}

fn breakdown_respects_the_window_bounds<R: MetricsRepository>(mut repo: R) {
    let (_, b) = fill_metrics(&mut repo);
    let breakdown = repo.cpu_breakdown(at_secs(20), at_secs(40), 10).unwrap();

    assert_eq!(breakdown.window_samples, 3);
    let process_b = breakdown.processes.iter().find(|p| p.instance == Some(b)).expect("B present at t=20");
    assert_close(process_b.avg_cpu_percent, 10.0 / 3.0);
}

fn breakdown_ranks_heaviest_first_and_applies_the_limit<R: MetricsRepository>(mut repo: R) {
    let (a, _) = fill_metrics(&mut repo);
    let all = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert!(all.processes[0].avg_cpu_percent >= all.processes[1].avg_cpu_percent);

    let top_one = repo.cpu_breakdown(at_secs(0), at_secs(100), 1).unwrap();
    assert_eq!(top_one.processes.len(), 1);
    assert_eq!(top_one.processes[0].instance, Some(a));
}

fn breakdown_reports_the_long_tail_separately<R: MetricsRepository>(mut repo: R) {
    fill_metrics(&mut repo);
    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_close(breakdown.others_avg_cpu_percent, 4.0);
}

fn breakdown_of_an_empty_window_is_empty<R: MetricsRepository>(repo: R) {
    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(breakdown.window_samples, 0);
    assert!(breakdown.processes.is_empty());
    assert_close(breakdown.system_avg_cpu_percent, 0.0);
    assert_close(breakdown.unaccounted_avg_cpu_percent, 0.0);
}

/// System 40% minus listed (A 20 + B 5 on average) minus the long tail (4) leaves 11% that no process explains.
fn breakdown_reports_cpu_that_no_process_explains<R: MetricsRepository>(mut repo: R) {
    fill_metrics(&mut repo);
    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_close(breakdown.unaccounted_avg_cpu_percent, 11.0);

    // Limiting the list must not turn the hidden processes into "unaccounted".
    let limited = repo.cpu_breakdown(at_secs(0), at_secs(100), 1).unwrap();
    assert_close(limited.unaccounted_avg_cpu_percent, 11.0);
}

fn the_unaccounted_share_never_goes_negative<R: MetricsRepository>(mut repo: R) {
    // Processes sum to more than the system figure (sampling jitter between the two readings).
    repo.append_metrics(&metrics_sample(10, 5.0, vec![process_metric(1, None, 20.0)], 0.0, 0)).unwrap();
    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_close(breakdown.unaccounted_avg_cpu_percent, 0.0);
}

pub fn flow_queries_contract<R: EventRepository + FlowQueries>(make: impl Fn() -> R) {
    destinations_are_ranked_by_total_bytes_and_summed(make());
    destinations_respect_the_range_and_the_limit(make());
    destinations_of_an_empty_store_are_empty(make());
}

fn flow_event(
    f: &mut EventFactory,
    at: i64,
    owner: Option<ProcessInstanceId>,
    pid: u32,
    remote: std::net::Ipv4Addr,
    sent: u64,
    received: u64,
) -> syntrac_core::EventEnvelope {
    let mut event = f.make(at_secs(at), net_flow(pid, remote, sent, received, at));
    event.process_instance_id = owner;
    event
}

fn fill_flows<R: EventRepository>(repo: &mut R) {
    let mut f = EventFactory::default();
    let (a, b) = (
        ProcessInstanceId::from_pid_and_creation(1, at_secs(0)),
        ProcessInstanceId::from_pid_and_creation(2, at_secs(0)),
    );
    let (x, y) = (BEACON_TARGET, std::net::Ipv4Addr::new(198, 51, 100, 7));
    repo.append(&[
        flow_event(&mut f, 10, Some(a), 1, x, 100, 1000),
        flow_event(&mut f, 20, Some(a), 1, x, 50, 500),
        flow_event(&mut f, 20, Some(b), 2, x, 10, 10),
        flow_event(&mut f, 20, Some(a), 1, y, 5, 5),
        flow_event(&mut f, 20, None, 99, y, 1, 1), // unattributed: counts as its own process
    ])
    .unwrap();
}

fn destination_for(destinations: &[Destination], addr: std::net::Ipv4Addr) -> Destination {
    *destinations.iter().find(|d| d.remote_addr == std::net::IpAddr::V4(addr)).expect("destination present")
}

fn destinations_are_ranked_by_total_bytes_and_summed<R: EventRepository + FlowQueries>(mut repo: R) {
    fill_flows(&mut repo);
    let destinations = repo.top_destinations(at_secs(0), at_secs(100), 10).unwrap();

    assert_eq!(destinations.len(), 2);
    assert_eq!(destinations[0].remote_addr, std::net::IpAddr::V4(BEACON_TARGET), "busiest first");

    let x = destination_for(&destinations, BEACON_TARGET);
    assert_eq!((x.bytes_sent, x.bytes_received, x.flow_count, x.process_count), (160, 1510, 3, 2));
    let y = destination_for(&destinations, std::net::Ipv4Addr::new(198, 51, 100, 7));
    assert_eq!((y.bytes_sent, y.bytes_received, y.flow_count, y.process_count), (6, 6, 2, 2));
}

fn destinations_respect_the_range_and_the_limit<R: EventRepository + FlowQueries>(mut repo: R) {
    fill_flows(&mut repo);

    let later = repo.top_destinations(at_secs(15), at_secs(100), 10).unwrap();
    let x = destination_for(&later, BEACON_TARGET);
    assert_eq!((x.bytes_sent, x.flow_count), (60, 2), "the flow at t=10 is outside the range");

    let top_one = repo.top_destinations(at_secs(0), at_secs(100), 1).unwrap();
    assert_eq!(top_one.len(), 1);
}

fn destinations_of_an_empty_store_are_empty<R: EventRepository + FlowQueries>(repo: R) {
    assert!(repo.top_destinations(at_secs(0), at_secs(100), 10).unwrap().is_empty());
}

pub fn retention_contract<R>(make: impl Fn() -> R)
where
    R: EventRepository + ProcessRepository + MetricsRepository + FlowQueries + Retention,
{
    purge_removes_only_data_older_than_the_cutoff(make());
    purging_twice_is_harmless(make());
    purging_removes_a_flow_event_together_with_its_flow(make());
}

fn purge_removes_only_data_older_than_the_cutoff<R>(mut repo: R)
where
    R: EventRepository + ProcessRepository + MetricsRepository + FlowQueries + Retention,
{
    let mut f = EventFactory::default();
    repo.append(&[
        f.make(at_secs(10), dns_query(1, "old.example", &[])),
        f.make(at_secs(20), dns_query(1, "old2.example", &[])),
        f.make(at_secs(30), dns_query(1, "new.example", &[])),
    ])
    .unwrap();

    let ended_early = running_process(1, at_secs(1));
    let ended_late = running_process(2, at_secs(2));
    let still_running = running_process(3, at_secs(3));
    for record in [&ended_early, &ended_late, &still_running] {
        repo.record_start((*record).clone()).unwrap();
    }
    repo.record_exit(ended_early.id, at_secs(15), None).unwrap();
    repo.record_exit(ended_late.id, at_secs(28), None).unwrap();

    for at in [10, 20, 30] {
        repo.append_metrics(&metrics_sample(at, 10.0, vec![process_metric(1, Some(ended_early.id), 5.0)], 0.0, 0))
            .unwrap();
    }

    let counts = repo.purge_before(at_secs(25)).unwrap();
    assert_eq!((counts.events, counts.processes, counts.metric_samples), (2, 1, 2));

    assert_eq!(repo.query(&EventQuery::default()).unwrap().len(), 1);
    assert_eq!(repo.get(ended_early.id).unwrap(), None, "ended before the cutoff: removed");
    assert!(repo.get(ended_late.id).unwrap().is_some(), "ended after the cutoff: kept");
    assert!(repo.get(still_running.id).unwrap().is_some(), "still running: always kept");
    assert_eq!(repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap().window_samples, 1);
}

fn purging_twice_is_harmless<R>(mut repo: R)
where
    R: EventRepository + ProcessRepository + MetricsRepository + FlowQueries + Retention,
{
    let cutoff = Timestamp::from_micros(0);
    let first = repo.purge_before(cutoff).unwrap();
    let second = repo.purge_before(cutoff).unwrap();
    assert_eq!(first, second);
}

/// After a restart the snapshot lists what is alive NOW. Anything recorded as running that is not in it
/// exited while the collector was down; its true exit time is unknown, so it is closed at the snapshot time
/// and flagged approximate. Without this, "running processes" would grow forever across reboots.
fn reconcile_closes_processes_that_are_no_longer_running<R: ProcessRepository>(mut repo: R) {
    let gone = running_process(10, at_secs(5));
    repo.record_start(gone.clone()).unwrap();

    let closed = repo.reconcile_running(&[], at_secs(100)).unwrap();

    assert_eq!(closed, 1);
    let stored = repo.get(gone.id).unwrap().unwrap();
    assert_eq!(stored.ended, Some(at_secs(100)));
    assert!(stored.ended_approximate, "the exit time is a bound, not a measurement");
    assert_eq!(repo.resolve(10, at_secs(200)).unwrap(), None, "a closed process no longer owns its PID");
}

fn reconcile_leaves_live_new_and_ended_processes_alone<R: ProcessRepository>(mut repo: R) {
    let alive = running_process(10, at_secs(5));
    let started_after = running_process(11, at_secs(150)); // began after the snapshot
    let already_ended = running_process(12, at_secs(6));
    for record in [&alive, &started_after, &already_ended] {
        repo.record_start((*record).clone()).unwrap();
    }
    repo.record_exit(already_ended.id, at_secs(50), Some(0)).unwrap();

    let closed = repo.reconcile_running(&[alive.id], at_secs(100)).unwrap();

    assert_eq!(closed, 0);
    assert_eq!(repo.get(alive.id).unwrap().unwrap().ended, None);
    assert_eq!(repo.get(started_after.id).unwrap().unwrap().ended, None);
    let ended = repo.get(already_ended.id).unwrap().unwrap();
    assert_eq!((ended.ended, ended.ended_approximate), (Some(at_secs(50)), false), "a real exit is never overwritten");
}

fn reconcile_is_idempotent<R: ProcessRepository>(mut repo: R) {
    repo.record_start(running_process(10, at_secs(5))).unwrap();
    assert_eq!(repo.reconcile_running(&[], at_secs(100)).unwrap(), 1);
    assert_eq!(repo.reconcile_running(&[], at_secs(200)).unwrap(), 0);
}

fn an_exact_exit_replaces_an_approximate_one_flag<R: ProcessRepository>(mut repo: R) {
    let record = running_process(10, at_secs(5));
    repo.record_start(record.clone()).unwrap();
    repo.reconcile_running(&[], at_secs(100)).unwrap();
    repo.record_exit(record.id, at_secs(90), Some(1)).unwrap(); // a real exit report arrives after all

    let stored = repo.get(record.id).unwrap().unwrap();
    assert_eq!((stored.ended, stored.ended_approximate), (Some(at_secs(90)), false));
}

pub fn process_queries_contract<R: ProcessRepository + ProcessQueries>(make: impl Fn() -> R) {
    listing_returns_newest_first_and_applies_the_limit(make());
    running_only_excludes_ended_processes(make());
    name_search_is_case_insensitive_and_matches_any_part_of_the_path(make());
    an_empty_store_lists_nothing(make());
}

fn fill_processes<R: ProcessRepository>(repo: &mut R) -> [ProcessRecord; 3] {
    let mut chrome = running_process(1, at_secs(10));
    chrome.image_path = r"C:\Program Files\Google\Chrome\chrome.exe".into();
    let mut code = running_process(2, at_secs(20));
    code.image_path = r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe".into();
    let mut gone = running_process(3, at_secs(30));
    gone.image_path = r"C:\tools\Gone.exe".into();
    for record in [&chrome, &code, &gone] {
        repo.record_start((*record).clone()).unwrap();
    }
    repo.record_exit(gone.id, at_secs(40), Some(0)).unwrap();
    [chrome, code, gone]
}

fn listing_returns_newest_first_and_applies_the_limit<R: ProcessRepository + ProcessQueries>(mut repo: R) {
    fill_processes(&mut repo);
    let all = repo.list_processes(&ProcessFilter { limit: 10, ..ProcessFilter::default() }).unwrap();
    let pids: Vec<u32> = all.iter().map(|p| p.pid).collect();
    assert_eq!(pids, vec![3, 2, 1]);

    let limited = repo.list_processes(&ProcessFilter { limit: 2, ..ProcessFilter::default() }).unwrap();
    assert_eq!(limited.len(), 2);
}

fn running_only_excludes_ended_processes<R: ProcessRepository + ProcessQueries>(mut repo: R) {
    fill_processes(&mut repo);
    let running =
        repo.list_processes(&ProcessFilter { running_only: true, limit: 10, ..ProcessFilter::default() }).unwrap();
    let pids: Vec<u32> = running.iter().map(|p| p.pid).collect();
    assert_eq!(pids, vec![2, 1]);
}

fn name_search_is_case_insensitive_and_matches_any_part_of_the_path<R: ProcessRepository + ProcessQueries>(
    mut repo: R,
) {
    fill_processes(&mut repo);
    let find = |text: &str| {
        repo.list_processes(&ProcessFilter { name_contains: Some(text.into()), limit: 10, ..ProcessFilter::default() })
            .unwrap()
            .iter()
            .map(|p| p.pid)
            .collect::<Vec<u32>>()
    };
    assert_eq!(find("CHROME"), vec![1]);
    assert_eq!(find("vs code"), vec![2]);
    assert_eq!(find("tools"), vec![3]);
    assert!(find("nothing-matches-this").is_empty());
}

fn an_empty_store_lists_nothing<R: ProcessRepository + ProcessQueries>(repo: R) {
    assert!(repo.list_processes(&ProcessFilter { limit: 10, ..ProcessFilter::default() }).unwrap().is_empty());
}

pub fn recent_flows_contract<R: EventRepository + ProcessRepository + FlowQueries>(make: impl Fn() -> R) {
    flows_are_listed_newest_first_within_the_range_with_the_process_name(make());
    the_limit_keeps_the_newest_flows(make());
}

fn flows_are_listed_newest_first_within_the_range_with_the_process_name<
    R: EventRepository + ProcessRepository + FlowQueries,
>(
    mut repo: R,
) {
    let mut f = EventFactory::default();
    let mut browser = running_process(1, at_secs(1));
    browser.image_path = r"C:\apps\browser.exe".into();
    repo.record_start(browser.clone()).unwrap();
    repo.append(&[
        flow_event(&mut f, 10, Some(browser.id), 1, BEACON_TARGET, 100, 1000),
        flow_event(&mut f, 20, Some(browser.id), 1, BEACON_TARGET, 5, 5),
        flow_event(&mut f, 30, None, 99, BEACON_TARGET, 1, 1),
    ])
    .unwrap();

    let rows = repo.recent_flows(at_secs(15), at_secs(100), 10).unwrap();

    let starts: Vec<_> = rows.iter().map(|r| r.window_start).collect();
    assert_eq!(starts, vec![at_secs(30), at_secs(20)], "newest first, and the flow at t=10 is out of range");
    assert_eq!(rows[0].image_path, None, "unattributed flows have no process name");
    assert_eq!(rows[0].pid, 99);
    assert_eq!(rows[1].image_path.as_deref(), Some(r"C:\apps\browser.exe"));
    assert_eq!((rows[1].bytes_sent, rows[1].bytes_received), (5, 5));
}

fn the_limit_keeps_the_newest_flows<R: EventRepository + ProcessRepository + FlowQueries>(mut repo: R) {
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, 10, None, 1, BEACON_TARGET, 1, 1),
        flow_event(&mut f, 20, None, 1, BEACON_TARGET, 1, 1),
        flow_event(&mut f, 30, None, 1, BEACON_TARGET, 1, 1),
    ])
    .unwrap();
    let rows = repo.recent_flows(at_secs(0), at_secs(100), 2).unwrap();
    let starts: Vec<_> = rows.iter().map(|r| r.window_start).collect();
    assert_eq!(starts, vec![at_secs(30), at_secs(20)]);
}

fn a_parent_link_round_trips<R: ProcessRepository>(mut repo: R) {
    let parent = running_process(10, at_secs(5));
    let mut child = running_process(11, at_secs(6));
    child.parent_pid = 10;
    child.parent_id = Some(parent.id);
    child.command_line = Some("child.exe --flag".into());
    repo.record_start(parent.clone()).unwrap();
    repo.record_start(child.clone()).unwrap();
    assert_eq!(repo.get(child.id).unwrap(), Some(child));
    assert_eq!(repo.get(parent.id).unwrap(), Some(parent));
}

/// A process that could not be matched to an instance still has its CPU counted and listed.
fn a_sample_without_an_owner_is_kept<R: MetricsRepository>(mut repo: R) {
    let owned = ProcessInstanceId::from_pid_and_creation(1, at_secs(0));
    let sample =
        metrics_sample(10, 30.0, vec![process_metric(1, Some(owned), 20.0), process_metric(9, None, 8.0)], 0.0, 0);
    repo.append_metrics(&sample).unwrap();

    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    let ownerless: Vec<_> = breakdown.processes.iter().filter(|p| p.instance.is_none()).collect();
    assert_eq!(ownerless.len(), 1);
    assert_eq!(ownerless[0].pid, 9);
    assert_close(ownerless[0].avg_cpu_percent, 8.0);
    assert!(breakdown.processes.iter().any(|p| p.instance == Some(owned)));
}

/// A flow is one stored thing: purging its event must not leave its flow behind, and keeping its event must keep it whole.
fn purging_removes_a_flow_event_together_with_its_flow<R>(mut repo: R)
where
    R: EventRepository + FlowQueries + Retention,
{
    let mut f = EventFactory::default();
    let owner = ProcessInstanceId::from_pid_and_creation(10, at_secs(1));
    let mut old = f.make(at_secs(10), net_flow(10, BEACON_TARGET, 100, 200, 10));
    old.process_instance_id = Some(owner);
    let mut recent = f.make(at_secs(30), net_flow(10, BEACON_TARGET, 5, 6, 30));
    recent.process_instance_id = Some(owner);
    repo.append(&[old, recent.clone()]).unwrap();

    repo.purge_before(at_secs(25)).unwrap();

    assert_eq!(repo.query(&EventQuery::default()).unwrap(), vec![recent]);
    let destinations = repo.top_destinations(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(destinations.len(), 1);
    assert_eq!((destinations[0].bytes_sent, destinations[0].bytes_received, destinations[0].flow_count), (5, 6, 1));
}
