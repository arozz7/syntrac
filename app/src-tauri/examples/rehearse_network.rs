//! Rehearses the Network history on a COPY of a real database. The source is only read.
//!
//!   cargo run --release -p syntrac-app --example rehearse_network -- <source.db> <work-dir>
//!
//! The copy is made with `VACUUM INTO` (a consistent snapshot while the service keeps writing), upgraded to the latest schema,
//! given its names by the backfill, and then every Network query is run with its timing: the ranking over 24 hours, 7 days and
//! all history, what is new, and the detail of the busiest destination. The work directory is left for you to inspect and
//! remove.

use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use syntrac_app_lib::history_network::{
    destination_history, destination_ranking, new_destinations_view, DestinationsView,
};
use syntrac_core::{backfill_names, AddressScope, Clock, DestinationOrder, SystemClock};
use syntrac_store::SqliteStore;

const DAY: u64 = 86_400;

fn show(view: &DestinationsView, rows: usize) {
    for row in view.destinations.iter().take(rows) {
        println!(
            "  {:<40} {:<36} sent {:>12} recv {:>12} flows {:>6} programs {}",
            row.addr,
            row.name.as_deref().unwrap_or("(no name seen)"),
            row.bytes_sent,
            row.bytes_received,
            row.flows,
            row.programs
        );
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().expect("source database"));
    let work = PathBuf::from(args.next().expect("work directory"));
    std::fs::create_dir_all(&work).expect("create work dir");
    let copy = work.join("network-rehearsal.db");
    assert!(!copy.exists(), "{} already exists; remove it first", copy.display());

    let started = Instant::now();
    {
        let src =
            Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open source read-only");
        src.execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()]).expect("copy");
    }
    println!("copied in {:.1}s", started.elapsed().as_secs_f64());

    let now = SystemClock.now();
    {
        let started = Instant::now();
        let mut store = SqliteStore::open(&copy).expect("open and upgrade the copy");
        println!("upgrade to v{} in {:.2}s", store.schema_version().unwrap(), started.elapsed().as_secs_f64());
        let started = Instant::now();
        let done = backfill_names(&mut store, now).expect("backfill");
        println!("backfill: pairs={} in {:.2}s", done.names, started.elapsed().as_secs_f64());
    }

    let store = SqliteStore::open_read_only(&copy).expect("open read-only");
    let mut busiest = None;
    for (label, secs) in [("24 hours", DAY), ("7 days", 7 * DAY), ("all history", 3650 * DAY)] {
        for (scope_label, scope) in [("internet", AddressScope::Internet), ("all addresses", AddressScope::All)] {
            let started = Instant::now();
            let view = destination_ranking(&store, now, secs, DestinationOrder::Bytes, scope, 25).expect("ranking");
            println!(
                "ranking {label}, {scope_label}: {} rows in {:.0} ms (history begins {:?}, {} hours with data)",
                view.destinations.len(),
                started.elapsed().as_secs_f64() * 1000.0,
                view.history_starts_ms,
                view.hours_in_range
            );
            if label == "7 days" && scope_label == "internet" {
                show(&view, 10);
                busiest = view.destinations.first().map(|row| row.addr.clone());
            }
        }
    }

    for (label, secs) in [("last day", DAY), ("last 7 days", 7 * DAY), ("last 30 days", 30 * DAY)] {
        let started = Instant::now();
        let view = new_destinations_view(&store, now, secs, AddressScope::Internet, 25).expect("new destinations");
        println!(
            "new destinations, {label}: {} rows in {:.0} ms (baseline until {:?})",
            view.destinations.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            view.baseline_until_ms
        );
        for row in view.destinations.iter().take(5) {
            println!(
                "  {:<40} {:<36} first by {}",
                row.addr,
                row.name.as_deref().unwrap_or("(no name seen)"),
                row.first_program.as_deref().unwrap_or("(unknown program)")
            );
        }
    }

    if let Some(addr) = busiest {
        let started = Instant::now();
        let view = destination_history(&store, now, 7 * DAY, &addr).expect("history");
        println!(
            "history of {addr}: {} names, {} programs, {} hours in {:.0} ms",
            view.names.len(),
            view.programs.len(),
            view.hours.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    println!("copy left at {}", copy.display());
}
