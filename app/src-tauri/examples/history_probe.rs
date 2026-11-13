//! Reads the History view models from a real database, READ-ONLY, and prints a summary with timings.
//! Usage: `cargo run -p syntrac-app --example history_probe -- <database path>`

use std::time::Instant;

use syntrac_app_lib::history::system_history;
use syntrac_core::Clock;
use syntrac_core::SystemClock;
use syntrac_store::SqliteStore;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: history_probe <database path>");
        std::process::exit(2);
    };
    let store = SqliteStore::open_read_only(std::path::Path::new(&path)).expect("open read-only");
    let now = SystemClock.now();
    for (label, secs) in [("24 hours", 86_400_u64), ("7 days", 7 * 86_400), ("1 year", 365 * 86_400), ("all", u64::MAX)]
    {
        let started = Instant::now();
        let view = system_history(&store, now, secs).expect("history");
        println!(
            "{label:>8}: {:>5} hours, history starts {:?}, {:?}",
            view.hours.len(),
            view.history_starts_ms,
            started.elapsed()
        );
        if let (Some(first), Some(last)) = (view.hours.first(), view.hours.last()) {
            println!(
                "          first hour {} ms (cpu avg {:.1}%), last hour {} ms (cpu avg {:.1}%, memory {:.1}%)",
                first.hour_ms, first.cpu_avg_percent, last.hour_ms, last.cpu_avg_percent, last.memory_avg_percent
            );
        }
    }
}
