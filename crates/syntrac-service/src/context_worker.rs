//! The context worker: looks at which programs run as which account (and host which services) and keeps what it finds.
//!
//! It runs on its OWN thread with its OWN database connection, never inside the collector's loop, so it can never delay draining
//! ETW. A pass is one process snapshot and writes only what is new, changed or due for a refresh (`syntrac_core::observe_context`
//! decides; this type only schedules passes and keeps going after errors). Plan: `docs/RUNAS-PLAN.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use syntrac_core::{
    observe_context, Clock, ContextPass, ContextRepository, ContextSettings, ContextTracker, ProcessContextSource,
    Timestamp,
};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextWorkerSettings {
    /// Between two passes.
    pub interval: Duration,
    pub pass: ContextSettings,
}

/// What one call to [`ContextWorker::tick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextTick {
    /// Not time for a pass yet.
    NotYet,
    Passed(ContextPass),
    /// The pass failed (the source or the database); the message is kept. The next pass is still scheduled.
    Failed(String),
}

pub struct ContextWorker<R, S, C> {
    repo: R,
    source: S,
    clock: C,
    settings: ContextWorkerSettings,
    tracker: ContextTracker,
    next_pass_at: Timestamp,
}

fn after(base: Timestamp, wait: Duration) -> Timestamp {
    let micros = i64::try_from(wait.as_micros()).unwrap_or(i64::MAX);
    Timestamp::from_micros(base.as_micros().saturating_add(micros))
}

impl<R: ContextRepository, S: ProcessContextSource, C: Clock> ContextWorker<R, S, C> {
    /// The first pass is due immediately.
    pub fn new(repo: R, source: S, clock: C, settings: ContextWorkerSettings) -> Self {
        let now = clock.now();
        let tracker = ContextTracker::new(settings.pass.refresh_every);
        Self { repo, source, clock, settings, tracker, next_pass_at: now }
    }

    /// Runs one pass if it is time, and schedules the next.
    pub fn tick(&mut self) -> ContextTick {
        let now = self.clock.now();
        if now < self.next_pass_at {
            return ContextTick::NotYet;
        }
        self.next_pass_at = after(now, self.settings.interval);
        match observe_context(&self.source, &mut self.repo, &mut self.tracker, now, &self.settings.pass) {
            Ok(pass) => ContextTick::Passed(pass),
            Err(failure) => ContextTick::Failed(failure.to_string()),
        }
    }

    /// Runs until `stop` is set, looking at the clock every `nap`. Logs a pass that learned something, and every failure.
    pub fn run(mut self, stop: &AtomicBool, nap: Duration) {
        while !stop.load(Ordering::SeqCst) {
            match self.tick() {
                ContextTick::NotYet => {}
                ContextTick::Passed(pass) => {
                    if pass.new_accounts + pass.new_services > 0 {
                        info!(
                            seen = pass.seen,
                            written = pass.written,
                            new_accounts = pass.new_accounts,
                            new_services = pass.new_services,
                            unreadable = pass.unreadable,
                            truncated = pass.truncated,
                            "Context pass learned something new"
                        );
                    } else {
                        debug!(seen = pass.seen, written = pass.written, unreadable = pass.unreadable, "Context pass");
                    }
                }
                ContextTick::Failed(message) => warn!(%message, "Context pass failed; retrying at the next interval"),
            }
            std::thread::sleep(nap);
        }
    }
}
