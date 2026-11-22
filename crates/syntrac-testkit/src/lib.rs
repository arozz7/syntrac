//! Test support for SYNTRAC: mocks, an in-memory repository, fixtures and contract suites.

pub mod context_contract;
pub mod contract;
pub mod destination_contract;
pub mod findings_contract;
pub mod fixtures;
pub mod identity_contract;
pub mod install_contract;
pub mod known_contract;
pub mod launcher_contract;
pub mod memory;
mod memory_context;
mod memory_destinations;
mod memory_findings;
mod memory_identity;
mod memory_installs;
mod memory_known;
mod memory_launchers;
mod memory_owner;
mod memory_rollup;
mod memory_spawns;
mod memory_usage;
pub mod mock_llm;
pub mod mock_process;
pub mod mocks;
pub mod names_contract;
pub mod owner_contract;
pub mod process_window_contract;
pub mod rollup_contract;
pub mod spawn_contract;
pub mod storage_contract;
pub mod trend_contract;
pub mod usage_contract;

pub use memory::InMemoryRepository;
pub use mocks::{
    FailingTelemetrySource, FakeContextSource, FakeDatasetFetcher, FakeInspector, FakeInstallSource, FakeLaunchSource,
    ManualClock, MockMetricsSource, MockTelemetrySource, RestartCounter, RestartableTelemetrySource,
    ScriptedTelemetrySource,
};
