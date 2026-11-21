//! `make-demo-db <database> [now-seconds]`: writes the demo database (a fictional machine; see `docs/PUBLIC-PLAN.md`). Refuses to
//! overwrite a file. `now-seconds` (seconds since 1970) defaults to the current time; a fixed one makes the output repeatable.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next().map(PathBuf::from) else {
        println!("usage: make-demo-db <database> [now-seconds]");
        std::process::exit(2);
    };
    let now = args.next().and_then(|text| text.parse::<i64>().ok()).unwrap_or_else(|| {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0))
    });
    match syntrac_demo::generate(&path, now) {
        Ok(summary) => {
            println!(
                "Demo database written to {}: {} samples, {} processes, {} events, {} findings.",
                path.display(),
                summary.samples,
                summary.processes,
                summary.events,
                summary.findings.len()
            );
            for (kind, tier) in &summary.findings {
                println!("  {tier:10} {kind}");
            }
        }
        Err(message) => {
            println!("{message}");
            std::process::exit(1);
        }
    }
}
