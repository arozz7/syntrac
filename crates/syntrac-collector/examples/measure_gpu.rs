//! Read-only: how long does one read of the GPU counters take, and how big is the answer (docs/ALERTS-PLAN.md, slice G1)?
//! Prints counts and timings only, no program names.

use std::time::{Duration, Instant};

use syntrac_collector::gpu_counters::PdhGpuSource;
use syntrac_core::{gpu_figures, GpuSource};

fn main() {
    let Some(mut source) = PdhGpuSource::open() else {
        println!("The GPU counters are not available on this machine.");
        return;
    };
    let mut millis = Vec::new();
    for round in 0..12 {
        std::thread::sleep(Duration::from_secs(1));
        let started = Instant::now();
        let reading = source.read();
        let read_ms = started.elapsed().as_secs_f64() * 1000.0;
        let Some(reading) = reading else {
            println!("round {round}: no reading");
            continue;
        };
        let figures_started = Instant::now();
        let figures = gpu_figures(&reading);
        let figures_ms = figures_started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "round {round}: read {read_ms:.1} ms, figures {figures_ms:.2} ms, {} engine rows, {} memory rows, {} processes, busiest engine {:.1}%",
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
}
