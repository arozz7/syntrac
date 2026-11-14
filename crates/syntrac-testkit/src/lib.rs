//! Test support for SYNTRAC: mocks, an in-memory repository, fixtures and contract suites.

pub mod contract;
pub mod fixtures;
pub mod identity_contract;
pub mod known_contract;
pub mod memory;
mod memory_identity;
mod memory_known;
mod memory_rollup;
pub mod mocks;
pub mod rollup_contract;
pub mod storage_contract;
pub mod trend_contract;

pub use memory::InMemoryRepository;
pub use mocks::{
    FailingTelemetrySource, FakeInspector, ManualClock, MockMetricsSource, MockTelemetrySource, RestartCounter,
    RestartableTelemetrySource, ScriptedTelemetrySource,
};
