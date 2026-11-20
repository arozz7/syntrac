//! Read-only: how long does one read of the GPU counters take, how big is the answer, and does a long-lived query see processes that
//! started AFTER it was opened (docs/ALERTS-PLAN.md, slice G1)? Prints counts and timings only, no program names.
//!
//! Usage: `measure_gpu [rounds]` (one round per second, default 12). At the end a FRESH query is opened and compared with the
//! long-lived one: any process only the fresh one knows is a process the long-lived query missed.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use syntrac_collector::gpu_counters::PdhGpuSource;
use syntrac_core::{gpu_figures, GpuSource};

fn pids_of(source: &mut PdhGpuSource) -> Option<BTreeSet<u32>> {
    source.read().map(|reading| gpu_figures(&reading).by_pid.keys().copied().collect())
}

fn main() {
    let rounds: usize = std::env::args().nth(1).and_then(|text| text.parse().ok()).unwrap_or(12);
    let Some(mut source) = PdhGpuSource::open() else {
        println!("The GPU counters are not available on this machine.");
        return;
    };
    let mut millis = Vec::new();
    let mut seen_by_long_lived: BTreeSet<u32> = BTreeSet::new();
    for round in 0..rounds {
        std::thread::sleep(Duration::from_secs(1));
        let started = Instant::now();
        let reading = source.read();
        let read_ms = started.elapsed().as_secs_f64() * 1000.0;
        let Some(reading) = reading else {
            println!("round {round}: no reading");
            continue;
        };
        let figures = gpu_figures(&reading);
        seen_by_long_lived = figures.by_pid.keys().copied().collect();
        println!(
            "round {round}: read {read_ms:.1} ms, {} engine rows, {} memory rows, {} processes, busiest engine {:.1}%",
            reading.engines.len(),
            reading.memory.len(),
            figures.by_pid.len(),
            figures.busiest_percent
        );
        millis.push(read_ms);
    }
    if !millis.is_empty() {
        let average = millis.iter().sum::<f64>() / millis.len() as f64;
        let worst = millis.iter().copied().fold(0.0, f64::max);
        println!(
            "average read {average:.1} ms, worst {worst:.1} ms (at a 5 s cadence the average is {:.2}% of one core)",
            average / 50.0
        );
    }

    // A fresh query: does it know processes the long-lived one never listed?
    let Some(mut fresh) = PdhGpuSource::open() else { return };
    std::thread::sleep(Duration::from_secs(1));
    let _ = pids_of(&mut fresh);
    std::thread::sleep(Duration::from_secs(1));
    let Some(fresh_pids) = pids_of(&mut fresh) else { return };
    let long_lived_now = pids_of(&mut source).unwrap_or_default();
    let missed: Vec<u32> = fresh_pids.difference(&long_lived_now).copied().collect();
    let only_old: Vec<u32> = long_lived_now.difference(&fresh_pids).copied().collect();
    println!(
        "final: long-lived query knows {} processes, a fresh query knows {}; the long-lived one MISSES {} ({:?}); only the long-lived one lists {} (ended since)",
        long_lived_now.len(),
        fresh_pids.len(),
        missed.len(),
        missed,
        only_old.len()
    );
    println!("(the long-lived query listed {} processes in its last round)", seen_by_long_lived.len());
}
