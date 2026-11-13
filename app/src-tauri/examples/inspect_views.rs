//! Dev tool: prints what the UI would receive from the real collector (health, overview, processes, connections).
//!
//!     cargo run -p syntrac-app --example inspect_views [range_secs]
//!
//! Honours SYNTRAC_DB / SYNTRAC_PIPE like the app does.

use syntrac_app_lib::backend::{Backend, Data};
use syntrac_app_lib::settings::AppSettings;
use syntrac_app_lib::views::ProcessListParams;
use syntrac_core::SystemClock;

fn main() {
    let range_secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(900);
    let settings = AppSettings::from_env();
    println!("database: {}\npipe:     {}\n", settings.database_path.display(), settings.pipe_name);
    let backend = Backend::new(settings, SystemClock);

    let health = backend.health();
    println!("HEALTH  {:?}: {}", health.state, health.message);
    println!(
        "        pid {:?}, uptime {:?}s, data age {:?}s, etw dropped {:?}\n",
        health.pid, health.uptime_secs, health.data_age_secs, health.etw_dropped
    );

    match backend.overview(range_secs) {
        Data::Ok { data } => {
            println!(
                "OVERVIEW (last {range_secs}s): {} chart points, {} samples",
                data.system.len(),
                data.cpu.window_samples
            );
            println!(
                "  CPU average: system {:.1}% = listed {:.1}% + long tail {:.1}% + NOT ATTRIBUTED {:.1}%",
                data.cpu.system_avg_percent,
                data.cpu.processes.iter().map(|p| p.avg_percent).sum::<f32>(),
                data.cpu.others_avg_percent,
                data.cpu.unaccounted_avg_percent
            );
            for p in data.cpu.processes.iter().take(6) {
                println!("    {:5.2}% avg {:5.2}% peak  {} (pid {})", p.avg_percent, p.peak_percent, p.name, p.pid);
            }
            println!("  busiest destinations:");
            for d in data.destinations.iter().take(4) {
                println!(
                    "    {}:{}  sent {}  received {}  ({} flows, {} processes)",
                    d.remote_addr, d.remote_port, d.bytes_sent, d.bytes_received, d.flow_count, d.process_count
                );
            }
        }
        other => println!("OVERVIEW: {}", serde_json::to_string(&other).unwrap_or_default()),
    }

    match backend.processes(ProcessListParams { running_only: true, limit: Some(5), ..ProcessListParams::default() }) {
        Data::Ok { data } => {
            println!("\nPROCESSES (5 newest running):");
            for p in &data {
                println!("  {} (pid {}) started {}", p.name, p.pid, p.started_ms);
            }
        }
        other => println!("PROCESSES: {}", serde_json::to_string(&other).unwrap_or_default()),
    }
    match backend.connections(range_secs, Some(5)) {
        Data::Ok { data } => {
            println!("\nCONNECTIONS (5 newest):");
            for c in &data {
                println!(
                    "  {} -> {}:{} {}  sent {} received {}",
                    c.name, c.remote_addr, c.remote_port, c.protocol, c.bytes_sent, c.bytes_received
                );
            }
        }
        other => println!("CONNECTIONS: {}", serde_json::to_string(&other).unwrap_or_default()),
    }
}
