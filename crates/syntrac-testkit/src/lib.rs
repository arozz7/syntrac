//! Test support for SYNTRAC: mocks, an in-memory repository, fixtures and contract suites.

pub mod contract;
pub mod fixtures;
pub mod memory;
mod memory_rollup;
pub mod mocks;
pub mod rollup_contract;
pub mod storage_contract;

pub use memory::InMemoryRepository;
pub use mocks::{
    FailingTelemetrySource, ManualClock, MockMetricsSource, MockTelemetrySource, RestartCounter,
    RestartableTelemetrySource, ScriptedTelemetrySource,
};
