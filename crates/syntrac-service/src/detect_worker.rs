//! The detect worker: every few minutes it asks the detectors what is new or unusual and stores the findings
//! (docs/ALERTS-PLAN.md, F1).
//!
//! Like the launch worker it runs on its OWN thread with its OWN database connection, never inside the collector's loop. A
//! detector that fails is reported and skipped (`syntrac_core::run_detection` decides); a run that cannot be stored is retried at
//! the next interval. Detection never calls the LLM and never touches the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use syntrac_core::findings::ReconcileSettings;
use syntrac_core::{
    run_detection, Clock, DetectionStateRepository, Detector, ExpectationRepository, FindingRepository, RunReport,
    Timestamp,
};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectWorkerSettings {
    /// Between two runs.
    pub interval: Duration,
    pub reconcile: ReconcileSettings,
}

/// What one call to [`DetectWorker::tick`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectTick {
    /// Not time for a run yet.
    NotYet,
    Ran(RunReport),
    /// The run could not be stored; the message is kept. The next run is still scheduled.
    Failed(String),
}

type Detectors = Vec<Box<dyn Detector + Send>>;

pub struct DetectWorker<R, C> {
    repo: R,
    detectors: Detectors,
    clock: C,
    settings: DetectWorkerSettings,
    next_run_at: Timestamp,
}

fn after(base: Timestamp, wait: Duration) -> Timestamp {
    let micros = i64::try_from(wait.as_micros()).unwrap_or(i64::MAX);
    Timestamp::from_micros(base.as_micros().saturating_add(micros))
}

impl<R: FindingRepository + ExpectationRepository + DetectionStateRepository, C: Clock> DetectWorker<R, C> {
    /// The first run is due immediately (and is the moment detection is switched on).
    pub fn new(repo: R, detectors: Detectors, clock: C, settings: DetectWorkerSettings) -> Self {
        let now = clock.now();
        Self { repo, detectors, clock, settings, next_run_at: now }
    }

    /// The repository (for tests).
    pub fn repository(&self) -> &R {
        &self.repo
    }

    /// Runs the detectors if it is time, and schedules the next run.
    pub fn tick(&mut self) -> DetectTick {
        let now = self.clock.now();
        if now < self.next_run_at {
            return DetectTick::NotYet;
        }
        self.next_run_at = after(now, self.settings.interval);
        let detectors: Vec<&dyn Detector> =
            self.detectors.iter().map(|detector| detector.as_ref() as &dyn Detector).collect();
        match run_detection(&detectors, &mut self.repo, now, &self.settings.reconcile) {
            Ok(report) => DetectTick::Ran(report),
            Err(error) => DetectTick::Failed(error.to_string()),
        }
    }

    /// Runs until `stop` is set, looking at the clock every `nap`. Logs what a run found, and every failure.
    pub fn run(mut self, stop: &AtomicBool, nap: Duration) {
        while !stop.load(Ordering::SeqCst) {
            match self.tick() {
                DetectTick::NotYet => {}
                DetectTick::Ran(report) => {
                    for (detector, why) in &report.failed {
                        warn!(detector = detector.as_str(), %why, "A detector could not run this time");
                    }
                    if report.inserted > 0 || report.reopened > 0 {
                        info!(
                            inserted = report.inserted,
                            reopened = report.reopened,
                            suppressed = report.suppressed,
                            overflow = report.overflow,
                            "Detection raised findings"
                        );
                    } else {
                        debug!(
                            candidates = report.candidates,
                            touched = report.touched,
                            suppressed = report.suppressed,
                            "Detection run"
                        );
                    }
                }
                DetectTick::Failed(message) => warn!(%message, "Detection run failed; retrying at the next interval"),
            }
            std::thread::sleep(nap);
        }
    }
}
