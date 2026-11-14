//! SYNTRAC collector host: configuration, the run loop and the shared host. The `syntrac-service` binary wires
//! the real sources into it (console mode or Windows service); tests wire mocks.

mod cli;
mod collector;
mod config;
mod db_check;
mod guard;
mod host;
mod identity_worker;
mod known_admin;
mod service_paths;
mod status_board;

pub use cli::{CliArgs, CliError, USAGE};
pub use collector::{Collector, CollectorSettings, TickReport, Totals};
pub use config::{ConfigError, ServiceConfig};
pub use db_check::{database_check_passed, format_database_report};
pub use guard::StorageSnapshot;
pub use host::{
    run_host, HostError, HostOptions, HostOutcome, PipeOptions, CONSOLE_PIPE_NAME, DEFAULT_SHUTDOWN_LIMIT,
    SERVICE_PIPE_NAME,
};
pub use identity_worker::{IdentityWorker, IdentityWorkerSettings, WorkerTick};
pub use known_admin::{KnownProductsAdmin, ServiceHandler};
pub use service_paths::{
    check_data_drives, service_config_path, validate_service_paths, ServicePathError, ServicePaths,
};
pub use status_board::{format_status, BoardInfo, StatusBoard};
