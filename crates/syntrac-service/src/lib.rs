//! SYNTRAC collector host: configuration, the run loop and the shared host. The `syntrac-service` binary wires
//! the real sources into it (console mode or Windows service); tests wire mocks.

mod cli;
mod collector;
mod config;
mod context_worker;
mod db_check;
mod detect_worker;
mod findings_admin;
mod guard;
mod host;
mod identity_worker;
mod known_admin;
mod launch_worker;
mod owner_data;
mod service_paths;
mod status_board;

pub use cli::{CliArgs, CliError, USAGE};
pub use collector::{Collector, CollectorSettings, TickReport, Totals};
pub use config::{ConfigError, ServiceConfig};
pub use context_worker::{ContextTick, ContextWorker, ContextWorkerSettings};
pub use db_check::{database_check_passed, format_database_report, format_version_report};
pub use detect_worker::{DetectTick, DetectWorker, DetectWorkerSettings};
pub use findings_admin::{FindingsAdmin, FindingsControl};
pub use guard::StorageSnapshot;
pub use host::{
    run_host, HostError, HostOptions, HostOutcome, PipeOptions, CONSOLE_PIPE_NAME, DEFAULT_SHUTDOWN_LIMIT,
    SERVICE_PIPE_NAME,
};
pub use identity_worker::{IdentityWorker, IdentityWorkerSettings, WorkerTick};
pub use known_admin::{KnownProductsAdmin, ServiceHandler};
pub use launch_worker::{LaunchTick, LaunchWorker, LaunchWorkerSettings};
pub use owner_data::{OwnerControl, OwnerData};
pub use service_paths::{
    check_data_drives, service_config_path, validate_service_paths, ServicePathError, ServicePaths,
};
pub use status_board::{format_status, BoardInfo, StatusBoard};
