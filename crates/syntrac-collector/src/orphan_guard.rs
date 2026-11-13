//! Cleans up trace sessions that SYNTRAC itself left behind.
//!
//! An ETW session outlives the process that started it. A leftover SYNTRAC session was seen to starve every other
//! session on the machine of events (the installed service received nothing for over an hour while reporting
//! healthy), so before a collector starts its own trace it stops leftovers of SYNTRAC's own - but only when it is
//! certain they ARE leftovers: no other collector process is running, and the session has a name SYNTRAC gives to
//! its own collectors. Anything else is reported, never stopped.
//!
//! The decision is pure (`plan_orphan_cleanup`); the machine is reached through `SessionHost`, so it is tested
//! without touching Windows.

use tracing::{info, warn};

/// Names SYNTRAC gives to the trace sessions of its collectors (service and console).
const COLLECTOR_SESSIONS: [&str; 2] = ["SyntracCollector", "SyntracConsole"];
const SYNTRAC_PREFIX: &str = "syntrac";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrphanAction {
    /// A leftover collector session, certainly nobody's any more: stop it.
    Stop(String),
    /// Looks like ours but is not a known collector session: mention it, leave it alone.
    Warn(String),
}

fn same_name(first: &str, second: &str) -> bool {
    first.eq_ignore_ascii_case(second)
}

/// Decides what to do about the other SYNTRAC-looking sessions, in the order they were listed.
///
/// `other_collector_running` means a `syntrac-service` process other than this one exists. While one does, the
/// known collector sessions are presumed to be its own and are left alone.
pub fn plan_orphan_cleanup(own_session: &str, running: &[String], other_collector_running: bool) -> Vec<OrphanAction> {
    running
        .iter()
        .filter(|name| name.to_ascii_lowercase().starts_with(SYNTRAC_PREFIX) && !same_name(name, own_session))
        .filter_map(|name| {
            let is_collector_session = COLLECTOR_SESSIONS.iter().any(|known| same_name(known, name));
            match (is_collector_session, other_collector_running) {
                (true, false) => Some(OrphanAction::Stop(name.clone())),
                (true, true) => None,
                (false, _) => Some(OrphanAction::Warn(name.clone())),
            }
        })
        .collect()
}

/// What the machine can do for the guard. Real implementation: `trace_sessions::WindowsSessions`.
pub trait SessionHost {
    fn list_sessions(&self) -> Result<Vec<String>, String>;
    fn stop_session(&self, name: &str) -> Result<(), String>;
    fn other_collector_running(&self) -> bool;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub stopped: Vec<String>,
    pub warned: Vec<String>,
    /// Session name and why it could not be stopped.
    pub failed: Vec<(String, String)>,
    /// Set when the sessions could not be listed at all.
    pub list_error: Option<String>,
}

/// Never fails: this runs on the way to starting ETW and must not be able to prevent it.
pub fn clean_up_orphans(own_session: &str, host: &dyn SessionHost) -> CleanupReport {
    let mut report = CleanupReport::default();
    let sessions = match host.list_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            warn!(%error, "Could not list trace sessions, so leftover SYNTRAC sessions were not checked for");
            report.list_error = Some(error);
            return report;
        }
    };

    for action in plan_orphan_cleanup(own_session, &sessions, host.other_collector_running()) {
        match action {
            OrphanAction::Stop(name) => match host.stop_session(&name) {
                Ok(()) => {
                    warn!(session = name.as_str(), "Stopped a leftover SYNTRAC trace session (no collector owns it)");
                    report.stopped.push(name);
                }
                Err(error) => {
                    warn!(session = name.as_str(), %error, "Could not stop a leftover SYNTRAC trace session");
                    report.failed.push((name, error));
                }
            },
            OrphanAction::Warn(name) => {
                warn!(
                    session = name.as_str(),
                    "Another SYNTRAC-looking trace session is running and was left alone; if it is stale, \
                     stop it with: logman stop <name> -ets"
                );
                report.warned.push(name);
            }
        }
    }
    if report.stopped.is_empty() && report.warned.is_empty() && report.failed.is_empty() {
        info!("No leftover SYNTRAC trace sessions");
    }
    report
}
