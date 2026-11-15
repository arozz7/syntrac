//! Measures what one context pass costs on this machine, and what 30-second passes would cost over a day.
//!
//!   cargo run --release -p syntrac-collector --example probe_context -- [passes]
//!
//! Run it unelevated and elevated: the elevated numbers are the ones that matter (the service runs as SYSTEM and can open far more
//! processes). It only READS: nothing is written anywhere. Prints, per pass: wall time and the CPU time this process used, and
//! the totals of what was seen (programs, accounts, services, unreadable processes).

use std::collections::BTreeMap;
use std::time::Instant;

use syntrac_collector::process_context::WindowsProcessContextSource;
use syntrac_collector::system::{is_elevated, own_cpu_time};
use syntrac_core::{classify_sid, merge_snapshot, ProcessContextSource};

fn main() {
    let passes: usize = std::env::args().nth(1).and_then(|text| text.parse().ok()).unwrap_or(20);
    let source = WindowsProcessContextSource::new();
    println!("elevated: {}", is_elevated());

    let mut wall_ms = Vec::new();
    let mut last = None;
    // CPU time is only counted in scheduler ticks (about 15.6 ms), so it is measured over ALL the passes and averaged.
    let cpu_before = own_cpu_time();
    for _ in 0..passes {
        let started = Instant::now();
        let snapshot = source.observe().expect("a pass");
        wall_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        last = Some(snapshot);
    }
    let cpu_per_pass_ms = (own_cpu_time() - cpu_before).as_secs_f64() * 1000.0 / passes as f64;
    wall_ms.sort_by(f64::total_cmp);
    let median = |values: &[f64]| values[values.len() / 2];

    let snapshot = last.expect("at least one pass");
    let merged = merge_snapshot(snapshot.clone());
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for observation in &merged.observations {
        *by_kind.entry(classify_sid(&observation.sid).as_str()).or_default() += 1;
    }
    let services: usize = merged.observations.iter().map(|o| o.services.len()).sum();

    println!("passes: {passes}");
    println!(
        "wall time per pass:  first {:.1} ms, median {:.1} ms, max {:.1} ms (the first pass fills the name cache)",
        wall_ms.first().copied().unwrap_or(0.0),
        median(&wall_ms),
        wall_ms.last().copied().unwrap_or(0.0)
    );
    println!("cpu time per pass:   {cpu_per_pass_ms:.1} ms on average over all passes");
    println!(
        "seen: {} processes readable, {} could not be opened, {} program/account pairs, {} service links",
        snapshot.processes.len(),
        snapshot.unreadable,
        merged.observations.len(),
        services
    );
    println!("pairs by account kind: {by_kind:?}");

    let per_day = 24.0 * 3600.0 / 30.0;
    let cpu_seconds_per_day = cpu_per_pass_ms / 1000.0 * per_day;
    println!(
        "at one pass every 30 s: {per_day:.0} passes a day, about {cpu_seconds_per_day:.1} CPU seconds a day = {:.4}% of one core",
        cpu_seconds_per_day / 86_400.0 * 100.0
    );
}
