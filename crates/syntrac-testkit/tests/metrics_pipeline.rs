//! Metrics ingest resolves PIDs to process instances, so a reused PID never inherits old CPU history.

use syntrac_core::{ingest_metrics, MetricsRepository, ProcessRepository};
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::{InMemoryRepository, MockMetricsSource};

fn instance_of(repo: &InMemoryRepository, pid: u32, at: i64) -> Option<syntrac_core::ProcessInstanceId> {
    repo.resolve(pid, at_secs(at)).unwrap()
}

#[test]
fn sampled_pids_are_resolved_to_the_instance_running_at_sample_time() {
    let mut repo = InMemoryRepository::default();
    let first = running_process(10, at_secs(1));
    let second = running_process(10, at_secs(50));
    repo.record_start(first.clone()).unwrap();
    repo.record_exit(first.id, at_secs(20), None).unwrap();
    repo.record_start(second.clone()).unwrap();

    let mut source = MockMetricsSource::new(vec![
        metrics_sample(10, 30.0, vec![process_metric(10, None, 12.0)], 0.0, 0),
        metrics_sample(60, 30.0, vec![process_metric(10, None, 8.0)], 0.0, 0),
    ]);
    ingest_metrics(&mut source, at_secs(10), &mut repo).unwrap();
    ingest_metrics(&mut source, at_secs(60), &mut repo).unwrap();

    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(breakdown.processes.len(), 2, "same PID, two instances, two separate rows");
    let ids: Vec<_> = breakdown.processes.iter().map(|p| p.instance).collect();
    assert!(ids.contains(&Some(first.id)) && ids.contains(&Some(second.id)));
}

#[test]
fn a_pid_with_no_known_process_is_stored_unattributed() {
    let mut repo = InMemoryRepository::default();
    let mut source =
        MockMetricsSource::new(vec![metrics_sample(10, 30.0, vec![process_metric(555, None, 9.0)], 0.0, 0)]);
    ingest_metrics(&mut source, at_secs(10), &mut repo).unwrap();

    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert_eq!(breakdown.processes[0].instance, None);
    assert_eq!(breakdown.processes[0].pid, 555);
    assert!(instance_of(&repo, 555, 10).is_none());
}

#[test]
fn the_long_tail_survives_ingest() {
    let mut repo = InMemoryRepository::default();
    let mut source = MockMetricsSource::new(vec![metrics_sample(10, 30.0, vec![], 25.0, 400)]);
    ingest_metrics(&mut source, at_secs(10), &mut repo).unwrap();

    let breakdown = repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap();
    assert!((breakdown.others_avg_cpu_percent - 25.0).abs() < 1e-3);
}

#[test]
fn a_failing_source_reports_an_error_and_stores_nothing() {
    let mut repo = InMemoryRepository::default();
    let mut source = MockMetricsSource::new(vec![]); // exhausted
    assert!(ingest_metrics(&mut source, at_secs(10), &mut repo).is_err());
    assert_eq!(repo.cpu_breakdown(at_secs(0), at_secs(100), 10).unwrap().window_samples, 0);
}
