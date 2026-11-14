//! Reads the History view models from a real database, READ-ONLY, and prints a summary with timings.
//! Usage: `cargo run -p syntrac-app --example history_probe -- <database path>`

use std::time::Instant;

use syntrac_app_lib::history::system_history;
use syntrac_app_lib::history_programs::{range_totals, rank_range, ProgramMetricParam};
use syntrac_core::Clock;
use syntrac_core::SystemClock;
use syntrac_core::TrendRepository;
use syntrac_store::SqliteStore;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: history_probe <database path>");
        std::process::exit(2);
    };
    let store = SqliteStore::open_read_only(std::path::Path::new(&path)).expect("open read-only");
    let now = SystemClock.now();
    let (from, to) = (syntrac_core::Timestamp::from_micros(now.as_micros() - 365 * 86_400 * 1_000_000), now);
    let started = Instant::now();
    let samples = TrendRepository::system_sample_total(&store, from, to).expect("samples");
    println!("  system_sample_total: {samples} samples, {:?}", started.elapsed());
    let started = Instant::now();
    let process = TrendRepository::process_totals(&store, from, to).expect("process totals");
    println!("  process_totals: {} programs, {:?}", process.len(), started.elapsed());
    let started = Instant::now();
    let traffic = TrendRepository::traffic_totals(&store, from, to).expect("traffic totals");
    println!("  traffic_totals: {} programs, {:?}", traffic.len(), started.elapsed());
    let started = Instant::now();
    let range = range_totals(&store, now, 365 * 86_400).expect("range totals");
    println!("range_totals (a year): {:?}", started.elapsed());
    for (label, metric) in [
        ("cpu", ProgramMetricParam::Cpu),
        ("memory", ProgramMetricParam::Memory),
        ("starts", ProgramMetricParam::Starts),
        ("network", ProgramMetricParam::Network),
    ] {
        let started = Instant::now();
        let view =
            rank_range(&store, &range, metric, 5, syntrac_app_lib::history_programs::Grouping::None).expect("programs");
        println!(
            "programs by {label:>7}: {} listed of {} hours, {:?}",
            view.programs.len(),
            view.hours_in_range,
            started.elapsed()
        );
        for p in &view.programs {
            println!(
                "          {:<28} cpu avg {:>5.2}%  peak {:>5.1}%  mem {:>6} MB  starts {:>3}  in {:>3} h  sent {:>10}  recv {:>10}",
                p.name,
                p.cpu_avg_percent,
                p.cpu_peak_percent,
                p.memory_peak_bytes / 1_000_000,
                p.starts,
                p.hours_present,
                p.bytes_sent,
                p.bytes_received
            );
        }
    }
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
