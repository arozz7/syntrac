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
    Backend::new(
        AppSettings { database_path: database.to_path_buf(), pipe_name: pipe.to_owned() },
        ManualClock::new(at_secs(now_secs)),
    )
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
