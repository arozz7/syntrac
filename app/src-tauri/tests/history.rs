//! The history view models: what the trend screens render, built from the rollup tables and tested against the in-memory
//! store (and, for the backend, a real database file).

use syntrac_app_lib::backend::{Backend, Data};
use syntrac_app_lib::history::{clamp_history_secs, system_history, MAX_HISTORY_SECS, MIN_HISTORY_SECS};
use syntrac_app_lib::settings::AppSettings;
use syntrac_core::{HourRollup, RollupRepository, SystemHour, Timestamp};
use syntrac_store::SqliteStore;
use syntrac_testkit::fixtures::at_secs;
use syntrac_testkit::{InMemoryRepository, ManualClock};

const HOUR: i64 = 3600;

fn hour_row(start_secs: i64, cpu_avg: f64, memory_used: u64) -> SystemHour {
    SystemHour {
        hour: at_secs(start_secs),
        samples: 120,
        cpu_avg,
        cpu_max: cpu_avg * 2.0,
        memory_used_avg: memory_used,
        memory_used_max: memory_used + 1_000,
        memory_total: 10_000,
        disk_read: 11,
        disk_write: 22,
        net_received: 3_000,
        net_sent: 400,
        others_cpu_avg: 1.5,
        gpu_avg: Some(20.0),
        gpu_max: Some(35.0),
    }
}

fn roll(start_secs: i64, row: Option<SystemHour>) -> HourRollup {
    HourRollup { hour: at_secs(start_secs), system: row, processes: Vec::new(), traffic: Vec::new(), names: Vec::new() }
}

fn store_hours(repo: &mut impl RollupRepository, hours: &[(i64, f64)]) {
    for (start, cpu) in hours {
        repo.store_hour(&roll(*start, Some(hour_row(*start, *cpu, 4_000)))).unwrap();
    }
}

fn now() -> Timestamp {
    at_secs(100 * HOUR)
}

#[test]
fn an_hour_row_becomes_percentages_and_millisecond_times() {
    let mut repo = InMemoryRepository::default();
    store_hours(&mut repo, &[(90 * HOUR, 12.5)]);

    let view = system_history(&repo, now(), 24 * 3600).unwrap();

    let hour = &view.hours[0];
    assert_eq!(hour.hour_ms, 90 * HOUR * 1000);
    assert_eq!(hour.samples, 120);
    assert!((hour.cpu_avg_percent - 12.5).abs() < 1e-9);
    assert!((hour.cpu_max_percent - 25.0).abs() < 1e-9);
    assert!((hour.memory_avg_percent - 40.0).abs() < 1e-9, "4000 of 10000 bytes");
    assert!((hour.memory_max_percent - 50.0).abs() < 1e-9);
    assert_eq!((hour.net_received_bytes, hour.net_sent_bytes), (3_000, 400));
    assert_eq!((hour.disk_read_bytes, hour.disk_write_bytes), (11, 22));
    assert!((hour.others_cpu_avg_percent - 1.5).abs() < 1e-9);
    assert_eq!((hour.gpu_avg_percent, hour.gpu_max_percent), (Some(20.0), Some(35.0)));
}

#[test]
fn only_hours_inside_the_range_are_returned_oldest_first() {
    let mut repo = InMemoryRepository::default();
    store_hours(&mut repo, &[(10 * HOUR, 1.0), (80 * HOUR, 2.0), (90 * HOUR, 3.0)]);

    let view = system_history(&repo, now(), 24 * 3600).unwrap();

    let starts: Vec<i64> = view.hours.iter().map(|h| h.hour_ms / 1000 / HOUR).collect();
    assert_eq!(starts, vec![80, 90]);
    assert_eq!(view.to_ms, 100 * HOUR * 1000);
    assert_eq!(view.from_ms, 76 * HOUR * 1000);
}

#[test]
fn the_start_of_history_is_the_oldest_rollup_even_when_the_range_is_shorter() {
    let mut repo = InMemoryRepository::default();
    store_hours(&mut repo, &[(10 * HOUR, 1.0), (90 * HOUR, 3.0)]);

    let view = system_history(&repo, now(), 24 * 3600).unwrap();

    assert_eq!(view.history_starts_ms, Some(10 * HOUR * 1000));
    assert_eq!(view.hours.len(), 1, "the old hour is outside the range");
}

#[test]
fn no_rollups_yet_is_an_empty_history_not_an_error() {
    let repo = InMemoryRepository::default();
    let view = system_history(&repo, now(), 7 * 24 * 3600).unwrap();
    assert!(view.hours.is_empty());
    assert_eq!(view.history_starts_ms, None);
}

#[test]
fn a_gap_in_the_hours_stays_a_gap() {
    let mut repo = InMemoryRepository::default();
    // The collector was down for hours 91 and 92: no rows for them, and none are invented.
    store_hours(&mut repo, &[(90 * HOUR, 5.0), (93 * HOUR, 6.0)]);
    let view = system_history(&repo, now(), 24 * 3600).unwrap();
    assert_eq!(view.hours.len(), 2);
}

#[test]
fn a_zero_memory_total_does_not_divide_by_zero() {
    let mut repo = InMemoryRepository::default();
    let mut row = hour_row(90 * HOUR, 5.0, 0);
    row.memory_total = 0;
    repo.store_hour(&roll(90 * HOUR, Some(row))).unwrap();
    let view = system_history(&repo, now(), 24 * 3600).unwrap();
    assert_eq!(view.hours[0].memory_avg_percent, 0.0);
}

#[test]
fn the_requested_range_is_kept_within_sane_limits() {
    assert_eq!(clamp_history_secs(0), MIN_HISTORY_SECS);
    assert_eq!(clamp_history_secs(u64::MAX), MAX_HISTORY_SECS);
    assert_eq!(clamp_history_secs(7 * 24 * 3600), 7 * 24 * 3600);
}

#[test]
fn the_backend_serves_history_from_a_real_database_and_reports_missing_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store_hours(&mut store, &[(90 * HOUR, 12.5), (91 * HOUR, 20.0)]);
    }
    let settings = |db: &std::path::Path| {
        AppSettings::for_database(db.to_path_buf(), &format!("syntrac-history-test-{}", std::process::id()))
    };

    let backend = Backend::new(settings(&path), ManualClock::new(now()));
    match backend.system_history(24 * 3600) {
        Data::Ok { data } => {
            assert_eq!(data.hours.len(), 2);
            assert_eq!(data.history_starts_ms, Some(90 * HOUR * 1000));
        }
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    }

    let missing = Backend::new(settings(&dir.path().join("nope.db")), ManualClock::new(now()));
    assert!(matches!(missing.system_history(24 * 3600), Data::NoDatabase { .. }));
}

#[test]
fn an_hour_without_gpu_counters_has_no_gpu_figures_not_zeros() {
    let mut repo = InMemoryRepository::default();
    repo.store_hour(&roll(
        90 * HOUR,
        Some(SystemHour { gpu_avg: None, gpu_max: None, ..hour_row(90 * HOUR, 1.0, 4_000) }),
    ))
    .unwrap();

    let view = system_history(&repo, now(), 24 * 3600).unwrap();

    assert_eq!((view.hours[0].gpu_avg_percent, view.hours[0].gpu_max_percent), (None, None));
}
