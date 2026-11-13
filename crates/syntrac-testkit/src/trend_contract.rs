//! Contract suite for trends: every `TrendRepository` (in-memory, SQLite, ...) must pass these, so the two cannot drift apart.

use std::net::Ipv4Addr;

use syntrac_core::{
    roll_up_due, top_programs, EventRepository, MetricsRepository, ProcessRepository, ProgramMetric, Retention,
    RollupRepository, TrendRepository,
};

use crate::fixtures::{metrics_sample, process_metric, process_started, EventFactory, BEACON_TARGET};
use crate::rollup_contract::{flow_event, secs, settings, start_process, t, A_EXE, B_EXE, OTHER_TARGET};

pub trait TrendSubject:
    TrendRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}
impl<T> TrendSubject for T where
    T: TrendRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}

pub fn trend_contract<R: TrendSubject>(make: impl Fn() -> R) {
    an_empty_database_ranks_nothing(make());
    cpu_is_weighted_by_the_samples_of_each_hour(make());
    only_the_hours_in_the_range_count(make());
    network_totals_leave_out_loopback_and_count_destinations(make());
    starts_and_memory_rank_too(make());
    each_ranked_program_carries_its_permanent_first_and_last_seen(make());
    rankings_survive_the_purge_of_detail(make());
}

/// A in hours 0 and 1: hour 0 has 3 samples (A at 6% in one of them), hour 1 has one sample (A at 8%).
fn two_hours_of_a<R: TrendSubject>(repo: &mut R) {
    let a = start_process(repo, 10, t(0, 0), A_EXE);
    repo.append_metrics(&metrics_sample(secs(0, 10), 10.0, vec![process_metric(10, Some(a.id), 6.0)], 0.0, 0)).unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 20), 10.0, vec![], 0.0, 0)).unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 30), 10.0, vec![], 0.0, 0)).unwrap();
    repo.append_metrics(&metrics_sample(secs(1, 10), 10.0, vec![process_metric(10, Some(a.id), 8.0)], 0.0, 0)).unwrap();
    roll_up_due(repo, t(3, 0), &settings(10)).unwrap();
}

fn an_empty_database_ranks_nothing<R: TrendSubject>(repo: R) {
    for metric in
        [ProgramMetric::CpuAverage, ProgramMetric::MemoryPeak, ProgramMetric::Starts, ProgramMetric::NetworkBytes]
    {
        assert!(top_programs(&repo, t(0, 0), t(9, 0), metric, 10).unwrap().is_empty());
    }
    assert_eq!(repo.system_sample_total(t(0, 0), t(9, 0)).unwrap(), 0);
}

fn cpu_is_weighted_by_the_samples_of_each_hour<R: TrendSubject>(mut repo: R) {
    two_hours_of_a(&mut repo);

    assert_eq!(repo.system_sample_total(t(0, 0), t(5, 0)).unwrap(), 4);
    let top = top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::CpuAverage, 10).unwrap();

    assert_eq!(top.len(), 1);
    let a = &top[0];
    assert_eq!(a.exe.as_ref().unwrap().path, A_EXE);
    // Hour 0: 6% in 1 of 3 samples is an average of 2%. Hour 1: 8%. Weighted by 3 and 1 samples: (2*3 + 8*1) / 4.
    assert!((a.cpu_avg_percent - 3.5).abs() < 1e-6, "{}", a.cpu_avg_percent);
    assert!((a.cpu_peak_percent - 8.0).abs() < 1e-6);
    assert_eq!(a.hours_present, 2);
    assert_eq!(a.memory_peak_bytes, 50_000_000);
}

fn only_the_hours_in_the_range_count<R: TrendSubject>(mut repo: R) {
    two_hours_of_a(&mut repo);

    let top = top_programs(&repo, t(1, 0), t(1, 0), ProgramMetric::CpuAverage, 10).unwrap();

    assert_eq!(repo.system_sample_total(t(1, 0), t(1, 0)).unwrap(), 1);
    assert!((top[0].cpu_avg_percent - 8.0).abs() < 1e-6, "hour 1 alone");
    assert_eq!(top[0].hours_present, 1);
}

fn network_totals_leave_out_loopback_and_count_destinations<R: TrendSubject>(mut repo: R) {
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 100, 200, secs(0, 10)),
        flow_event(&mut f, Some(a.id), OTHER_TARGET, 1, 1, secs(0, 20)),
        flow_event(&mut f, Some(a.id), Ipv4Addr::LOCALHOST, 9_999, 9_999, secs(0, 30)),
        flow_event(&mut f, None, BEACON_TARGET, 3, 0, secs(0, 40)),
    ])
    .unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 5), 1.0, vec![], 0.0, 0)).unwrap();
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    let top = top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::NetworkBytes, 10).unwrap();

    assert_eq!(top.len(), 2);
    let a = &top[0];
    assert_eq!(a.exe.as_ref().unwrap().path, A_EXE);
    assert_eq!((a.bytes_sent, a.bytes_received), (101, 201), "traffic to this machine itself is not counted");
    assert_eq!((a.flows, a.destinations), (2, 2));
    assert!(top[1].exe.is_none(), "the unattributed flows are listed, not hidden");
    assert_eq!(top[1].bytes_sent, 3);
}

fn starts_and_memory_rank_too<R: TrendSubject>(mut repo: R) {
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    let b = start_process(&mut repo, 11, t(0, 1), B_EXE);
    let mut f = EventFactory::default();
    repo.append(&[
        f.make(t(0, 1), process_started(10, 4, secs(0, 1), A_EXE)),
        f.make(t(0, 2), process_started(10, 4, secs(0, 2), A_EXE)),
        f.make(t(0, 3), process_started(11, 4, secs(0, 3), B_EXE)),
    ])
    .unwrap();
    repo.append_metrics(&metrics_sample(
        secs(0, 10),
        5.0,
        vec![process_metric(10, Some(a.id), 1.0), process_metric(11, Some(b.id), 1.0)],
        0.0,
        0,
    ))
    .unwrap();
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    let by_starts = top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::Starts, 10).unwrap();
    assert_eq!(
        by_starts.iter().map(|p| (p.exe.as_ref().unwrap().path.clone(), p.starts)).collect::<Vec<_>>(),
        vec![(A_EXE.to_owned(), 2), (B_EXE.to_owned(), 1)]
    );
    let by_memory = top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::MemoryPeak, 1).unwrap();
    assert_eq!(by_memory.len(), 1, "the limit applies");
    assert_eq!(by_memory[0].exe.as_ref().unwrap().path, A_EXE, "a tie is broken by path");
}

fn each_ranked_program_carries_its_permanent_first_and_last_seen<R: TrendSubject>(mut repo: R) {
    two_hours_of_a(&mut repo);

    let top = top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::CpuAverage, 10).unwrap();

    let ledger = top[0].ledger.expect("a ledger entry");
    assert_eq!((ledger.first_seen, ledger.last_seen), (t(0, 10), t(1, 10)));
}

fn rankings_survive_the_purge_of_detail<R: TrendSubject>(mut repo: R) {
    two_hours_of_a(&mut repo);
    let before = top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::CpuAverage, 10).unwrap();

    repo.purge_before(t(100, 0)).unwrap();

    assert_eq!(top_programs(&repo, t(0, 0), t(5, 0), ProgramMetric::CpuAverage, 10).unwrap(), before);
}
