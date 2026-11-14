//! The identity worker: reads the files of executables SYNTRAC has seen (signature, version, hash) and stores what it finds.
//!
//! It runs on its OWN thread with its OWN database connection, never inside the collector's loop, so a slow disk or a large
//! file can never delay draining ETW. It is paced: a short pause between passes while a backlog remains (the first run sees
//! hundreds of executables), a long interval otherwise. The decisions (what is due, in what order) are `syntrac_core`'s
//! `inspect_due`; this type only schedules passes and keeps going after errors.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use syntrac_core::{inspect_due, Clock, FileInspector, IdentityPass, IdentityRepository, IdentitySettings, Timestamp};
use tracing::{error, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityWorkerSettings {
    /// Between two passes when nothing is waiting.
    pub interval: Duration,
    /// Between two passes while executables are still waiting: a gentle pace, not a burst of disk reads.
    pub backlog_pause: Duration,
    pub pass: IdentitySettings,
}

/// What one call to [`IdentityWorker::tick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerTick {
    /// Not time for a pass yet.
    NotYet,
    Passed(IdentityPass),
    /// The pass hit a storage error (the message is kept). The next pass is still scheduled.
    StorageError(String),
}

pub struct IdentityWorker<R, I, C> {
    repo: R,
    inspector: I,
    clock: C,
    settings: IdentityWorkerSettings,
    next_pass_at: Timestamp,
}

fn after(base: Timestamp, wait: Duration) -> Timestamp {
    let micros = i64::try_from(wait.as_micros()).unwrap_or(i64::MAX);
    Timestamp::from_micros(base.as_micros().saturating_add(micros))
}

impl<R: IdentityRepository, I: FileInspector, C: Clock> IdentityWorker<R, I, C> {
    /// The first pass is due immediately.
    pub fn new(repo: R, inspector: I, clock: C, settings: IdentityWorkerSettings) -> Self {
        let now = clock.now();
        Self { repo, inspector, clock, settings, next_pass_at: now }
    }

    /// Runs one pass if it is time, and schedules the next.
    pub fn tick(&mut self) -> WorkerTick {
        let now = self.clock.now();
        if now < self.next_pass_at {
            return WorkerTick::NotYet;
        }
        match inspect_due(&mut self.repo, &self.inspector, now, &self.settings.pass) {
            Ok(pass) => {
                let wait = if pass.backlog { self.settings.backlog_pause } else { self.settings.interval };
                self.next_pass_at = after(now, wait);
                WorkerTick::Passed(pass)
            }
            Err(failure) => {
                self.next_pass_at = after(now, self.settings.interval);
                WorkerTick::StorageError(failure.to_string())
            }
        }
    }

    /// Runs until `stop` is set, looking at the clock every `nap`. Logs every pass that did anything.
    pub fn run(mut self, stop: &AtomicBool, nap: Duration) {
        while !stop.load(Ordering::SeqCst) {
            match self.tick() {
                WorkerTick::NotYet => {}
                WorkerTick::Passed(pass) => {
                    if pass.inspected + pass.unchanged + pass.failed > 0 {
                        info!(
                            inspected = pass.inspected,
                            unchanged = pass.unchanged,
                            failed = pass.failed,
                            backlog = pass.backlog,
                            "Identity pass"
                        );
                    }
                }
                WorkerTick::StorageError(message) => {
                    error!(%message, "Identity pass hit a storage error; retrying later")
                }
            }
            std::thread::sleep(nap);
        }
    }
}
