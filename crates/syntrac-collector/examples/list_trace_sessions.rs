//! Dev tool: lists the ETW trace sessions the orphan guard can see, and what it WOULD do about them, without
//! stopping anything. Run it elevated to see every session:
//!
//!   cargo run -p syntrac-collector --example list_trace_sessions [own-session-name]

use syntrac_collector::orphan_guard::{plan_orphan_cleanup, SessionHost};
use syntrac_collector::trace_sessions::WindowsSessions;

fn main() {
    let own = std::env::args().nth(1).unwrap_or_else(|| "SyntracCollector".to_owned());
    let host = WindowsSessions;
    match host.list_sessions() {
        Ok(sessions) => {
            println!("{} trace session(s) visible:", sessions.len());
            for name in &sessions {
                println!("  {name}");
            }
            let other_running = host.other_collector_running();
            println!("another syntrac-service process running: {other_running}");
            println!("plan for own session {own:?}: {:?}", plan_orphan_cleanup(&own, &sessions, other_running));
        }
        Err(error) => println!("could not list sessions: {error}"),
    }
}
