//! Mock implementations of the tool-layer interfaces.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;

use syntrac_core::{Clock, EventEnvelope, MetricsSample, MetricsSource, SourceError, TelemetrySource, Timestamp};

/// Replays a fixed list of events once, then reports nothing new.
#[derive(Debug)]
pub struct MockTelemetrySource {
    pending: Vec<EventEnvelope>,
}

impl MockTelemetrySource {
    pub fn new(events: Vec<EventEnvelope>) -> Self {
        Self { pending: events }
    }
}

impl TelemetrySource for MockTelemetrySource {
    fn name(&self) -> &str {
        "mock"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Ok(std::mem::take(&mut self.pending))
    }
}

/// Delivers one prepared batch per drain, then nothing. Lets a test model events that ARRIVE LATE
/// (stamped earlier than the previous batch) the way ETW's buffered delivery does.
#[derive(Debug)]
pub struct ScriptedTelemetrySource {
    batches: VecDeque<Vec<EventEnvelope>>,
}

impl ScriptedTelemetrySource {
    pub fn new(batches: Vec<Vec<EventEnvelope>>) -> Self {
        Self { batches: batches.into() }
    }
}

impl TelemetrySource for ScriptedTelemetrySource {
    fn name(&self) -> &str {
        "scripted"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Ok(self.batches.pop_front().unwrap_or_default())
    }
}

/// A source that always fails, to prove one broken source cannot stop the others.
#[derive(Debug, Default)]
pub struct FailingTelemetrySource;

impl TelemetrySource for FailingTelemetrySource {
    fn name(&self) -> &str {
        "failing"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Err(SourceError::Failed("simulated failure".into()))
    }
}

/// Hands out prepared samples one per call, then fails (to exercise error handling).
#[derive(Debug)]
pub struct MockMetricsSource {
    samples: VecDeque<MetricsSample>,
}

impl MockMetricsSource {
    pub fn new(samples: Vec<MetricsSample>) -> Self {
        Self { samples: samples.into() }
    }
}

impl MetricsSource for MockMetricsSource {
    fn name(&self) -> &str {
        "mock-metrics"
    }

    fn sample(&mut self, _now: Timestamp) -> Result<MetricsSample, SourceError> {
        self.samples.pop_front().ok_or_else(|| SourceError::Failed("no more prepared samples".into()))
    }
}

/// A clock that only moves when told to. Clones share the same time, so a test can keep one
/// handle while handing another to the code under test.
#[derive(Debug, Clone)]
pub struct ManualClock {
    micros: Arc<AtomicI64>,
}

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        Self { micros: Arc::new(AtomicI64::new(start.as_micros())) }
    }

    pub fn set(&self, now: Timestamp) {
        self.micros.store(now.as_micros(), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_micros(self.micros.load(Ordering::SeqCst))
    }
}

/// How many times a `RestartableTelemetrySource` was asked to restart. Shared, so a test can read it after the
/// source has been handed to the collector.
#[derive(Debug, Clone, Default)]
pub struct RestartCounter(Arc<AtomicU32>);

impl RestartCounter {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A source with a name of the test's choosing that supports `restart`, for exercising the silence watchdog.
#[derive(Debug)]
pub struct RestartableTelemetrySource {
    name: String,
    restarts: RestartCounter,
    fail: bool,
}

impl RestartableTelemetrySource {
    pub fn new(name: &str) -> (Self, RestartCounter) {
        let restarts = RestartCounter::default();
        (Self { name: name.to_owned(), restarts: restarts.clone(), fail: false }, restarts)
    }

    /// Makes every restart fail, after counting it.
    pub fn fail_restarts(&mut self) {
        self.fail = true;
    }
}

impl TelemetrySource for RestartableTelemetrySource {
    fn name(&self) -> &str {
        &self.name
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Ok(Vec::new())
    }

    fn restart(&mut self) -> Result<(), SourceError> {
        self.restarts.0.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(SourceError::Failed("simulated restart failure".into()))
        } else {
            Ok(())
        }
    }
}
