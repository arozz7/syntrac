//! SYNTRAC collector host: configuration and the run loop. The `syntrac-service` binary wires the
//! real sources into it; tests wire mocks.

mod cli;
mod collector;
mod config;

pub use cli::{CliArgs, CliError, USAGE};
pub use collector::{Collector, CollectorSettings, TickReport, Totals};
pub use config::{ConfigError, ServiceConfig};
