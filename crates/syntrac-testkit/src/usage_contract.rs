//! Contract suite for the hourly usage rows the usage rules read (docs/ALERTS-PLAN.md, F3): every `UsageRepository` (in-memory,
//! SQLite, ...) must pass these, so the two cannot drift apart.

use std::net::Ipv4Addr;

use syntrac_core::{
    roll_up_due, EventRepository, MetricsRepository, ProcessHourRow, ProcessRepository, Retention, RollupRepository,
    TrafficHourRow, UsageRepository,
};

use crate::fixtures::{metrics_sample, process_metric, EventFactory, BEACON_TARGET};
use crate::rollup_contract::{flow_event, secs, settings, start_process, t, A_EXE, B_EXE, OTHER_TARGET};

pub trait UsageSubject:
    UsageRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}
impl<T> UsageSubject for T where
    T: UsageRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}

pub fn usage_contract<R: UsageSubject>(make: impl Fn() -> R) {
    an_empty_database_has_no_rows(make());
    cpu_and_memory_come_back_per_program_and_hour(make());
    traffic_is_added_up_over_addresses_and_leaves_out_loopback_and_unattributed_programs(make());
    only_the_hours_in_the_range_are_returned(make());
    the_rows_survive_the_purge_of_detail(make());
}

/// Hour 0: A uses 30% CPU and talks to two addresses (and to loopback); an unattributed program talks too. Hour 1: A uses 10%, B
/// uploads 7 bytes.
fn scenario<R: UsageSubject>(repo: &mut R) {
    let a = start_process(repo, 10, t(0, 0), A_EXE);
    let b = start_process(repo, 11, t(0, 0), B_EXE);
    repo.append_metrics(&metrics_sample(secs(0, 10), 40.0, vec![process_metric(10, Some(a.id), 30.0)], 0.0, 0))
        .unwrap();
    repo.append_metrics(&metrics_sample(secs(1, 10), 15.0, vec![process_metric(10, Some(a.id), 10.0)], 0.0, 0))
        .unwrap();
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 100, 200, secs(0, 15)),
        flow_event(&mut f, Some(a.id), OTHER_TARGET, 1, 1, secs(0, 20)),
        flow_event(&mut f, Some(a.id), Ipv4Addr::LOCALHOST, 999, 999, secs(0, 25)),
        flow_event(&mut f, None, BEACON_TARGET, 5, 5, secs(0, 30)),
        flow_event(&mut f, Some(b.id), BEACON_TARGET, 7, 0, secs(1, 15)),
    ])
    .unwrap();
    roll_up_due(repo, t(4, 0), &settings(10)).unwrap();
}

fn an_empty_database_has_no_rows<R: UsageSubject>(repo: R) {
    assert!(repo.process_hour_rows(t(0, 0), t(9, 0)).unwrap().is_empty());
    assert!(repo.traffic_hour_rows(t(0, 0), t(9, 0)).unwrap().is_empty());
}

fn sorted_process(mut rows: Vec<ProcessHourRow>) -> Vec<ProcessHourRow> {
    rows.sort_by(|a, b| a.hour.cmp(&b.hour).then(a.exe.key.cmp(&b.exe.key)));
    rows
}

fn sorted_traffic(mut rows: Vec<TrafficHourRow>) -> Vec<TrafficHourRow> {
    rows.sort_by(|a, b| a.hour.cmp(&b.hour).then(a.exe.key.cmp(&b.exe.key)));
    rows
}

fn cpu_and_memory_come_back_per_program_and_hour<R: UsageSubject>(mut repo: R) {
    scenario(&mut repo);

    let rows = sorted_process(repo.process_hour_rows(t(0, 0), t(5, 0)).unwrap());

    assert_eq!(rows.len(), 2, "A in two hours; B has no CPU sample and no row");
    assert_eq!((rows[0].exe.path.as_str(), rows[0].hour), (A_EXE, t(0, 0)));
    assert!((rows[0].cpu_avg - 30.0).abs() < 0.01, "{}", rows[0].cpu_avg);
    assert_eq!(rows[0].memory_peak, 50_000_000);
    assert_eq!(rows[1].hour, t(1, 0));
    assert!((rows[1].cpu_avg - 10.0).abs() < 0.01);
}

fn traffic_is_added_up_over_addresses_and_leaves_out_loopback_and_unattributed_programs<R: UsageSubject>(mut repo: R) {
    scenario(&mut repo);

    let rows = sorted_traffic(repo.traffic_hour_rows(t(0, 0), t(5, 0)).unwrap());

    assert_eq!(rows.len(), 2, "A in hour 0 and B in hour 1; nothing for loopback or the unattributed bucket");
    assert_eq!(
        (rows[0].exe.path.as_str(), rows[0].hour, rows[0].bytes_sent, rows[0].bytes_received),
        (A_EXE, t(0, 0), 101, 201)
    );
    assert_eq!(
        (rows[1].exe.path.as_str(), rows[1].hour, rows[1].bytes_sent, rows[1].bytes_received),
        (B_EXE, t(1, 0), 7, 0)
    );
}

fn only_the_hours_in_the_range_are_returned<R: UsageSubject>(mut repo: R) {
    scenario(&mut repo);

    let process = repo.process_hour_rows(t(1, 0), t(5, 0)).unwrap();
    let traffic = repo.traffic_hour_rows(t(1, 0), t(5, 0)).unwrap();

    assert_eq!(process.len(), 1);
    assert_eq!(process[0].hour, t(1, 0));
    assert_eq!(traffic.len(), 1);
    assert_eq!(traffic[0].exe.path, B_EXE);
    assert!(repo.process_hour_rows(t(2, 0), t(5, 0)).unwrap().is_empty());
}

fn the_rows_survive_the_purge_of_detail<R: UsageSubject>(mut repo: R) {
    scenario(&mut repo);

    repo.purge_before(syntrac_core::Timestamp::from_micros(1_000_000_000_000_000)).unwrap();

    assert_eq!(repo.process_hour_rows(t(0, 0), t(5, 0)).unwrap().len(), 2);
    assert_eq!(repo.traffic_hour_rows(t(0, 0), t(5, 0)).unwrap().len(), 2);
}
