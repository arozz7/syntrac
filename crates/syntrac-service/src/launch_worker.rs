//! The launch-record worker: every few minutes it reads the places that say what starts (services, scheduled tasks, Run keys,
//! Startup folders) and keeps what it finds (`docs/RUNAS-PLAN.md`, I4b).
//!
//! It runs on its OWN thread with its OWN database connection, never inside the collector's loop. A sweep is a few hundred small
//! reads and one transaction; a source that fails is reported and skipped (`syntrac_core::sweep_launchers` decides), and a
//! failed sweep is retried at the next interval.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use syntrac_core::{sweep_launchers, Clock, LaunchSource, LauncherRepository, SweepReport, Timestamp};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchWorkerSettings {
    /// Between two sweeps.
    pub interval: Duration,
}

/// What one call to [`LaunchWorker::tick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTick {
    /// Not time for a sweep yet.
    NotYet,
    Swept(SweepReport),
    /// The sweep could not be stored; the message is kept. The next sweep is still scheduled.
    Failed(String),
}

type Sources = Vec<Box<dyn LaunchSource + Send>>;

pub struct LaunchWorker<R, C> {
    repo: R,
    sources: Sources,
    env: fn(&str) -> Option<String>,
    exists: fn(&str) -> bool,
    clock: C,
    settings: LaunchWorkerSettings,
    next_sweep_at: Timestamp,
}

fn after(base: Timestamp, wait: Duration) -> Timestamp {
    let micros = i64::try_from(wait.as_micros()).unwrap_or(i64::MAX);
    Timestamp::from_micros(base.as_micros().saturating_add(micros))
}

impl<R: LauncherRepository, C: Clock> LaunchWorker<R, C> {
    /// The first sweep is due immediately.
    pub fn new(
        repo: R,
        sources: Sources,
        env: fn(&str) -> Option<String>,
        exists: fn(&str) -> bool,
        clock: C,
        settings: LaunchWorkerSettings,
    ) -> Self {
        let now = clock.now();
        Self { repo, sources, env, exists, clock, settings, next_sweep_at: now }
    }

    /// Runs one sweep if it is time, and schedules the next.
    pub fn tick(&mut self) -> LaunchTick {
        let now = self.clock.now();
        if now < self.next_sweep_at {
            return LaunchTick::NotYet;
        }
        self.next_sweep_at = after(now, self.settings.interval);
        let sources: Vec<&dyn LaunchSource> =
            self.sources.iter().map(|source| source.as_ref() as &dyn LaunchSource).collect();
        match sweep_launchers(&sources, &self.env, &self.exists, &mut self.repo, now) {
            Ok(report) => LaunchTick::Swept(report),
            Err(error) => LaunchTick::Failed(error.to_string()),
        }
    }

    /// Runs until `stop` is set, looking at the clock every `nap`. Logs what a sweep learned, and every failure.
    pub fn run(mut self, stop: &AtomicBool, nap: Duration) {
        while !stop.load(Ordering::SeqCst) {
            match self.tick() {
                LaunchTick::NotYet => {}
                LaunchTick::Swept(report) => {
                    for (source, why) in &report.failed {
                        warn!(source = source.as_str(), %why, "A place that says what starts could not be read this sweep");
                    }
                    if report.new > 0 {
                        info!(
                            entries = report.entries,
                            recorded = report.recorded,
                            new = report.new,
                            unresolved = report.unresolved,
                            missing_files = report.missing_files,
                            unreadable = report.unreadable,
                            "Launch sweep found entries not seen before"
                        );
                    } else {
                        debug!(
                            entries = report.entries,
                            recorded = report.recorded,
                            unresolved = report.unresolved,
                            "Launch sweep"
                        );
                    }
                }
                LaunchTick::Failed(message) => warn!(%message, "Launch sweep failed; retrying at the next interval"),
            }
            std::thread::sleep(nap);
        }
    }
}
