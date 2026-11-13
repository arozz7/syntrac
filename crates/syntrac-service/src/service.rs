//! Windows service entry point. The Service Control Manager launches `syntrac-service --service`.
//!
//! Deliberately thin: it translates SCM controls into the stop flag, reads the installed configuration
//! (beside the `bin` folder, writable by administrators only), sets up file logging, and calls the same
//! `run_host` as console mode. Everything that can be tested without installing a service is tested elsewhere
//! (`service_paths`, the host, the collector loop).

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use syntrac_service::{
    run_host, service_config_path, validate_service_paths, HostOptions, PipeOptions, ServiceConfig,
    DEFAULT_SHUTDOWN_LIMIT, SERVICE_PIPE_NAME,
};
use tracing::{error, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::{define_windows_service, service_dispatcher};

pub const SERVICE_NAME: &str = "Syntrac";
/// Differs from console mode's session name, so a console run never clears the installed service's trace.
const SERVICE_ETW_SESSION: &str = "SyntracCollector";
const LOG_FILE_PREFIX: &str = "syntrac-service";
const LOG_FILES_KEPT: usize = 14;
const START_WAIT_HINT: Duration = Duration::from_secs(30);
/// Longer than the host's shutdown limit, so the SCM does not give up before the watchdog would act.
const STOP_WAIT_HINT: Duration = Duration::from_secs(45);
const ERROR_TIMEOUT: u32 = 1460;
const EXIT_STARTUP_FAILED: u32 = 1;

pub fn run_as_service() -> Result<(), windows_service::Error> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    run_service();
}

fn status(state: ServiceState, wait_hint: Duration, exit_code: ServiceExitCode) -> ServiceStatus {
    let pending = matches!(state, ServiceState::StartPending | ServiceState::StopPending);
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        // Preshutdown gives the service extra time at system shutdown; it is exclusive with plain Shutdown.
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::PRESHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code,
        checkpoint: u32::from(pending),
        wait_hint,
        process_id: None,
    }
}

fn run_service() {
    let stop = Arc::new(AtomicBool::new(false));
    let handle_cell: Arc<OnceLock<ServiceStatusHandle>> = Arc::new(OnceLock::new());

    let handler = {
        let stop = Arc::clone(&stop);
        let handle_cell = Arc::clone(&handle_cell);
        move |control| match control {
            ServiceControl::Stop | ServiceControl::Preshutdown | ServiceControl::Shutdown => {
                stop.store(true, Ordering::SeqCst);
                if let Some(handle) = handle_cell.get() {
                    let _ = handle.set_service_status(status(
                        ServiceState::StopPending,
                        STOP_WAIT_HINT,
                        ServiceExitCode::Win32(0),
                    ));
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };
    let Ok(handle) = service_control_handler::register(SERVICE_NAME, handler) else {
        return; // nothing can be reported without a status handle
    };
    let _ = handle_cell.set(handle);
    let _ = handle.set_service_status(status(ServiceState::StartPending, START_WAIT_HINT, ServiceExitCode::Win32(0)));

    let exit_code = match start_and_run(&stop, handle) {
        Ok(()) => ServiceExitCode::Win32(0),
        Err(message) => {
            write_startup_error(&message);
            ServiceExitCode::ServiceSpecific(EXIT_STARTUP_FAILED)
        }
    };
    let _ = handle.set_service_status(status(ServiceState::Stopped, Duration::ZERO, exit_code));
}

fn start_and_run(stop: &Arc<AtomicBool>, handle: ServiceStatusHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate the executable: {e}"))?;
    let exe_dir = exe.parent().ok_or("the executable has no parent folder")?;

    let config_path = service_config_path(exe_dir);
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("cannot read the service configuration {}: {e}", config_path.display()))?;
    let config = ServiceConfig::from_toml_str(&text).map_err(|e| format!("{}: {e}", config_path.display()))?;
    let paths = validate_service_paths(&config).map_err(|e| format!("{}: {e}", config_path.display()))?;

    let _log_guard = init_logging(&paths.log_dir)?;
    info!(config = %config_path.display(), database = %paths.database_path.display(), "Service starting");

    let pipe = PipeOptions {
        name: SERVICE_PIPE_NAME.to_owned(),
        allowed_sids: config.pipe_allowed_sid.clone().into_iter().collect(),
    };
    let options = HostOptions {
        pipe: Some(pipe),
        config,
        database_path: paths.database_path,
        use_etw: true,
        etw_session_name: SERVICE_ETW_SESSION.to_owned(),
        run_for: None,
        shutdown_limit: DEFAULT_SHUTDOWN_LIMIT,
    };

    // Teardown stalled: report the service as stopped (so the SCM does not treat the exit as a crash that
    // needs an immediate restart), then end the process. All collected data is already stored.
    let on_stall: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = handle.set_service_status(status(
            ServiceState::Stopped,
            Duration::ZERO,
            ServiceExitCode::Win32(ERROR_TIMEOUT),
        ));
        std::process::exit(1);
    });

    let _ = handle.set_service_status(status(ServiceState::Running, Duration::ZERO, ServiceExitCode::Win32(0)));
    match run_host(&options, stop, on_stall) {
        Ok(outcome) => {
            info!("{}", outcome.summary);
            info!("Service stopped");
            Ok(())
        }
        Err(error) => {
            error!(%error, "The collector stopped with an error");
            Err(error.to_string())
        }
    }
}

/// Daily rolling log files with a retention limit. The returned guard must live as long as the process,
/// or buffered log lines are lost.
fn init_logging(log_dir: &Path) -> Result<WorkerGuard, String> {
    std::fs::create_dir_all(log_dir).map_err(|e| format!("cannot create the log folder {}: {e}", log_dir.display()))?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix("log")
        .max_log_files(LOG_FILES_KEPT)
        .build(log_dir)
        .map_err(|e| format!("cannot open the log file in {}: {e}", log_dir.display()))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|e| format!("cannot start logging: {e}"))?;
    Ok(guard)
}

/// Logging may not exist yet when startup fails, so the reason goes in a file beside the executable.
fn write_startup_error(message: &str) {
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        let _ = std::fs::write(dir.join("startup-error.txt"), message);
    }
}
