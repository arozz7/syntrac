//! Test support for SYNTRAC: mocks, an in-memory repository, fixtures and contract suites.

pub mod contract;
pub mod fixtures;
pub mod memory;
pub mod mocks;

pub use memory::InMemoryRepository;
pub use mocks::{FailingTelemetrySource, ManualClock, MockMetricsSource, MockTelemetrySource, ScriptedTelemetrySource};
