//! The collector run loop, proven offline with mocks, a manual clock and an in-memory repository.

use std::time::Duration;

use syntrac_core::retention::{GuardLevel, GuardLimits};
use syntrac_core::storage_guard::FreeSpace;
use syntrac_core::{
    EventEnvelope, EventQuery, EventRepository, MetricsRepository, NetAction, RollupRepository, RollupSettings,
    TelemetrySource, Timestamp,
};
use syntrac_service::{Collector, CollectorSettings};
use syntrac_testkit::fixtures::{
    at_secs, dns_query, metrics_sample, net_activity, process_metric, process_observed, process_started, EventFactory,
    BEACON_TARGET,
};
use syntrac_testkit::{
    FailingTelemetrySource, InMemoryRepository, ManualClock, MockMetricsSource, MockTelemetrySource,
    RestartableTelemetrySource, ScriptedTelemetrySource,
};

fn settings() -> CollectorSettings {
    CollectorSettings {
        flow_window: Duration::from_secs(10),
        flow_flush_grace: Duration::from_secs(3),
        metrics_interval: Duration::from_secs(5),
        retention: Duration::from_secs(24 * 3600),
        purge_interval: Duration::from_secs(3600),
        rollup_interval: Duration::from_secs(600),
        guard_interval: Duration::from_secs(60),
        rollup: RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 1 },
    }
}

fn collector_at(
    start_secs: i64,
    sources: Vec<Box<dyn TelemetrySource>>,
    metrics: Option<MockMetricsSource>,
    repo: InMemoryRepository,
) -> (Collector<InMemoryRepository, ManualClock>, ManualClock) {
    let clock = ManualClock::new(at_secs(start_secs));
    let metrics = metrics.map(|m| Box::new(m) as Box<dyn syntrac_core::MetricsSource>);
    (Collector::new(settings(), sources, metrics, repo, clock.clone()), clock)
}

fn stored(collector: &Collector<InMemoryRepository, ManualClock>) -> Vec<EventEnvelope> {
    collector.repository().query(&EventQuery::default()).unwrap()
}

fn kinds(events: &[EventEnvelope]) -> Vec<&'static str> {
    events.iter().map(|e| e.payload.kind()).collect()
}

#[test]
fn a_tick_stores_events_from_every_source() {
    let mut f = EventFactory::default();
    let first = MockTelemetrySource::new(vec![f.make(at_secs(1), process_started(10, 4, 1, r"C:\a.exe"))]);
    let second = MockTelemetrySource::new(vec![f.make(at_secs(2), dns_query(10, "example.org", &[]))]);
    let (mut collector, _) =
        collector_at(5, vec![Box::new(first), Box::new(second)], None, InMemoryRepository::default());

    let report = collector.tick();

    assert_eq!(report.events_stored, 2);
    assert_eq!(kinds(&stored(&collector)), vec!["process_started", "dns_query"]);
}

#[test]
fn flows_are_stored_only_once_their_window_has_ended() {
    let mut f = EventFactory::default();
    let source = MockTelemetrySource::new(vec![
        f.make(at_secs(1), process_started(10, 4, 1, r"C:\a.exe")),
        f.make(at_secs(4), net_activity(10, NetAction::Send, BEACON_TARGET, 100)),
    ]);
    let (mut collector, clock) = collector_at(5, vec![Box::new(source)], None, InMemoryRepository::default());

    collector.tick();
    assert!(!kinds(&stored(&collector)).contains(&"network_flow"), "window 0-10s is still open at t=5");

    clock.set(at_secs(10));
    assert_eq!(collector.tick().flows_stored, 0, "the window just ended; late arrivals still get a grace period");

    clock.set(at_secs(13));
    let report = collector.tick();
    assert_eq!(report.flows_stored, 1);
    assert!(kinds(&stored(&collector)).contains(&"network_flow"));
}

fn flows_in(collector: &Collector<InMemoryRepository, ManualClock>) -> Vec<EventEnvelope> {
    stored(collector).into_iter().filter(|e| e.payload.kind() == "network_flow").collect()
}

#[test]
fn a_late_event_for_a_window_that_just_ended_joins_it_instead_of_creating_a_duplicate_row() {
    // ETW delivers in buffered batches, so an event stamped 9.8s can arrive after the clock passed 10s.
    let mut f = EventFactory::default();
    let on_time = vec![
        f.make(at_secs(1), process_started(10, 4, 1, r"C:\a.exe")),
        f.make(at_secs(9), net_activity(10, NetAction::Send, BEACON_TARGET, 100)),
    ];
    let late = vec![f.make(Timestamp::from_micros(9_800_000), net_activity(10, NetAction::Send, BEACON_TARGET, 50))];
    let source = ScriptedTelemetrySource::new(vec![on_time, late]);
    let (mut collector, clock) = collector_at(11, vec![Box::new(source)], None, InMemoryRepository::default());

    collector.tick(); // clock 11: window [0,10) has ended but is within the grace period
    clock.set(at_secs(12));
    collector.tick(); // the late event arrives and joins window [0,10)
    clock.set(at_secs(13));
    collector.tick(); // grace over: the window is stored once

    let flows = flows_in(&collector);
    assert_eq!(flows.len(), 1, "one row per window, not one per delivery");
    match &flows[0].payload {
        syntrac_core::EventPayload::NetworkFlow { bytes_sent, .. } => assert_eq!(*bytes_sent, 150),
        other => panic!("unexpected {}", other.kind()),
    }
}

#[test]
fn the_order_the_sources_are_listed_in_does_not_change_attribution() {
    // ETW reports traffic of a process that was already running; the snapshot is what tells us about that process.
    let build = || {
        let mut f = EventFactory::default();
        let etw_like =
            MockTelemetrySource::new(vec![f.make(at_secs(5), net_activity(300, NetAction::Send, BEACON_TARGET, 10))]);
        let snapshot_like =
            MockTelemetrySource::new(vec![f.make(at_secs(3), process_observed(300, 4, 1, r"C:\old.exe"))]);
        (etw_like, snapshot_like)
    };

    for etw_first in [true, false] {
        let (etw_like, snapshot_like) = build();
        let sources: Vec<Box<dyn TelemetrySource>> = if etw_first {
            vec![Box::new(etw_like), Box::new(snapshot_like)]
        } else {
            vec![Box::new(snapshot_like), Box::new(etw_like)]
        };
        let (mut collector, _) = collector_at(20, sources, None, InMemoryRepository::default());

        collector.tick();
        collector.shutdown();

        let flows = flows_in(&collector);
        assert_eq!(flows.len(), 1);
        assert!(flows[0].process_instance_id.is_some(), "etw_first={etw_first}: the flow must be attributed");
    }
}

#[test]
fn metrics_are_sampled_on_their_interval_and_not_in_between() {
    let samples = vec![
        metrics_sample(5, 20.0, vec![process_metric(1, None, 5.0)], 0.0, 0),
        metrics_sample(10, 30.0, vec![], 0.0, 0),
    ];
    let (mut collector, clock) =
        collector_at(0, vec![], Some(MockMetricsSource::new(samples)), InMemoryRepository::default());
    let series = |c: &Collector<InMemoryRepository, ManualClock>| {
        c.repository().system_series(at_secs(0), at_secs(100)).unwrap().len()
    };

    assert!(!collector.tick().metrics_sampled, "CPU is a delta, so the very first reading is skipped");
    clock.set(at_secs(3));
    assert!(!collector.tick().metrics_sampled, "3s < 5s interval");
    clock.set(at_secs(5));
    assert!(collector.tick().metrics_sampled);
    clock.set(at_secs(7));
    assert!(!collector.tick().metrics_sampled);
    clock.set(at_secs(10));
    assert!(collector.tick().metrics_sampled);
    assert_eq!(series(&collector), 2);
}

#[test]
fn a_failing_metrics_sample_is_reported_but_does_not_stop_event_ingest() {
    let mut f = EventFactory::default();
    let source = MockTelemetrySource::new(vec![f.make(at_secs(1), dns_query(1, "example.org", &[]))]);
    let (mut collector, clock) = collector_at(
        0,
        vec![Box::new(source)],
        Some(MockMetricsSource::new(vec![])), // exhausted: every sample fails
        InMemoryRepository::default(),
    );
    clock.set(at_secs(5)); // the first sample is due one interval after start

    let report = collector.tick();

    assert_eq!(report.metrics_errors, 1);
    assert!(!report.metrics_sampled);
    assert_eq!(report.events_stored, 1, "events still flow");
}

#[test]
fn one_failing_source_does_not_stop_the_others() {
    let mut f = EventFactory::default();
    let good = MockTelemetrySource::new(vec![f.make(at_secs(1), dns_query(1, "example.org", &[]))]);
    let (mut collector, _) =
        collector_at(5, vec![Box::new(FailingTelemetrySource), Box::new(good)], None, InMemoryRepository::default());

    let report = collector.tick();

    assert_eq!(report.source_errors, 1);
    assert_eq!(report.events_stored, 1);
}

#[test]
fn old_data_is_purged_when_due_and_then_not_again_until_the_interval_passes() {
    let day = 24 * 3600;
    let now = 10 * day;
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    repo.append(&[
        f.make(at_secs(100), dns_query(1, "ancient.example", &[])), // far older than the 1-day retention
        f.make(at_secs(now - 60), dns_query(1, "recent.example", &[])),
    ])
    .unwrap();
    let (mut collector, clock) = collector_at(now, vec![], None, repo);

    let report = collector.tick();
    assert_eq!(report.purged.expect("purge is due at startup").events, 1);
    assert_eq!(stored(&collector).len(), 1);

    clock.set(at_secs(now + 600));
    assert!(collector.tick().purged.is_none(), "next purge is an hour away");

    clock.set(at_secs(now + 3600));
    assert!(collector.tick().purged.is_some());
}

#[test]
fn shutdown_does_a_final_drain_and_flushes_every_open_flow_window() {
    let mut f = EventFactory::default();
    let source = MockTelemetrySource::new(vec![
        f.make(at_secs(1), process_started(10, 4, 1, r"C:\a.exe")),
        f.make(at_secs(4), net_activity(10, NetAction::Send, BEACON_TARGET, 100)),
    ]);
    let (mut collector, _) = collector_at(5, vec![Box::new(source)], None, InMemoryRepository::default());
    // No tick has run: the events are still waiting in the source, and no window has ended.

    let report = collector.shutdown();

    assert_eq!(report.flows_stored, 1, "the still-open window is flushed at shutdown");
    let events = stored(&collector);
    assert!(kinds(&events).contains(&"process_started"), "final drain picked up pending events");
    assert!(kinds(&events).contains(&"network_flow"));
}

/// Releases its events only once stopped, like a trace that hands over its buffered events when it ends.
struct FlushesOnStop {
    events: Vec<EventEnvelope>,
    stopped: bool,
    stop_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl TelemetrySource for FlushesOnStop {
    fn name(&self) -> &str {
        "flushes-on-stop"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, syntrac_core::SourceError> {
        Ok(if self.stopped { std::mem::take(&mut self.events) } else { Vec::new() })
    }

    fn stop(&mut self) {
        self.stopped = true;
        self.stop_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
fn shutdown_stops_every_source_before_the_final_drain_so_nothing_is_lost() {
    let mut f = EventFactory::default();
    let stop_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let source = FlushesOnStop {
        events: vec![f.make(at_secs(1), dns_query(1, "last-words.example", &[]))],
        stopped: false,
        stop_calls: std::sync::Arc::clone(&stop_calls),
    };
    let (mut collector, _) = collector_at(5, vec![Box::new(source)], None, InMemoryRepository::default());

    collector.tick();
    assert_eq!(stop_calls.load(std::sync::atomic::Ordering::SeqCst), 0, "a normal tick never stops sources");
    assert!(stored(&collector).is_empty(), "nothing released yet");

    collector.shutdown();
    assert_eq!(stop_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        kinds(&stored(&collector)),
        vec!["dns_query"],
        "events released by stop() were still drained and stored"
    );
}

#[test]
fn shutdown_survives_a_failing_source() {
    let (mut collector, _) =
        collector_at(5, vec![Box::new(FailingTelemetrySource)], None, InMemoryRepository::default());
    assert_eq!(collector.shutdown().source_errors, 1);
}

#[test]
fn totals_accumulate_across_ticks() {
    let mut f = EventFactory::default();
    let source = MockTelemetrySource::new(vec![f.make(at_secs(1), dns_query(1, "a.example", &[]))]);
    let (mut collector, _) = collector_at(5, vec![Box::new(source)], None, InMemoryRepository::default());

    collector.tick();
    collector.tick();

    assert_eq!(collector.totals().events_stored, 1);
    assert_eq!(collector.totals().ticks, 2);
    let _ = Timestamp::from_micros(0);
}

// ---- restarting a source that has gone silent

#[test]
fn a_named_source_can_be_restarted_and_only_that_one() {
    let (etw, etw_restarts) = RestartableTelemetrySource::new("etw");
    let (other, other_restarts) = RestartableTelemetrySource::new("other");
    let (mut collector, _clock) =
        collector_at(1_000, vec![Box::new(etw), Box::new(other)], None, InMemoryRepository::default());

    collector.restart_source("etw").expect("etw can be restarted");

    assert_eq!((etw_restarts.count(), other_restarts.count()), (1, 0));
}

#[test]
fn restarting_a_source_that_does_not_exist_is_an_error_not_a_panic() {
    let (mut collector, _clock) =
        collector_at(1_000, vec![Box::new(MockTelemetrySource::new(vec![]))], None, InMemoryRepository::default());
    let error = collector.restart_source("etw").expect_err("no such source");
    assert!(error.to_string().contains("etw"), "{error}");
}

#[test]
fn a_source_that_cannot_be_restarted_says_so_and_collection_carries_on() {
    // MockTelemetrySource keeps the trait's default `restart`.
    let (mut collector, _clock) =
        collector_at(1_000, vec![Box::new(MockTelemetrySource::new(vec![]))], None, InMemoryRepository::default());
    let error = collector.restart_source("mock").expect_err("the default cannot restart");
    assert!(error.to_string().contains("cannot be restarted"), "{error}");
    let report = collector.tick();
    assert_eq!(report.source_errors, 0, "a refused restart must not poison the next pass");
}

#[test]
fn a_failed_restart_is_reported_to_the_caller() {
    let (mut flaky, restarts) = RestartableTelemetrySource::new("etw");
    flaky.fail_restarts();
    let (mut collector, _clock) = collector_at(1_000, vec![Box::new(flaky)], None, InMemoryRepository::default());
    assert!(collector.restart_source("etw").is_err());
    assert_eq!(restarts.count(), 1, "it was attempted");
}

fn repo_with_samples_in_hours(hours: &[i64]) -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    for hour in hours {
        repo.append_metrics(&metrics_sample(hour * 3600 + 60, 10.0, vec![], 0.0, 0)).unwrap();
    }
    repo
}

#[test]
fn a_completed_hour_is_rolled_up_when_the_rollup_is_due() {
    let repo = repo_with_samples_in_hours(&[0]);
    let (mut collector, _) = collector_at(3600 + 10 * 60, vec![], None, repo);

    let report = collector.tick();

    assert_eq!(report.rollup_hours, 1);
    let rolled = collector.repository().system_hours(at_secs(0), at_secs(7200)).unwrap();
    assert_eq!(rolled.len(), 1);
}

#[test]
fn the_hour_still_in_progress_is_not_rolled_up() {
    let repo = repo_with_samples_in_hours(&[0]);
    let (mut collector, _) = collector_at(30 * 60, vec![], None, repo);
    assert_eq!(collector.tick().rollup_hours, 0);
    assert_eq!(collector.repository().rolled_up_before().unwrap(), None);
}

#[test]
fn rollups_wait_for_their_interval_unless_a_backlog_is_waiting() {
    // Hours 0-3 are complete and a pass does one hour at most: the backlog is worked off quickly. Hour 4 only completes at 5:00.
    let repo = repo_with_samples_in_hours(&[0, 1, 2, 3, 4]);
    let start = 4 * 3600 + 58 * 60;
    let (mut collector, clock) = collector_at(start, vec![], None, repo);

    assert_eq!(collector.tick().rollup_hours, 1, "pass 1: hour 0");
    assert_eq!(collector.tick().rollup_hours, 0, "the same instant: the short backlog pause has not passed");
    for (seconds, hour) in [(2, 1), (4, 2), (6, 3)] {
        clock.set(at_secs(start + seconds));
        assert_eq!(collector.tick().rollup_hours, 1, "a backlog does not wait for the 10-minute interval: hour {hour}");
    }

    // Hour 4 is settled by 5:05, but the last pass ran at 4:58 and the interval is 10 minutes.
    clock.set(at_secs(5 * 3600 + 6 * 60));
    assert_eq!(collector.tick().rollup_hours, 0, "settled, but the interval since the last pass has not elapsed");
    clock.set(at_secs(5 * 3600 + 9 * 60));
    assert_eq!(collector.tick().rollup_hours, 1, "hour 4 on the next scheduled pass");
}

// ---- retention gate and storage guard

#[test]
fn detail_that_is_not_rolled_up_is_not_purged_however_old_it_is() {
    let day = 24 * 3600;
    let now = 10 * day;
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    repo.append(&[
        f.make(at_secs(100), dns_query(1, "rolled-up.example", &[])),
        f.make(at_secs(5 * 3600 + 100), dns_query(1, "old-but-not-rolled-up.example", &[])),
        f.make(at_secs(now - 60), dns_query(1, "recent.example", &[])),
    ])
    .unwrap();
    // A pass rolls up one hour at most, so after the first tick only hour 0 is summarised.
    let (mut collector, _) = collector_at(now, vec![], None, repo);

    let report = collector.tick();

    assert_eq!(report.rollup_hours, 1);
    assert_eq!(report.purged.expect("purge ran").events, 1, "only the event in the rolled-up hour went");
    assert_eq!(stored(&collector).len(), 2, "the older event that was not summarised yet stays");
}

/// A drive whose free space the test can change.
#[derive(Clone)]
struct Drive(std::rc::Rc<std::cell::Cell<u64>>);

impl FreeSpace for Drive {
    fn free_bytes(&self) -> Result<u64, String> {
        Ok(self.0.get())
    }
}

const GB: u64 = 1_000_000_000;

fn guard_limits() -> GuardLimits {
    GuardLimits { max_db_bytes: 10 * GB, min_free_bytes: 20 * GB, hard_floor_bytes: 5 * GB }
}

fn guarded(
    start_secs: i64,
    sources: Vec<Box<dyn TelemetrySource>>,
    metrics: Option<MockMetricsSource>,
    repo: InMemoryRepository,
    free_gb: u64,
) -> (Collector<InMemoryRepository, ManualClock>, ManualClock, Drive) {
    let drive = Drive(std::rc::Rc::new(std::cell::Cell::new(free_gb * GB)));
    let (collector, clock) = collector_at(start_secs, sources, metrics, repo);
    (collector.with_storage_guard(guard_limits(), Box::new(drive.clone())), clock, drive)
}

#[test]
fn a_collector_without_a_guard_reports_no_storage_state() {
    let (mut collector, _) = collector_at(100, vec![], None, InMemoryRepository::default());
    collector.tick();
    assert!(collector.storage().is_none());
}

#[test]
fn with_room_to_spare_the_guard_reports_ok_and_the_database_state() {
    let (mut collector, _, _) = guarded(100, vec![], None, InMemoryRepository::default(), 200);
    let report = collector.tick();
    assert_eq!(report.guard_level, GuardLevel::Ok);
    let storage = collector.storage().expect("the guard has measured");
    assert_eq!((storage.level, storage.drive_free_bytes), (GuardLevel::Ok, 200 * GB));
}

#[test]
fn below_the_hard_floor_detail_is_discarded_except_process_events_and_nothing_is_sampled() {
    let mut f = EventFactory::default();
    let source = MockTelemetrySource::new(vec![
        f.make(at_secs(1), process_started(10, 4, 1, r"C:\a.exe")),
        f.make(at_secs(2), dns_query(10, "dropped.example", &[])),
        f.make(at_secs(3), net_activity(10, NetAction::Send, BEACON_TARGET, 100)),
    ]);
    let metrics = MockMetricsSource::new(vec![metrics_sample(5, 10.0, vec![], 0.0, 0)]);
    let (mut collector, clock, _) = guarded(5, vec![Box::new(source)], Some(metrics), InMemoryRepository::default(), 2);

    clock.set(at_secs(10)); // the metrics interval has passed
    let report = collector.tick();

    assert_eq!(report.guard_level, GuardLevel::Paused);
    assert_eq!(
        kinds(&stored(&collector)),
        vec!["process_started"],
        "process starts are kept so traffic after the pause is attributed"
    );
    assert_eq!(report.events_discarded, 2);
    assert!(!report.metrics_sampled, "no samples while paused");
}

#[test]
fn collection_resumes_by_itself_when_free_space_is_back_above_the_minimum() {
    let mut f = EventFactory::default();
    let first = MockTelemetrySource::new(vec![f.make(at_secs(2), dns_query(10, "dropped.example", &[]))]);
    let (mut collector, clock, drive) = guarded(5, vec![Box::new(first)], None, InMemoryRepository::default(), 2);

    assert_eq!(collector.tick().guard_level, GuardLevel::Paused);
    assert!(stored(&collector).is_empty());

    drive.0.set(50 * GB);
    clock.set(at_secs(5 + 60)); // the next guard check
    assert_eq!(collector.tick().guard_level, GuardLevel::Ok, "back above the minimum plus its margin");
}

#[test]
fn an_oversize_database_is_trimmed_by_the_guard_but_never_past_what_is_rolled_up() {
    // One event per hour for 40 hours: 8 KB at the in-memory estimate, against a cap of 1 KB, so the guard always wants to trim.
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    let events: Vec<_> = (0..40).map(|i| f.make(at_secs(i * 3600 + 100), dns_query(1, "x.example", &[]))).collect();
    repo.append(&events).unwrap();
    let now = 40 * 3600 + 600;
    let drive = Drive(std::rc::Rc::new(std::cell::Cell::new(200 * GB)));
    let (collector, clock) = collector_at(now, vec![], None, repo);
    let limits = GuardLimits { max_db_bytes: 1_000, min_free_bytes: 20 * GB, hard_floor_bytes: 5 * GB };
    let mut collector = collector.with_storage_guard(limits, Box::new(drive));

    // A pass rolls up one hour, so the watermark grows by one hour per tick; a guard check follows every minute.
    let mut trimmed = 0;
    for minute in 0..4 {
        clock.set(at_secs(now + minute * 60));
        trimmed += collector.tick().guard_trimmed_steps;
    }

    let remaining = stored(&collector);
    assert!(trimmed >= 1, "the guard trimmed something");
    assert!(remaining.len() < 40, "and some detail went");
    assert!(
        remaining.len() >= 36,
        "the watermark is at hour 4 after four passes, so nothing from hour 4 on may be deleted: {} left",
        remaining.len()
    );
}

#[test]
fn a_long_backlog_of_old_detail_is_purged_an_hour_at_a_time_one_second_apart() {
    let day = 24 * 3600;
    let now = 10 * day;
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    // One event an hour for 30 hours, all far older than the one-day retention.
    let events: Vec<_> = (0..30).map(|h| f.make(at_secs(h * 3600 + 100), dns_query(1, "old.example", &[]))).collect();
    repo.append(&events).unwrap();
    // Everything is rolled up in the first pass, so only the retention window and the pass size limit the purge.
    let wide = CollectorSettings {
        rollup: RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 500 },
        ..settings()
    };
    let clock = ManualClock::new(at_secs(now));
    let mut collector = Collector::new(wide, vec![], None, repo, clock.clone());

    let first = collector.tick();
    assert_eq!(first.purged.expect("the first pass").events, 1, "a pass removes an hour, not the whole backlog");
    assert_eq!(stored(&collector).len(), 29);

    assert!(collector.tick().purged.is_none(), "the same instant: the short pause between passes has not passed");

    let mut passes = 1;
    for second in 1..=40 {
        clock.set(at_secs(now + second));
        if collector.tick().purged.is_some_and(|counts| counts.events > 0) {
            passes += 1;
        }
    }
    assert_eq!(stored(&collector).len(), 0, "the backlog is gone");
    assert_eq!(passes, 30, "30 hours at an hour a pass (a final pass that finds nothing left removes nothing)");
}
