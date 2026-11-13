//! SYNTRAC collector, console mode: runs the shared host from a terminal (Ctrl+C to stop).
//! The installed Windows service runs the same host; see `service.rs`.

mod service;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use std::time::Duration;

use syntrac_ipc::pipe::{request, ClientError};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_service::{
    format_status, run_host, CliArgs, HostError, HostOptions, PipeOptions, ServiceConfig, CONSOLE_PIPE_NAME,
    DEFAULT_SHUTDOWN_LIMIT, SERVICE_PIPE_NAME, USAGE,
};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const EXIT_FAILURE: u8 = 1;
const EXIT_NOT_ELEVATED: u8 = 2;
const EXIT_SHUTDOWN_STALLED: i32 = 3;
/// `--status` / `--ping` exit codes, so scripts can tell the cases apart.
const EXIT_NOT_RUNNING: u8 = 4;
const EXIT_ACCESS_DENIED: u8 = 5;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_CONFIG_PATH: &str = "config/service.toml";
/// Console runs keep their own database, separate from the installed service's.
const DEFAULT_CONSOLE_DATABASE: &str = "data/syntrac-console.db";
/// Console and service must not share an ETW session name (starting one clears the other's).
const CONSOLE_ETW_SESSION: &str = "SyntracConsole";

fn main() -> ExitCode {
    let args = match CliArgs::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    if args.help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.status || args.ping {
        return run_client(&args);
    }
    if args.service {
        // The Service Control Manager launched us: it sets up its own logging and reports its own status.
        return match service::run_as_service() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: not started by the Service Control Manager: {error}");
                ExitCode::from(EXIT_FAILURE)
            }
        };
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(HostError::NotElevated) => {
            eprintln!("error: ETW needs an elevated terminal (Run as administrator). Use --no-etw to run without it.");
            ExitCode::from(EXIT_NOT_ELEVATED)
        }
        Err(HostError::Failed(message)) => {
            error!(error = %message, "Collector stopped with an error");
            eprintln!("error: {message}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// `--status` / `--ping`: ask a running collector over its pipe. Used by people and by the smoke test, which runs it
/// as an ordinary user to prove the pipe''s access list lets that user in.
fn run_client(args: &CliArgs) -> ExitCode {
    let pipe = args.pipe.as_deref().unwrap_or(SERVICE_PIPE_NAME);
    let wanted = if args.status { Request::Status } else { Request::Ping };
    match request(pipe, &wanted, CLIENT_TIMEOUT) {
        Ok(Response::Status(report)) => {
            print!("{}", format_status(&report));
            ExitCode::SUCCESS
        }
        Ok(Response::Pong { version }) => {
            println!("pong (collector {version})");
            ExitCode::SUCCESS
        }
        Ok(Response::Error { message }) => {
            eprintln!("error: the collector refused the request: {message}");
            ExitCode::from(EXIT_FAILURE)
        }
        Err(ClientError::NotRunning) => {
            eprintln!("The collector is not running (no pipe named {pipe}).");
            ExitCode::from(EXIT_NOT_RUNNING)
        }
        Err(ClientError::AccessDenied) => {
            eprintln!("Access to the collector (pipe {pipe}) was denied for this user.");
            ExitCode::from(EXIT_ACCESS_DENIED)
        }
        Err(other) => {
            eprintln!("error: {other}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

fn run(args: &CliArgs) -> Result<(), HostError> {
    let config = load_config(args)?;
    let options = HostOptions {
        database_path: args
            .db
            .clone()
            .or_else(|| config.database_path.clone())
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONSOLE_DATABASE)),
        use_etw: !args.no_etw,
        etw_session_name: CONSOLE_ETW_SESSION.to_owned(),
        pipe: Some(PipeOptions { name: CONSOLE_PIPE_NAME.to_owned(), allowed_sids: Vec::new() }),
        run_for: args.seconds.map(std::time::Duration::from_secs),
        shutdown_limit: DEFAULT_SHUTDOWN_LIMIT,
        config,
    };

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    ctrlc::set_handler(move || stop_flag.store(true, Ordering::SeqCst))
        .map_err(|e| HostError::Failed(format!("could not install the Ctrl+C handler: {e}")))?;

    // Everything is already stored when teardown stalls, so end the process rather than leave it running.
    let on_stall: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        eprintln!("error: shutdown did not finish in time; ending the process");
        std::process::exit(EXIT_SHUTDOWN_STALLED);
    });

    info!("Press Ctrl+C to stop");
    let outcome = run_host(&options, &stop, on_stall)?;

    println!("{}", outcome.summary);
    if let Some(path) = &args.summary_file {
        if let Err(error) = std::fs::write(path, &outcome.summary) {
            warn!(file = %path.display(), %error, "Could not write the summary file");
        }
    }
    info!("Shutdown complete");
    Ok(())
}

fn load_config(args: &CliArgs) -> Result<ServiceConfig, HostError> {
    let path = match &args.config {
        Some(path) => Some(path.clone()),
        None => Some(PathBuf::from(DEFAULT_CONFIG_PATH)).filter(|p| p.exists()),
    };
    let Some(path) = path else {
        info!("No configuration file found; using built-in defaults");
        return Ok(ServiceConfig::default());
    };
    read_config(&path)
}

fn read_config(path: &Path) -> Result<ServiceConfig, HostError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| HostError::Failed(format!("could not read {}: {e}", path.display())))?;
    let config =
        ServiceConfig::from_toml_str(&text).map_err(|e| HostError::Failed(format!("{}: {e}", path.display())))?;
    info!(config = %path.display(), "Configuration loaded");
    Ok(config)
}
