//! The backend opens the database read-only on every call and turns problems into explicit states the UI can show,
//! never raw errors. Uses real SQLite files and, for health, a real pipe.

use std::sync::Arc;
use std::time::Duration;

use syntrac_app_lib::backend::{Backend, Data};
use syntrac_app_lib::health::HealthState;
use syntrac_app_lib::settings::AppSettings;
use syntrac_app_lib::views::ProcessListParams;
use syntrac_core::{MetricsRepository, ProcessRepository, Timestamp};
use syntrac_ipc::pipe::{PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{CollectorTotals, Request, Response, StatusReport};
use syntrac_ipc::serve::Handler;
use syntrac_store::SqliteStore;
use syntrac_testkit::fixtures::{at_secs, metrics_sample, process_metric, running_process};
use syntrac_testkit::ManualClock;

fn backend(database: &std::path::Path, pipe: &str, now_secs: i64) -> Backend<ManualClock> {
    Backend::new(AppSettings::for_database(database.to_path_buf(), pipe), ManualClock::new(at_secs(now_secs)))
}

/// A backend that, like the real app, has no explicit database: it must find the database through the service.
fn following_backend(default_database: &std::path::Path, pipe: &str, now_secs: i64) -> Backend<ManualClock> {
    let mut settings = AppSettings::for_database(default_database.to_path_buf(), pipe);
    settings.database_pinned = false;
    Backend::new(settings, ManualClock::new(at_secs(now_secs)))
}

fn unique_pipe(tag: &str) -> String {
    format!("syntrac-app-test-{tag}-{}", std::process::id())
}

#[test]
fn a_missing_database_is_reported_as_no_database_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let backend = backend(&dir.path().join("nope.db"), &unique_pipe("a"), 10_000);
    assert!(matches!(backend.overview(3600), Data::NoDatabase { .. }));
    assert!(matches!(backend.processes(ProcessListParams::default()), Data::NoDatabase { .. }));
    assert!(matches!(backend.connections(3600, None), Data::NoDatabase { .. }));
}

#[test]
fn a_database_from_another_version_is_reported_as_a_schema_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let result = backend(&path, &unique_pipe("b"), 10_000).overview(3600);
    match result {
        Data::SchemaMismatch { message } => assert!(message.contains("v1"), "{message}"),
        other => panic!("expected a schema mismatch, got {}", serde_json::to_string(&other).unwrap()),
    }
}

#[test]
fn a_file_that_is_not_a_database_is_a_plain_error_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garbage.db");
    std::fs::write(&path, b"this is not a sqlite database at all, just some text to fill the header").unwrap();
    assert!(matches!(backend(&path, &unique_pipe("c"), 10_000).overview(3600), Data::Error { .. }));
}

#[test]
fn a_real_database_gives_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let mut chrome = running_process(1, at_secs(100));
        chrome.image_path = r"C:\apps\chrome.exe".into();
        store.record_start(chrome.clone()).unwrap();
        store
            .append_metrics(&metrics_sample(9_990, 30.0, vec![process_metric(1, Some(chrome.id), 12.0)], 1.0, 5))
            .unwrap();
    }
    let backend = backend(&path, &unique_pipe("d"), 10_000);

    match backend.overview(3600) {
        Data::Ok { data } => assert_eq!(data.cpu.processes[0].name, "chrome.exe"),
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    }
    match backend.processes(ProcessListParams { running_only: true, ..ProcessListParams::default() }) {
        Data::Ok { data } => assert_eq!(data.len(), 1),
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    }
}

#[test]
fn the_wire_shape_of_each_state_is_what_the_frontend_expects() {
    let ok = serde_json::to_value(Data::Ok { data: 7 }).unwrap();
    assert_eq!((ok["state"].as_str(), ok["data"].as_i64()), (Some("ok"), Some(7)));
    let none = serde_json::to_value(Data::<i32>::NoDatabase { message: "m".into() }).unwrap();
    assert_eq!(none["state"], "no_database");
    let mismatch = serde_json::to_value(Data::<i32>::SchemaMismatch { message: "m".into() }).unwrap();
    assert_eq!(mismatch["state"], "schema_mismatch");
}

struct FixedStatus;

impl Handler for FixedStatus {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "t".into() },
            Request::Status => Response::Status(StatusReport {
                version: "t".into(),
                pid: 4242,
                started_at_micros: 0,
                uptime_secs: 99,
                database_path: "db".into(),
                etw_enabled: false,
                last_tick_at_micros: None,
                totals: CollectorTotals::default(),
                etw: None,
            }),
        }
    }
}

#[test]
fn health_is_offline_when_no_collector_listens_and_running_when_one_answers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.append_metrics(&metrics_sample(9_995, 10.0, vec![], 0.0, 0)).unwrap();
    }
    let pipe = unique_pipe("e");
    let backend = backend(&path, &pipe, 10_000);

    let offline = backend.health();
    assert_eq!(offline.state, HealthState::Offline);
    assert_eq!(offline.data_age_secs, Some(5), "the newest sample is 5 s old");

    let mut config = PipeServerConfig::new(&pipe);
    config.read_timeout = Duration::from_secs(2);
    let _server = PipeServer::start(config, Arc::new(FixedStatus)).unwrap();
    let running = backend.health();
    assert_eq!(running.state, HealthState::Running);
    assert_eq!((running.pid, running.uptime_secs), (Some(4242), Some(99)));
    let _ = Timestamp::from_micros(0);
}

// ---- the database moved: follow where the service says it is

/// A service that reports a given database path.
struct ReportsDatabase(String);

impl Handler for ReportsDatabase {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "t".into() },
            Request::Status => Response::Status(StatusReport {
                version: "t".into(),
                pid: 1,
                started_at_micros: 0,
                uptime_secs: 5,
                database_path: self.0.clone(),
                etw_enabled: false,
                last_tick_at_micros: None,
                totals: CollectorTotals::default(),
                etw: None,
            }),
        }
    }
}

fn database_with_one_process(path: &std::path::Path) {
    let mut store = SqliteStore::open(path).unwrap();
    store.record_start(running_process(1, at_secs(100))).unwrap();
}

fn serve(pipe: &str, reported_path: &std::path::Path) -> PipeServer {
    let mut config = PipeServerConfig::new(pipe);
    config.read_timeout = Duration::from_secs(2);
    PipeServer::start(config, Arc::new(ReportsDatabase(reported_path.display().to_string()))).unwrap()
}

#[test]
fn after_the_data_moved_the_first_call_finds_the_new_database_through_the_service() {
    let dir = tempfile::tempdir().unwrap();
    let moved = dir.path().join("moved").join("syntrac.db");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    database_with_one_process(&moved);
    let pipe = unique_pipe("move1");
    let _server = serve(&pipe, &moved);

    // The app still believes the old location, where there is no database any more.
    let backend = following_backend(&dir.path().join("old").join("syntrac.db"), &pipe, 10_000);
    match backend.processes(ProcessListParams::default()) {
        Data::Ok { data } => assert_eq!(data.len(), 1),
        other => panic!("expected the moved database to be found, got {}", serde_json::to_string(&other).unwrap()),
    }
}

#[test]
fn a_pinned_database_is_not_replaced_by_what_the_service_reports() {
    let dir = tempfile::tempdir().unwrap();
    let moved = dir.path().join("syntrac.db");
    database_with_one_process(&moved);
    let pipe = unique_pipe("move2");
    let _server = serve(&pipe, &moved);

    let backend = backend(&dir.path().join("elsewhere.db"), &pipe, 10_000);
    assert!(matches!(backend.processes(ProcessListParams::default()), Data::NoDatabase { .. }));
}

#[test]
fn with_the_service_offline_a_missing_database_is_still_reported_as_no_data_and_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let backend = following_backend(&dir.path().join("gone.db"), &unique_pipe("move3"), 10_000);
    match backend.overview(3600) {
        Data::NoDatabase { message } => assert!(message.contains("gone.db"), "{message}"),
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    }
}

#[test]
fn health_polling_also_makes_the_app_follow_a_moved_database() {
    let dir = tempfile::tempdir().unwrap();
    let moved = dir.path().join("syntrac.db");
    database_with_one_process(&moved);
    let pipe = unique_pipe("move4");
    let _server = serve(&pipe, &moved);

    let backend = following_backend(&dir.path().join("old.db"), &pipe, 10_000);
    let _ = backend.health();
    // Even if the service later goes away, the app now knows where the data is.
    drop(_server);
    assert!(matches!(backend.processes(ProcessListParams::default()), Data::Ok { .. }));
}
