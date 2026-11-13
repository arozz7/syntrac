//! The SQLite and in-memory repositories are fed the same scenario and must produce the same rollups, row for row.

use std::net::IpAddr;
use std::time::Duration;

use syntrac_core::{
    roll_up_due, MetricsRepository, Pipeline, ProcessRepository, RollupRepository, RollupSettings, Timestamp,
};
use syntrac_store::SqliteStore;
use syntrac_testkit::fixtures::{
    at_secs, beacon_scenario, metrics_sample, process_metric, BEACON_TARGET, POWERSHELL_PID,
};
use syntrac_testkit::{InMemoryRepository, MockTelemetrySource};

const POWERSHELL: &str = r"C:\Windows\System32\powershell.exe";

fn feed<R>(repo: &mut R)
where
    R: ProcessRepository + syntrac_core::EventRepository + MetricsRepository,
{
    let mut pipeline = Pipeline::new(Duration::from_secs(10));
    pipeline.ingest(&mut MockTelemetrySource::new(beacon_scenario()), repo).unwrap();
    pipeline.flush_all(repo).unwrap();
    // Metrics every 5 s through the scenario, each with the PowerShell instance that owns the PID at that moment.
    for at in (5..300).step_by(5) {
        let owner = repo.resolve(POWERSHELL_PID, at_secs(at)).unwrap();
        repo.append_metrics(&metrics_sample(
            at,
            10.0 + at as f32 / 100.0,
            vec![process_metric(POWERSHELL_PID, owner, 2.5)],
            1.0,
            3,
        ))
        .unwrap();
    }
}

fn roll_up<R: RollupRepository>(repo: &mut R) {
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 50 };
    roll_up_due(repo, at_secs(3 * 3600), &settings).unwrap();
}

#[test]
fn both_repositories_produce_identical_rollups() {
    let mut sqlite = SqliteStore::open_in_memory().unwrap();
    let mut memory = InMemoryRepository::default();
    feed(&mut sqlite);
    feed(&mut memory);
    roll_up(&mut sqlite);
    roll_up(&mut memory);

    let (from, to) = (Timestamp::from_micros(0), at_secs(10 * 3600));
    let target = IpAddr::V4(BEACON_TARGET);
    assert_eq!(sqlite.rolled_up_before().unwrap(), memory.rolled_up_before().unwrap());
    assert_eq!(sqlite.system_hours(from, to).unwrap(), memory.system_hours(from, to).unwrap());
    assert_eq!(
        sqlite.executable_hours(POWERSHELL, from, to).unwrap(),
        memory.executable_hours(POWERSHELL, from, to).unwrap()
    );
    assert_eq!(
        sqlite.destination_hours(target, from, to).unwrap(),
        memory.destination_hours(target, from, to).unwrap()
    );
    assert_eq!(sqlite.executable_ledger(POWERSHELL).unwrap(), memory.executable_ledger(POWERSHELL).unwrap());
    assert_eq!(sqlite.destination_ledger(target).unwrap(), memory.destination_ledger(target).unwrap());
    assert_eq!(sqlite.pair_ledger(POWERSHELL, target).unwrap(), memory.pair_ledger(POWERSHELL, target).unwrap());

    // And the scenario really produced something to compare.
    assert_eq!(sqlite.system_hours(from, to).unwrap().len(), 1);
    assert!(!sqlite.destination_hours(target, from, to).unwrap().is_empty());
    assert!(sqlite.executable_ledger(POWERSHELL).unwrap().is_some());
}

#[test]
fn a_reader_connection_can_read_rollup_history_while_the_writer_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let mut writer = SqliteStore::open(&path).unwrap();
    feed(&mut writer);
    roll_up(&mut writer);

    let reader = SqliteStore::open_read_only(&path).unwrap();
    let hours = reader.system_hours(Timestamp::from_micros(0), at_secs(10 * 3600)).unwrap();
    assert_eq!(hours.len(), 1);
    assert_eq!(hours[0].samples, 59);
}

#[test]
fn both_repositories_rank_programs_identically() {
    use syntrac_core::{top_programs, ProgramMetric};

    let mut sqlite = SqliteStore::open_in_memory().unwrap();
    let mut memory = InMemoryRepository::default();
    feed(&mut sqlite);
    feed(&mut memory);
    roll_up(&mut sqlite);
    roll_up(&mut memory);

    let (from, to) = (Timestamp::from_micros(0), at_secs(10 * 3600));
    for metric in
        [ProgramMetric::CpuAverage, ProgramMetric::MemoryPeak, ProgramMetric::Starts, ProgramMetric::NetworkBytes]
    {
        let a = top_programs(&sqlite, from, to, metric, 20).unwrap();
        let b = top_programs(&memory, from, to, metric, 20).unwrap();
        assert_eq!(a.len(), b.len(), "{metric:?}");
        for (x, y) in a.iter().zip(&b) {
            // Sums may be added in a different order, so floating point values agree to rounding, not bit for bit.
            assert!((x.cpu_avg_percent - y.cpu_avg_percent).abs() < 1e-9, "{metric:?}");
            assert!((x.cpu_peak_percent - y.cpu_peak_percent).abs() < 1e-9, "{metric:?}");
            assert_eq!(
                (&x.exe, x.memory_peak_bytes, x.starts, x.hours_present, x.bytes_sent, x.bytes_received, x.flows),
                (&y.exe, y.memory_peak_bytes, y.starts, y.hours_present, y.bytes_sent, y.bytes_received, y.flows),
                "{metric:?}"
            );
            assert_eq!((x.destinations, x.ledger), (y.destinations, y.ledger), "{metric:?}");
        }
    }
    let cpu = top_programs(&sqlite, from, to, ProgramMetric::CpuAverage, 20).unwrap();
    assert!(!cpu.is_empty(), "the scenario really produced programs to compare");
}
