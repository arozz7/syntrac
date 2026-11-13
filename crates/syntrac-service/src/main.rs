//! SYNTRAC collector, console mode. The composition root: it wires the real Windows sources, the
//! SQLite store and the system clock into the (offline-tested) `Collector` run loop.
//!
//! The Windows service wrapper arrives in Phase 1b; this is the same loop run from a terminal.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_collector::etw::EtwSource;
use syntrac_collector::etw_stats::EtwStats;
use syntrac_collector::snapshot::SnapshotSource;
use syntrac_collector::sysmetrics::SysinfoMetricsSource;
use syntrac_collector::system::{is_elevated, own_cpu_time, SystemClock};
use syntrac_collector::volumes::system_path_converter;
use syntrac_core::{MetricsSource, TelemetrySource};
use syntrac_service::{CliArgs, Collector, CollectorSettings, ServiceConfig, USAGE};
use syntrac_store::SqliteStore;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const EXIT_FAILURE: u8 = 1;
const EXIT_NOT_ELEVATED: u8 = 2;
const EXIT_SHUTDOWN_STALLED: u8 = 3;
/// Teardown (stopping sources, final drain, summary) must finish within this long, or the process is ended.
const SHUTDOWN_WATCHDOG: Duration = Duration::from_secs(20);
const DEFAULT_CONFIG_PATH: &str = "config/service.toml";

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

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            error!(error = %failure.message, "Collector stopped with an error");
            eprintln!("error: {}", failure.message);
            ExitCode::from(failure.exit_code)
        }
    }
}

struct Failure {
    message: String,
    exit_code: u8,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), exit_code: EXIT_FAILURE }
    }
}

fn run(args: &CliArgs) -> Result<(), Failure> {
    let config = load_config(args)?;

    if !args.no_etw && !is_elevated() {
        return Err(Failure {
            message: "ETW needs an elevated terminal (Run as administrator). Use --no-etw to run without it.".into(),
            exit_code: EXIT_NOT_ELEVATED,
        });
    }

    let database_path = args.db.clone().or_else(|| config.database_path.clone()).unwrap_or_else(default_database_path);
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Failure::new(format!("could not create {}: {e}", parent.display())))?;
    }
    let store = SqliteStore::open(&database_path)
        .map_err(|e| Failure::new(format!("could not open the database {}: {e}", database_path.display())))?;
    info!(database = %database_path.display(), "Database ready (folder permissions are inherited; restricted ACLs arrive with the Phase 1b installer)");

    // ETW first, so nothing is missed while the snapshot is taken.
    let mut sources: Vec<Box<dyn TelemetrySource>> = Vec::new();
    let etw_stats: Option<Arc<EtwStats>> = if args.no_etw {
        warn!("Running with --no-etw: no live process, network or DNS events will be collected");
        None
    } else {
        let source = EtwSource::start(system_path_converter()).map_err(|e| Failure::new(e.to_string()))?;
        let stats = source.stats();
        sources.push(Box::new(source));
        Some(stats)
    };
    sources.push(Box::new(SnapshotSource::new(SystemClock)));

    let metrics: Box<dyn MetricsSource> = Box::new(SysinfoMetricsSource::new(config.top_processes));
    let settings = CollectorSettings {
        flow_window: Duration::from_secs(config.flow_window_secs),
        flow_flush_grace: Duration::from_secs(config.flow_flush_grace_secs),
        metrics_interval: Duration::from_secs(config.metrics_interval_secs),
        retention: Duration::from_secs(u64::from(config.retention_days) * 24 * 3600),
        purge_interval: Duration::from_secs(config.purge_interval_minutes * 60),
    };
    let mut collector = Collector::new(settings, sources, Some(metrics), store, SystemClock);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    ctrlc::set_handler(move || stop_flag.store(true, Ordering::SeqCst))
        .map_err(|e| Failure::new(format!("could not install the Ctrl+C handler: {e}")))?;

    let started = Instant::now();
    let cpu_at_start = own_cpu_time();
    let deadline = args.seconds.map(|secs| started + Duration::from_secs(secs));
    let drain_interval = Duration::from_millis(config.drain_interval_ms);
    let status_interval = Duration::from_secs(config.status_interval_secs);
    let mut last_status = Instant::now();
    let mut cpu_at_last_status = cpu_at_start;

    info!("Collecting. Press Ctrl+C to stop.");
    while !stop.load(Ordering::SeqCst) && deadline.is_none_or(|end| Instant::now() < end) {
        collector.tick();

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

    info!("Stopping: stopping sources, draining them and flushing open flow windows");
    // Everything is already in the database by the time teardown starts, so if stopping ever stalls the
    // process is ended outright rather than left running (it once outlived its run).
    spawn_shutdown_watchdog();
    collector.shutdown();
    let summary = render_summary(&collector, etw_stats.as_deref(), started.elapsed(), own_cpu_time() - cpu_at_start);
    println!("{summary}");
    if let Some(path) = &args.summary_file {
        if let Err(error) = std::fs::write(path, &summary) {
            warn!(file = %path.display(), %error, "Could not write the summary file");
        }
    }
    info!("Shutdown complete");
    Ok(())
}

/// Ends the process if teardown takes longer than [`SHUTDOWN_WATCHDOG`]. A detached thread: when shutdown
/// finishes normally the process exits and this thread dies with it.
fn spawn_shutdown_watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(SHUTDOWN_WATCHDOG);
        eprintln!("error: shutdown did not finish within {}s; ending the process", SHUTDOWN_WATCHDOG.as_secs());
        std::process::exit(i32::from(EXIT_SHUTDOWN_STALLED));
    });
}

fn load_config(args: &CliArgs) -> Result<ServiceConfig, Failure> {
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

fn read_config(path: &Path) -> Result<ServiceConfig, Failure> {
    let text =
        std::fs::read_to_string(path).map_err(|e| Failure::new(format!("could not read {}: {e}", path.display())))?;
    let config = ServiceConfig::from_toml_str(&text).map_err(|e| Failure::new(format!("{}: {e}", path.display())))?;
    info!(config = %path.display(), "Configuration loaded");
    Ok(config)
}

fn default_database_path() -> PathBuf {
    std::env::var_os("ProgramData")
        .map_or_else(|| PathBuf::from("data"), PathBuf::from)
        .join("Syntrac")
        .join("syntrac.db")
}

/// CPU used by this process as a share of the whole machine.
fn cpu_percent(cpu_used: Duration, wall: Duration) -> f64 {
    let cores = std::thread::available_parallelism().map_or(1, usize::from) as f64;
    if wall.is_zero() {
        return 0.0;
    }
    cpu_used.as_secs_f64() / (wall.as_secs_f64() * cores) * 100.0
}

fn render_etw_report(out: &mut String, report: &syntrac_collector::etw_stats::EtwReport) {
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
    let _ = writeln!(out, "\n=== SYNTRAC console run summary ===");
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
            let _ = writeln!(out, "ETW:                   not started (--no-etw)");
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
