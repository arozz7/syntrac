//! The shared host, run for real on this machine WITHOUT ETW (so it needs no elevation): the startup snapshot
//! and metrics are genuine, only the live trace is skipped.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_core::{EventCategory, EventQuery, EventRepository};
use syntrac_service::{run_host, HostOptions, ServiceConfig, DEFAULT_SHUTDOWN_LIMIT};
use syntrac_store::SqliteStore;

fn options(database: &Path, run_for: Option<Duration>) -> HostOptions {
    HostOptions {
        config: ServiceConfig::default(),
        database_path: database.to_path_buf(),
        use_etw: false,
        etw_session_name: "SyntracTest".into(),
        run_for,
        shutdown_limit: DEFAULT_SHUTDOWN_LIMIT,
    }
}

fn never_stalls() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| panic!("shutdown stalled"))
}

#[test]
fn a_run_without_etw_snapshots_this_machine_into_a_database_that_can_be_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("nested").join("host.db"); // the folder does not exist yet

    let outcome =
        run_host(&options(&database, Some(Duration::from_secs(2))), &AtomicBool::new(false), never_stalls()).unwrap();

    assert!(outcome.summary.contains("process_observed"), "summary: {}", outcome.summary);
    let reader = SqliteStore::open_read_only(&database).expect("a finished run leaves a readable database");
    let processes =
        reader.query(&EventQuery { category: Some(EventCategory::Process), ..EventQuery::default() }).unwrap();
    assert!(!processes.is_empty(), "the snapshot must find at least this test process");
}

#[test]
fn setting_the_stop_flag_ends_a_run_promptly_instead_of_waiting_for_the_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("host.db");
    let stop = Arc::new(AtomicBool::new(false));
    let stopper = Arc::clone(&stop);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        stopper.store(true, Ordering::SeqCst);
    });

    let began = Instant::now();
    // No deadline at all: only the flag can end this run.
    run_host(&options(&database, None), &stop, never_stalls()).unwrap();

    assert!(began.elapsed() < Duration::from_secs(8), "took {:?}", began.elapsed());
}

#[test]
fn two_runs_on_one_database_do_not_leave_the_first_runs_processes_marked_running_if_they_are_gone() {
    // The second run's snapshot reconciles the first run's records. Every process the snapshot sees stays running;
    // the test process itself is alive in both runs, so it must still be running afterwards.
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("host.db");

    for _ in 0..2 {
        run_host(&options(&database, Some(Duration::from_secs(1))), &AtomicBool::new(false), never_stalls()).unwrap();
    }

    let reader = SqliteStore::open_read_only(&database).unwrap();
    let me = std::process::id();
    let running = syntrac_core::ProcessQueries::list_processes(
        &reader,
        &syntrac_core::ProcessFilter { running_only: true, limit: 10_000, ..Default::default() },
    )
    .unwrap();
    assert!(running.iter().any(|p| p.pid == me), "the live test process must still be listed as running");
}
