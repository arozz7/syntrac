//! `demo-collector <database> <now-seconds> [pipe-name]`: a stand-in for the collector, only for the demo (docs/PUBLIC-PLAN.md, P1).
//!
//! It does what the app needs from a running collector and nothing more: it answers the status request on the pipe (so the app shows
//! "running" instead of "offline"), refuses every request that would change something, and adds a fresh sample of the fictional machine
//! every five seconds (so the data is never "stale" and the Overview moves). `now-seconds` must be the one the database was made with.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use syntrac_core::{roll_up_due, MetricsRepository, ProcessRecord, ProcessRepository, RollupSettings, Timestamp};
use syntrac_demo::world::{sample_at, Moment, PROGRAMS};
use syntrac_ipc::pipe::{PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{
    CollectorTotals, EtwStatus, OwnerDataReport, OwnerDatasetReport, Request, Response, StatusReport, StorageLevel,
    StorageStatus,
};
use syntrac_ipc::serve::Handler;
use syntrac_store::SqliteStore;

const DEFAULT_PIPE: &str = "syntrac-demo";
const FEED_EVERY: Duration = Duration::from_secs(5);

fn wall_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0))
}

struct Demo {
    database: PathBuf,
    started: Instant,
    started_micros: i64,
    oldest_micros: i64,
    owner_data_micros: i64,
}

impl Handler for Demo {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "demo".to_owned() },
            Request::Status => Response::Status(self.report()),
            // The demo has an invented owner dataset and never downloads anything.
            Request::OwnerDataStatus => Response::OwnerData(OwnerDataReport {
                dataset: Some(OwnerDatasetReport {
                    source: "demo data (invented networks)".to_owned(),
                    downloaded_at_micros: self.owner_data_micros,
                    rows: 6,
                }),
                refreshing: false,
                last_error: None,
                allowed: true,
            }),
            _ => Response::Error { message: "This is the demo: nothing can be changed here.".to_owned() },
        }
    }
}

impl Demo {
    fn report(&self) -> StatusReport {
        let size = std::fs::metadata(&self.database).map_or(0, |meta| meta.len());
        StatusReport {
            version: "demo".to_owned(),
            pid: std::process::id(),
            started_at_micros: self.started_micros,
            uptime_secs: self.started.elapsed().as_secs(),
            database_path: self.database.display().to_string(),
            etw_enabled: true,
            last_tick_at_micros: Some(wall_secs() * 1_000_000),
            totals: CollectorTotals { ticks: self.started.elapsed().as_secs(), ..CollectorTotals::default() },
            etw: Some(EtwStatus {
                received: 1_000_000,
                dropped: 0,
                parse_errors: 0,
                max_lag_ms: 3,
                future_events: 0,
                silent_secs: 0,
            }),
            storage: Some(StorageStatus {
                level: StorageLevel::Ok,
                database_file_bytes: size,
                database_live_bytes: size,
                drive_free_bytes: 400 * 1024 * 1024 * 1024,
                oldest_detail_micros: Some(self.oldest_micros),
                oldest_rollup_micros: Some(self.oldest_micros),
                trimmed_hours_total: 0,
            }),
        }
    }
}

/// Adds the processes of the sample that the database does not know yet (a new day brings new processes), then the sample itself.
fn feed(store: &mut SqliteStore, moment: &Moment, known: &mut HashSet<syntrac_core::ProcessInstanceId>, ts: i64) {
    let sample = sample_at(moment, ts, FEED_EVERY.as_secs() as i64);
    for (metric, program) in
        sample.processes.iter().zip(PROGRAMS.iter().filter(|program| syntrac_demo::world::is_busy(program, moment, ts)))
    {
        let Some(id) = metric.instance else { continue };
        if known.insert(id) && store.get(id).ok().flatten().is_none() {
            let record = ProcessRecord {
                id,
                pid: metric.pid,
                parent_pid: 4,
                parent_id: None,
                image_path: program.path.to_owned(),
                command_line: None,
                started: Timestamp::from_micros(ts * 1_000_000),
                ended: None,
                ended_approximate: false,
                exit_code: None,
            };
            let _ = store.record_start(record);
        }
    }
    let _ = store.append_metrics(&sample);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(database), Some(now)) =
        (args.first().map(PathBuf::from), args.get(1).and_then(|text| text.parse::<i64>().ok()))
    else {
        println!("usage: demo-collector <database> <now-seconds> [pipe-name]");
        std::process::exit(2);
    };
    let pipe = args.get(2).map_or(DEFAULT_PIPE, String::as_str).to_owned();
    let moment = Moment::new(now);

    let mut store = match SqliteStore::open(Path::new(&database)) {
        Ok(store) => store,
        Err(error) => {
            println!("Could not open {}: {error}", database.display());
            std::process::exit(1);
        }
    };
    let demo = Arc::new(Demo {
        database: database.clone(),
        started: Instant::now(),
        started_micros: wall_secs() * 1_000_000,
        oldest_micros: moment.history_start() * 1_000_000,
        owner_data_micros: (now - 3 * 86_400) * 1_000_000,
    });
    let _server = match PipeServer::start(PipeServerConfig::new(&pipe), demo) {
        Ok(server) => server,
        Err(error) => {
            println!("Could not start the pipe {pipe}: {error}");
            std::process::exit(1);
        }
    };
    println!("Demo collector running on pipe {pipe} for {}. Press Ctrl+C to stop.", database.display());

    let mut known = HashSet::new();
    let rollups = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 24 };
    let mut last_rollup = Instant::now();
    loop {
        feed(&mut store, &moment, &mut known, wall_secs());
        if last_rollup.elapsed() >= Duration::from_secs(60) {
            let _ = roll_up_due(&mut store, Timestamp::from_micros(wall_secs() * 1_000_000), &rollups);
            last_rollup = Instant::now();
        }
        std::thread::sleep(FEED_EVERY);
    }
}
