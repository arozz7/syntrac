//! The app's two owner-data commands: asking the service what it has, and asking it to download. The service is a fake on a real
//! pipe; the app never downloads anything itself.

use std::sync::{Arc, Mutex};

use syntrac_app_lib::backend::Backend;
use syntrac_app_lib::settings::AppSettings;
use syntrac_core::Timestamp;
use syntrac_ipc::pipe::{PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{OwnerDataReport, OwnerDatasetReport, Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_testkit::ManualClock;

struct Recording {
    seen: Mutex<Vec<Request>>,
    reply: Response,
}

impl Handler for Recording {
    fn handle(&self, request: Request) -> Response {
        self.seen.lock().unwrap().push(request);
        self.reply.clone()
    }
}

fn backend_with_service(tag: &str, reply: Response) -> (Backend<ManualClock>, PipeServer, Arc<Recording>) {
    let name = format!("syntrac-owner-{}-{tag}", std::process::id());
    let handler = Arc::new(Recording { seen: Mutex::new(Vec::new()), reply });
    let server = PipeServer::start(PipeServerConfig::new(&name), handler.clone()).expect("pipe starts");
    let settings = AppSettings::for_database(std::path::PathBuf::from("none.db"), &name);
    (Backend::new(settings, ManualClock::new(Timestamp::from_micros(1))), server, handler)
}

fn report() -> OwnerDataReport {
    OwnerDataReport {
        dataset: Some(OwnerDatasetReport {
            source: "iptoasn.com".into(),
            downloaded_at_micros: 5_000_000,
            rows: 480_000,
        }),
        refreshing: true,
        last_error: Some("the server answered 503".into()),
        allowed: true,
    }
}

#[test]
fn the_status_is_asked_of_the_service_and_shown_in_milliseconds() {
    let (backend, _server, handler) = backend_with_service("status", Response::OwnerData(report()));

    let state = backend.owner_data_status().unwrap();

    assert_eq!(handler.seen.lock().unwrap().as_slice(), [Request::OwnerDataStatus]);
    let dataset = state.dataset.unwrap();
    assert_eq!((dataset.source.as_str(), dataset.downloaded_ms, dataset.rows), ("iptoasn.com", 5_000, 480_000));
    assert!(state.refreshing && state.allowed);
    assert_eq!(state.last_error.as_deref(), Some("the server answered 503"));
}

#[test]
fn asking_for_a_download_sends_only_the_request_and_returns_the_state() {
    let (backend, _server, handler) = backend_with_service("refresh", Response::OwnerData(report()));

    let state = backend.refresh_owner_data().unwrap();

    assert_eq!(handler.seen.lock().unwrap().as_slice(), [Request::OwnerDataRefresh]);
    assert!(state.refreshing);
}

#[test]
fn a_refusal_reaches_the_user_in_the_services_own_words() {
    let (backend, _server, _handler) = backend_with_service(
        "refused",
        Response::Error { message: "Downloading owner data is switched off in the service's configuration.".into() },
    );

    let error = backend.refresh_owner_data().unwrap_err();

    assert!(error.contains("switched off"), "{error}");
}

#[test]
fn an_answer_that_is_not_an_owner_report_is_reported_as_unexpected() {
    let (backend, _server, _handler) = backend_with_service("odd", Response::Done);

    assert!(backend.owner_data_status().unwrap_err().contains("unexpected"));
}

#[test]
fn with_the_service_not_running_the_user_is_told_so() {
    let pipe = format!("syntrac-owner-nobody-home-{}", std::process::id());
    let settings = AppSettings::for_database(std::path::PathBuf::from("none.db"), &pipe);
    let backend = Backend::new(settings, ManualClock::new(Timestamp::from_micros(1)));

    let message = backend.owner_data_status().unwrap_err();

    assert!(message.to_lowercase().contains("not running"), "{message}");
    assert!(!message.contains("could not be saved"), "this is not a save: {message}");
}
