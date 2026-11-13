//! What the collector publishes about itself, and the pipe handler that serves it.
//!
//! The run loop calls `publish` after each pass (a cheap write lock); the pipe's connection threads call `handle`
//! (a read lock). Nothing here touches the database or the collector, so a slow client can never slow collection.

use std::sync::RwLock;

use syntrac_collector::etw_stats::EtwReport;
use syntrac_core::{Clock, Timestamp};
use syntrac_ipc::protocol::{CollectorTotals, EtwStatus, Request, Response, StatusReport};
use syntrac_ipc::serve::Handler;

use crate::Totals;

/// Facts that never change while the collector runs.
pub struct BoardInfo {
    pub version: String,
    pub pid: u32,
    pub started_at: Timestamp,
    pub database_path: String,
    pub etw_enabled: bool,
}

#[derive(Default)]
struct Latest {
    totals: CollectorTotals,
    etw: Option<EtwStatus>,
    last_tick: Option<Timestamp>,
}

pub struct StatusBoard<C> {
    info: BoardInfo,
    clock: C,
    latest: RwLock<Latest>,
}

impl<C: Clock> StatusBoard<C> {
    pub fn new(info: BoardInfo, clock: C) -> Self {
        Self { info, clock, latest: RwLock::new(Latest::default()) }
    }

    /// Records the state after a collector pass. A poisoned lock is ignored: status is best-effort.
    pub fn publish(&self, totals: Totals, etw: Option<&EtwReport>, at: Timestamp) {
        if let Ok(mut latest) = self.latest.write() {
            latest.totals = CollectorTotals {
                ticks: totals.ticks,
                events_stored: totals.events_stored,
                flows_stored: totals.flows_stored,
                metrics_samples: totals.metrics_samples,
                source_errors: totals.source_errors,
                metrics_errors: totals.metrics_errors,
                storage_errors: totals.storage_errors,
            };
            latest.etw = etw.map(|report| EtwStatus {
                received: report.received,
                dropped: report.dropped,
                parse_errors: report.parse_errors_total,
                max_lag_ms: report.max_lag_ms,
                future_events: report.future_events,
            });
            latest.last_tick = Some(at);
        }
    }

    fn report(&self) -> StatusReport {
        let now = self.clock.now();
        let uptime_micros = now.as_micros().saturating_sub(self.info.started_at.as_micros()).max(0);
        let (totals, etw, last_tick) = match self.latest.read() {
            Ok(latest) => (latest.totals, latest.etw, latest.last_tick),
            Err(_) => (CollectorTotals::default(), None, None),
        };
        StatusReport {
            version: self.info.version.clone(),
            pid: self.info.pid,
            started_at_micros: self.info.started_at.as_micros(),
            uptime_secs: u64::try_from(uptime_micros / 1_000_000).unwrap_or(0),
            database_path: self.info.database_path.clone(),
            etw_enabled: self.info.etw_enabled,
            last_tick_at_micros: last_tick.map(Timestamp::as_micros),
            totals,
            etw,
        }
    }
}

impl<C: Clock + Send + Sync> Handler for StatusBoard<C> {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: self.info.version.clone() },
            Request::Status => Response::Status(self.report()),
        }
    }
}

/// A short human-readable rendering for `syntrac-service --status`.
pub fn format_status(report: &StatusReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "SYNTRAC collector {} (pid {})", report.version, report.pid);
    let _ = writeln!(out, "uptime:           {}s", report.uptime_secs);
    let _ = writeln!(out, "database:         {}", report.database_path);
    let _ = writeln!(
        out,
        "stored:           {} events, {} flow windows, {} metrics samples",
        report.totals.events_stored, report.totals.flows_stored, report.totals.metrics_samples
    );
    let _ = writeln!(
        out,
        "errors:           {} source, {} metrics, {} storage",
        report.totals.source_errors, report.totals.metrics_errors, report.totals.storage_errors
    );
    match &report.etw {
        Some(etw) => {
            let _ = writeln!(
                out,
                "ETW:              {} received, {} DROPPED, {} parse errors, max lag {} ms",
                etw.received, etw.dropped, etw.parse_errors, etw.max_lag_ms
            );
        }
        None => {
            let _ = writeln!(out, "ETW:              {}", if report.etw_enabled { "starting" } else { "not running" });
        }
    }
    out
}
