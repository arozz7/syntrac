//! SYNTRAC collector host: configuration, the run loop and the shared host. The `syntrac-service` binary wires
//! the real sources into it (console mode or Windows service); tests wire mocks.

mod cli;
mod collector;
mod config;
mod host;
mod service_paths;

pub use cli::{CliArgs, CliError, USAGE};
pub use collector::{Collector, CollectorSettings, TickReport, Totals};
pub use config::{ConfigError, ServiceConfig};
pub use host::{run_host, HostError, HostOptions, HostOutcome, DEFAULT_SHUTDOWN_LIMIT};
pub use service_paths::{service_config_path, validate_service_paths, ServicePathError, ServicePaths};
