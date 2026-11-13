//! Mock implementations of the tool-layer interfaces.

use std::cell::Cell;

use syntrac_core::{Clock, EventEnvelope, SourceError, TelemetrySource, Timestamp};

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

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct ManualClock {
    now: Cell<Timestamp>,
}

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        Self { now: Cell::new(start) }
    }

    pub fn set(&self, now: Timestamp) {
        self.now.set(now);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        self.now.get()
    }
}
