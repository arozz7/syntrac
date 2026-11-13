//! Telemetry sources and enrichers for SYNTRAC.
//!
//! Pure, unit-tested logic (what raw OS values MEAN) lives in `paths`, `etw_map`, `metrics_build`
//! and `snapshot_map`. Thin adapters that talk to Windows (`system`, `volumes`, `snapshot`, `etw`,
//! `sysmetrics`) only move raw values into those functions.

pub mod etw;
pub mod etw_map;
pub mod etw_stats;
pub mod metrics_build;
pub mod paths;
pub mod snapshot;
pub mod snapshot_map;
pub mod sysmetrics;
pub mod system;
pub mod volumes;
