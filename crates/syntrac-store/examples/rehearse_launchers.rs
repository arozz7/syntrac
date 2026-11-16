//! Rehearses the launch-record sweep on a COPY of a real database. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_launchers -- <source.db> <work-dir>
//!
//! The copy is made with `VACUUM INTO`, upgraded to the latest schema, then two real sweeps are run with the real sources (services,
//! scheduled tasks, Run keys, Startup folders): the first writes what it finds, the second must find nothing new. Run it ELEVATED to
//! see what the SYSTEM service will see (unelevated, the Task Scheduler folder cannot be read, and the tasks are reported as failed).
//! Nothing here deletes anything; the work directory is left for you to inspect and remove.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_collector::launch_env::{environment, file_exists};
use syntrac_collector::launch_registry::RunKeyLaunchSource;
use syntrac_collector::launch_services::ServiceLaunchSource;
use syntrac_collector::launch_startup::StartupLaunchSource;
use syntrac_collector::launch_tasks::TaskLaunchSource;
use syntrac_collector::system::is_elevated;
use syntrac_core::{sweep_launchers, Clock, LaunchSource, LauncherRepository, SystemClock};
use syntrac_store::SqliteStore;

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("launchers-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());
    println!("elevated: {}", is_elevated());

    let started = Instant::now();
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    println!("copied in {:.1}s", started.elapsed().as_secs_f64());
    let mut store = SqliteStore::open(&copy).expect("open and upgrade the copy");
    println!("schema v{}", store.schema_version().unwrap());

    let services = ServiceLaunchSource;
    let tasks = TaskLaunchSource::for_this_machine();
    let runs = RunKeyLaunchSource;
    let startup = StartupLaunchSource::for_this_machine();
    let sources: Vec<&dyn LaunchSource> = vec![&services, &tasks, &runs, &startup];

    for pass in 1..=2 {
        let started = Instant::now();
        let report =
            sweep_launchers(&sources, &environment, &file_exists, &mut store, SystemClock.now()).expect("a sweep");
        println!(
            "sweep {pass}: {:.0} ms; entries {}, recorded {}, new {}, unresolved {}, missing files {}, unreadable {}, failed sources {:?}",
            started.elapsed().as_secs_f64() * 1000.0,
            report.entries,
            report.recorded,
            report.new,
            report.unresolved,
            report.missing_files,
            report.unreadable,
            report.failed
        );
    }

    let all = store.all_launchers().expect("read back");
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    let mut boot = 0usize;
    for (_, entries) in &all {
        for entry in entries {
            *by_kind.entry(entry.kind.as_str()).or_default() += 1;
        }
        if entries.iter().any(|e| e.current && e.kind.starts_with_windows()) {
            boot += 1;
        }
    }
    println!("programs with an entry: {}, starting with Windows: {boot}", all.len());
    for (kind, count) in by_kind {
        println!("  {count:5}  {kind}");
    }
    println!("a few programs and how they start:");
    for (exe, entries) in all.iter().filter(|(_, e)| e.len() > 1).take(5) {
        let ways: Vec<&str> = entries.iter().map(|e| e.kind.as_str()).collect();
        println!("  {}  {ways:?}", exe.path);
    }
    println!("copy left at {}", copy.display());
}
