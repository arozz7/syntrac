//! What the collector publishes about itself, and the pipe handler that serves it.
//!
//! The run loop calls `publish` after each pass (a cheap write lock); the pipe's connection threads call `handle`
//! (a read lock). Nothing here touches the database or the collector, so a slow client can never slow collection.

use std::sync::RwLock;

use syntrac_collector::etw_stats::EtwReport;
use syntrac_core::retention::GuardLevel;
use syntrac_core::{Clock, Timestamp};
use syntrac_ipc::protocol::{CollectorTotals, EtwStatus, Request, Response, StatusReport, StorageLevel, StorageStatus};
use syntrac_ipc::serve::Handler;

use crate::{StorageSnapshot, Totals};

/// Quiet for this long is worth saying out loud in the text status. (A few quiet seconds are normal.)
pub const ETW_SILENT_WARNING_SECS: u64 = 60;

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
    storage: Option<StorageStatus>,
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
    pub fn publish(&self, totals: Totals, etw: Option<&EtwReport>, etw_silent_secs: u64, at: Timestamp) {
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
                silent_secs: etw_silent_secs,
            });
            latest.last_tick = Some(at);
        }
    }

    /// Records what the storage guard last measured (`None` clears it). Separate from `publish` because the guard checks once
    /// a minute, not once a pass.
    pub fn publish_storage(&self, snapshot: Option<StorageSnapshot>) {
        if let Ok(mut latest) = self.latest.write() {
            latest.storage = snapshot.map(|s| StorageStatus {
                level: match s.level {
                    GuardLevel::Ok => StorageLevel::Ok,
                    GuardLevel::Trimming => StorageLevel::Trimming,
                    GuardLevel::Paused => StorageLevel::Paused,
                },
                database_file_bytes: s.usage.file_bytes,
                database_live_bytes: s.usage.live_bytes,
                drive_free_bytes: s.drive_free_bytes,
                oldest_detail_micros: s.usage.oldest_detail.map(Timestamp::as_micros),
                oldest_rollup_micros: s.usage.oldest_rollup.map(Timestamp::as_micros),
                trimmed_hours_total: s.trimmed_steps_total,
            });
        }
    }

    fn report(&self) -> StatusReport {
        let now = self.clock.now();
        let uptime_micros = now.as_micros().saturating_sub(self.info.started_at.as_micros()).max(0);
        let (totals, etw, last_tick, storage) = match self.latest.read() {
            Ok(latest) => (latest.totals, latest.etw, latest.last_tick, latest.storage),
            Err(_) => (CollectorTotals::default(), None, None, None),
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
            storage,
        }
    }
}

impl<C: Clock + Send + Sync> Handler for StatusBoard<C> {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: self.info.version.clone() },
            Request::Status => Response::Status(self.report()),
            // The status board only reports; the write commands are answered by the service's own handler, which wraps it.
            Request::KnownProductAdd { .. } | Request::KnownProductRemove { .. } => {
                Response::Error { message: "this endpoint only answers status requests".into() }
            }
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
    if let Some(storage) = &report.storage {
        let _ = writeln!(
            out,
            "database size:    {} live ({} file), drive free {}",
            gigabytes(storage.database_live_bytes),
            gigabytes(storage.database_file_bytes),
            gigabytes(storage.drive_free_bytes)
        );
        match storage.level {
            StorageLevel::Ok => {}
            StorageLevel::Trimming => {
                let _ = writeln!(
                    out,
                    "                  storage guard is TRIMMING the oldest rolled-up detail ({} hours so far)",
                    storage.trimmed_hours_total
                );
            }
            StorageLevel::Paused => {
                let _ = writeln!(
                    out,
                    "                  detail collection is PAUSED: the data drive is nearly full (rollups and status continue)"
                );
            }
        }
    }
    match &report.etw {
        Some(etw) => {
            let _ = writeln!(
                out,
                "ETW:              {} received, {} DROPPED, {} parse errors, max lag {} ms",
                etw.received, etw.dropped, etw.parse_errors, etw.max_lag_ms
            );
            if etw.silent_secs >= ETW_SILENT_WARNING_SECS {
                let _ = writeln!(
                    out,
                    "                  ETW has been SILENT for {} s: no process or network activity is being recorded",
                    etw.silent_secs
                );
            }
        }
        None => {
            let _ = writeln!(out, "ETW:              {}", if report.etw_enabled { "starting" } else { "not running" });
        }
    }
    out
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
}
