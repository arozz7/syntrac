//! The collector host: wires the real Windows sources, the SQLite store and the system clock into the
//! (offline-tested) `Collector` run loop. Shared by console mode and the Windows service, so both run
//! exactly the same code; only how they start, stop and log differs.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_collector::drive_space::DriveFreeSpace;
use syntrac_collector::etw::EtwSource;
use syntrac_collector::etw_stats::{EtwReport, EtwStats};
use syntrac_collector::file_inspector::{InspectorSettings, WindowsFileInspector};
use syntrac_collector::orphan_guard::clean_up_orphans;
use syntrac_collector::snapshot::SnapshotSource;
use syntrac_collector::sysmetrics::SysinfoMetricsSource;
use syntrac_collector::system::{is_elevated, own_cpu_time, SystemClock};
use syntrac_collector::trace_sessions::WindowsSessions;
use syntrac_collector::volumes::system_path_converter;
use syntrac_core::retention::GuardLimits;
use syntrac_core::silence::{SilenceAction, SilencePolicy, SilenceVerdict, SilenceWatchdog};
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::IdentitySettings;
use syntrac_core::{
    Clock, EventRepository, MetricsRepository, MetricsSource, ProcessRepository, Retention, RollupRepository,
    RollupSettings, TelemetrySource,
};
use syntrac_ipc::pipe::{PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_store::SqliteStore;
use tracing::{error, info, warn};

use crate::{
    BoardInfo, Collector, CollectorSettings, IdentityWorker, IdentityWorkerSettings, KnownProductsAdmin, ServiceConfig,
    ServiceHandler, StatusBoard,
};

/// Teardown (stopping sources, final drain, summary) must finish within this long, or `on_stall` runs.
pub const DEFAULT_SHUTDOWN_LIMIT: Duration = Duration::from_secs(20);

/// The status pipe. Optional: if it cannot start (for example the name is already taken) collection carries on
/// without it, because losing telemetry is worse than losing the status endpoint.
#[derive(Debug, Clone)]
pub struct PipeOptions {
    /// Name only, not the full path.
    pub name: String,
    /// Users (SID strings) allowed to connect in addition to SYSTEM, Administrators and this process's own user.
    pub allowed_sids: Vec<String>,
}

pub const SERVICE_PIPE_NAME: &str = "syntrac-collector";
pub const CONSOLE_PIPE_NAME: &str = "syntrac-console";

pub struct HostOptions {
    pub config: ServiceConfig,
    /// Absolute or relative path of the database file; its folder is created if missing.
    pub database_path: PathBuf,
    /// Start the live ETW trace (needs an elevated process). When false only the snapshot and metrics run.
    pub use_etw: bool,
    /// Name of the ETW session. Console and service MUST use different names, because starting a session
    /// clears any existing one with the same name.
    pub etw_session_name: String,
    /// Serve status over a named pipe, or not at all.
    pub pipe: Option<PipeOptions>,
    /// Stop on its own after this long; otherwise run until `stop` is set.
    pub run_for: Option<Duration>,
    pub shutdown_limit: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("ETW needs an elevated process (Run as administrator)")]
    NotElevated,
    #[error("{0}")]
    Failed(String),
}

impl HostError {
    fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

/// What a finished run reports.
pub struct HostOutcome {
    /// Human-readable run summary, ready to print or write to a file.
    pub summary: String,
}

/// Runs the collector until `stop` is set (or `run_for` elapses), then shuts down cleanly.
///
/// `on_stall` is called from a watchdog thread if teardown takes longer than `shutdown_limit`. By then all
/// collected data is already stored, so the caller can end the process (console) or report the service as
/// stopped and then exit (service).
pub fn run_host(
    options: &HostOptions,
    stop: &AtomicBool,
    on_stall: Arc<dyn Fn() + Send + Sync>,
) -> Result<HostOutcome, HostError> {
    let config = &options.config;
    if options.use_etw && !is_elevated() {
        return Err(HostError::NotElevated);
    }

    if let Some(parent) = options.database_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| HostError::failed(format!("could not create {}: {e}", parent.display())))?;
    }
    let mut store = SqliteStore::open(&options.database_path).map_err(|e| {
        HostError::failed(format!("could not open the database {}: {e}", options.database_path.display()))
    })?;
    info!(database = %options.database_path.display(), "Database ready");
    for notice in store.take_notices() {
        warn!(notice = notice.as_str(), "Database");
    }

    // ETW first, so nothing is missed while the snapshot is taken.
    let mut sources: Vec<Box<dyn TelemetrySource>> = Vec::new();
    let etw_stats: Option<Arc<EtwStats>> = if options.use_etw {
        // A leftover SYNTRAC trace session can starve every other session of events, so clear our own leftovers
        // first. This cannot fail the start: problems are logged and reported, never raised.
        clean_up_orphans(&options.etw_session_name, &WindowsSessions);
        let source = EtwSource::start(system_path_converter(), &options.etw_session_name)
            .map_err(|e| HostError::failed(e.to_string()))?;
        let stats = source.stats();
        sources.push(Box::new(source));
        Some(stats)
    } else {
        warn!("Running without ETW: no live process, network or DNS events will be collected");
        None
    };
    sources.push(Box::new(SnapshotSource::new(SystemClock)));

    let metrics: Box<dyn MetricsSource> = Box::new(SysinfoMetricsSource::new(config.top_processes));
    let settings = CollectorSettings {
        flow_window: Duration::from_secs(config.flow_window_secs),
        flow_flush_grace: Duration::from_secs(config.flow_flush_grace_secs),
        metrics_interval: Duration::from_secs(config.metrics_interval_secs),
        retention: Duration::from_secs(u64::from(config.detail_retention_days) * 24 * 3600),
        purge_interval: Duration::from_secs(config.purge_interval_minutes * 60),
        rollup_interval: Duration::from_secs(config.rollup_interval_minutes * 60),
        rollup: RollupSettings::default(),
        guard_interval: Duration::from_secs(60),
    };
    let gb = 1_000_000_000_u64;
    let limits = GuardLimits {
        max_db_bytes: config.max_database_gb.saturating_mul(gb),
        min_free_bytes: config.min_free_space_gb.saturating_mul(gb),
        hard_floor_bytes: config.hard_floor_free_gb.saturating_mul(gb),
    };
    let data_folder =
        options.database_path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let mut collector = Collector::new(settings, sources, Some(metrics), store, SystemClock)
        .with_storage_guard(limits, Box::new(DriveFreeSpace::new(data_folder.to_path_buf())));

    let board = Arc::new(StatusBoard::new(
        BoardInfo {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: std::process::id(),
            started_at: SystemClock.now(),
            database_path: options.database_path.display().to_string(),
            etw_enabled: options.use_etw,
        },
        SystemClock,
    ));
    let pipe = options.pipe.as_ref().and_then(|pipe| start_pipe(pipe, Arc::clone(&board), &options.database_path));

    let worker_stop = Arc::new(AtomicBool::new(false));
    let identity_thread = start_identity_worker(options, &worker_stop);

    let started = Instant::now();
    let cpu_at_start = own_cpu_time();
    let deadline = options.run_for.map(|span| started + span);
    let drain_interval = Duration::from_millis(config.drain_interval_ms);
    let status_interval = Duration::from_secs(config.status_interval_secs);
    let mut last_status = Instant::now();
    let mut cpu_at_last_status = cpu_at_start;

    let mut etw_watchdog = etw_stats.as_ref().map(|_| {
        let policy =
            SilencePolicy { restart_after_secs: config.etw_silent_restart_secs, max_restarts: config.etw_max_restarts };
        SilenceWatchdog::new(policy, SystemClock.now())
    });

    info!("Collecting");
    while !stop.load(Ordering::SeqCst) && deadline.is_none_or(|end| Instant::now() < end) {
        collector.tick();
        let etw_report = etw_stats.as_ref().map(|s| s.report());
        let etw_silent_secs = match (etw_watchdog.as_mut(), etw_report.as_ref()) {
            (Some(watchdog), Some(report)) => {
                let verdict = watchdog.observe(report.received, SystemClock.now());
                act_on_silence(&mut collector, verdict);
                verdict.silent_secs
            }
            _ => 0,
        };
        board.publish(collector.totals(), etw_report.as_ref(), etw_silent_secs, SystemClock.now());
        board.publish_storage(collector.storage());
        if last_status.elapsed() >= status_interval {
            let cpu_now = own_cpu_time();
            let totals = collector.totals();
            let etw = etw_stats.as_ref().map(|s| s.report());
            info!(
                events_stored = totals.events_stored,
                flows_stored = totals.flows_stored,
                metrics_samples = totals.metrics_samples,
                etw_received = etw.as_ref().map_or(0, |r| r.received),
                etw_dropped = etw.as_ref().map_or(0, |r| r.dropped),
                etw_parse_errors = etw.as_ref().map_or(0, |r| r.parse_errors_total),
                source_errors = totals.source_errors,
                storage_errors = totals.storage_errors,
                own_cpu_percent = format!("{:.2}", cpu_percent(cpu_now - cpu_at_last_status, last_status.elapsed())),
                "Collector status"
            );
            last_status = Instant::now();
            cpu_at_last_status = cpu_now;
        }
        std::thread::sleep(drain_interval);
    }

    drop(pipe); // stop answering status requests first; they are not worth delaying shutdown for
    info!("Stopping: stopping sources, draining them and flushing open flow windows");
    spawn_shutdown_watchdog(on_stall, options.shutdown_limit);
    worker_stop.store(true, Ordering::SeqCst);
    collector.shutdown();
    if let Some(thread) = identity_thread {
        let _ = thread.join();
    }
    let summary = render_summary(&collector, etw_stats.as_deref(), started.elapsed(), own_cpu_time() - cpu_at_start);
    Ok(HostOutcome { summary })
}

/// Starts the identity worker on its own thread with its own database connection, so reading files can never delay the
/// collector. If it cannot start, collection carries on without it: identities are evidence, not the telemetry itself.
fn start_identity_worker(options: &HostOptions, stop: &Arc<AtomicBool>) -> Option<std::thread::JoinHandle<()>> {
    let config = &options.config;
    let store = match SqliteStore::open(&options.database_path) {
        Ok(store) => store,
        Err(error) => {
            warn!(%error, "The identity worker could not open the database; collecting without it");
            return None;
        }
    };
    let inspector = WindowsFileInspector::new(
        InspectorSettings { max_hash_bytes: config.identity_max_hash_mb.saturating_mul(1024 * 1024) },
        Arc::clone(stop),
    );
    let settings = IdentityWorkerSettings {
        interval: Duration::from_secs(config.identity_interval_minutes * 60),
        backlog_pause: Duration::from_secs(config.identity_backlog_pause_secs),
        pass: IdentitySettings { max_per_pass: config.identity_files_per_pass, ..IdentitySettings::default() },
    };
    let worker = IdentityWorker::new(store, inspector, SystemClock, settings);
    let stop = Arc::clone(stop);
    let spawned = std::thread::Builder::new()
        .name("syntrac-identity".into())
        .spawn(move || worker.run(&stop, Duration::from_millis(500)));
    match spawned {
        Ok(handle) => {
            info!("Identity worker started");
            Some(handle)
        }
        Err(error) => {
            warn!(%error, "The identity worker thread could not start; collecting without it");
            None
        }
    }
}

/// Name of the ETW source, as `EtwSource` reports it.
const ETW_SOURCE_NAME: &str = "etw";

/// Carries out the watchdog's decision. A trace that is "started" but silent records nothing while every other
/// number still looks healthy, so each step is logged loudly.
fn act_on_silence<R, C>(collector: &mut Collector<R, C>, verdict: SilenceVerdict)
where
    R: ProcessRepository + EventRepository + MetricsRepository + Retention + RollupRepository + StorageStats,
    C: Clock,
{
    match verdict.action {
        SilenceAction::None => {}
        SilenceAction::Restart { attempt } => {
            warn!(
                silent_secs = verdict.silent_secs,
                attempt, "ETW has delivered no events for too long; restarting the trace session"
            );
            if let Err(error) = collector.restart_source(ETW_SOURCE_NAME) {
                error!(%error, attempt, "Restarting the ETW trace session failed");
            }
        }
        SilenceAction::GiveUp => error!(
            silent_secs = verdict.silent_secs,
            "ETW is still silent after restarting; process and network activity is NOT being recorded"
        ),
    }
}

/// The status board as the pipe's read-only handler.
struct BoardHandler(Arc<StatusBoard<SystemClock>>);

impl Handler for BoardHandler {
    fn handle(&self, request: Request) -> Response {
        self.0.handle(request)
    }
}

/// Starts the pipe. Besides status, it answers the write commands of the known-programs list, which need their own database
/// connection; if that cannot be opened the pipe still serves status (the write commands then answer that they are not handled).
fn start_pipe(options: &PipeOptions, board: Arc<StatusBoard<SystemClock>>, database_path: &Path) -> Option<PipeServer> {
    let mut config = PipeServerConfig::new(&options.name);
    config.allowed_sids = options.allowed_sids.clone();
    let status = BoardHandler(board);
    let handler: Arc<dyn Handler> = match SqliteStore::open(database_path) {
        Ok(store) => Arc::new(ServiceHandler::new(status, KnownProductsAdmin::new(store, SystemClock))),
        Err(error) => {
            warn!(%error, "The known-programs list could not open the database; the pipe answers status only");
            Arc::new(status)
        }
    };
    match PipeServer::start(config, handler) {
        Ok(server) => {
            info!(pipe = options.name.as_str(), "Status pipe listening");
            Some(server)
        }
        Err(error) => {
            warn!(pipe = options.name.as_str(), %error, "The status pipe could not start; collecting without it");
            None
        }
    }
}

/// Calls `on_stall` if teardown takes longer than `limit`. A detached thread: when shutdown finishes
/// normally the process exits and this thread dies with it.
fn spawn_shutdown_watchdog(on_stall: Arc<dyn Fn() + Send + Sync>, limit: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(limit);
        error!(limit_secs = limit.as_secs(), "Shutdown did not finish in time");
        on_stall();
    });
}

/// CPU used by this process as a share of the whole machine.
fn cpu_percent(cpu_used: Duration, wall: Duration) -> f64 {
    let cores = std::thread::available_parallelism().map_or(1, usize::from) as f64;
    if wall.is_zero() {
        return 0.0;
    }
    cpu_used.as_secs_f64() / (wall.as_secs_f64() * cores) * 100.0
}

fn render_etw_report(out: &mut String, report: &EtwReport) {
    let _ = writeln!(out, "ETW events received:   {}", report.received);
    let _ = writeln!(out, "ETW events DROPPED:    {}", report.dropped);
    let _ = writeln!(out, "ETW parse errors:      {}", report.parse_errors_total);
    for error in &report.parse_errors {
        let _ = writeln!(out, "  {:<8} event {:<5} {}", error.provider, error.event_id, error.count);
    }
    let _ = writeln!(out, "ETW max delivery lag:  {} ms (event time vs wall clock)", report.max_lag_ms);
    let _ = writeln!(
        out,
        "ETW events in the future: {} (most extreme {} ms ahead); many would mean ETW timestamps are not system time",
        report.future_events, report.max_ahead_ms
    );
}

fn render_summary(
    collector: &Collector<SqliteStore, SystemClock>,
    etw: Option<&EtwStats>,
    elapsed: Duration,
    cpu_used: Duration,
) -> String {
    let totals = collector.totals();
    let mut out = String::new();
    let _ = writeln!(out, "\n=== SYNTRAC run summary ===");
    let _ = writeln!(out, "ran for:               {:.1}s", elapsed.as_secs_f64());
    let _ = writeln!(
        out,
        "collector CPU (self):  {:.3}s = {:.2}% of the whole machine (average)",
        cpu_used.as_secs_f64(),
        cpu_percent(cpu_used, elapsed)
    );
    let _ = writeln!(out, "ticks:                 {}", totals.ticks);
    let _ = writeln!(out, "events stored:         {}", totals.events_stored);
    let _ = writeln!(out, "flow windows stored:   {}", totals.flows_stored);
    let _ = writeln!(out, "metrics samples:       {}", totals.metrics_samples);
    let _ = writeln!(out, "source errors:         {}", totals.source_errors);
    let _ = writeln!(out, "metrics errors:        {}", totals.metrics_errors);
    let _ = writeln!(out, "storage errors:        {}", totals.storage_errors);
    match etw {
        Some(stats) => render_etw_report(&mut out, &stats.report()),
        None => {
            let _ = writeln!(out, "ETW:                   not started");
        }
    }
    match collector.repository().event_counts_by_kind() {
        Ok(counts) => {
            let _ = writeln!(out, "stored events by kind:");
            for (kind, count) in counts {
                let _ = writeln!(out, "  {kind:<18} {count}");
            }
        }
        Err(error) => {
            let _ = writeln!(out, "could not summarise stored events: {error}");
        }
    }
    out
}
