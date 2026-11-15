//! Rehearses the v9 upgrade and the context worker's passes on a COPY of a real database. The source is only read.
//!
//!   cargo run --release -p syntrac-store --example rehearse_context -- <source.db> <work-dir>
//!
//! The copy is made with `VACUUM INTO`, upgraded to the latest schema, and then two real passes are run with the Windows adapter
//! and the same throttling the service uses. Prints the upgrade time, what each pass wrote (the second must write nothing),
//! how many programs now have an account and a service, and what the app would show for a few of them. Run it elevated to see
//! what the SYSTEM service will see. Nothing here deletes anything; the work directory is left for you to inspect and remove.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use syntrac_collector::process_context::WindowsProcessContextSource;
use syntrac_collector::system::is_elevated;
use syntrac_core::{observe_context, Clock, ContextRepository, ContextSettings, ContextTracker, SystemClock};
use syntrac_store::SqliteStore;

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("context-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());
    println!("elevated: {}", is_elevated());

    let started = Instant::now();
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    println!("copied in {:.1}s", started.elapsed().as_secs_f64());

    let started = Instant::now();
    let mut store = SqliteStore::open(&copy).expect("open and upgrade the copy");
    println!("upgrade to v{} in {:.2}s", store.schema_version().unwrap(), started.elapsed().as_secs_f64());
    let executables_before: i64 = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0))
        .unwrap();

    let source = WindowsProcessContextSource::new();
    let settings = ContextSettings::default();
    let mut tracker = ContextTracker::new(Duration::from_secs(600));
    for pass_number in 1..=2 {
        let started = Instant::now();
        let pass = observe_context(&source, &mut store, &mut tracker, SystemClock.now(), &settings).expect("a pass");
        println!(
            "pass {pass_number}: {:.0} ms; seen {}, written {}, new accounts {}, new services {}, skipped {}, unreadable {}",
            started.elapsed().as_secs_f64() * 1000.0,
            pass.seen,
            pass.written,
            pass.new_accounts,
            pass.new_services,
            pass.skipped,
            pass.unreadable
        );
    }

    let all = store.all_contexts().expect("read back");
    let with_services = all.iter().filter(|(_, context)| !context.services.is_empty()).count();
    println!("programs with an account: {}, hosting at least one service: {with_services}", all.len());
    let executables_after: i64 = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM executables", [], |r| r.get(0))
        .unwrap();
    println!(
        "executables: {executables_before} before, {executables_after} after ({} new to the rollups: they get history when they next appear in an hour)",
        executables_after - executables_before
    );
    for (exe, context) in all.iter().filter(|(_, c)| !c.services.is_empty()).take(6) {
        let accounts: Vec<String> = context.accounts.iter().map(|a| a.kind.as_str().to_owned()).collect();
        let services: Vec<&str> = context.services.iter().map(|s| s.name.as_str()).take(4).collect();
        println!("  {}  as {accounts:?}  services {services:?}", exe.path);
    }
    println!("copy left at {}", copy.display());
}
