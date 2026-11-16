//! The service's owner-data control: what it reports, that a refresh runs in the background, and that every failure leaves the
//! stored dataset alone.

use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_core::{OwnerRepository, RefreshLimits, Timestamp};
use syntrac_ipc::protocol::{OwnerDataReport, Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_service::{KnownProductsAdmin, OwnerControl, OwnerData, ServiceHandler};
use syntrac_store::SqliteStore;
use syntrac_testkit::{FakeDatasetFetcher, InMemoryRepository, ManualClock};

fn file(blocks: u32) -> String {
    (0..blocks).map(|n| format!("40.0.{n}.0\t40.0.{n}.255\t{}\tUS\tOwner {n}\n", 200 + n)).collect()
}

fn limits() -> RefreshLimits {
    RefreshLimits { min_ranges: 3, max_bad_percent: 5 }
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    fetcher: Arc<FakeDatasetFetcher>,
}

fn fixture(fetcher: FakeDatasetFetcher) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    SqliteStore::open(&path).unwrap();
    Fixture { _dir: dir, path, fetcher: Arc::new(fetcher) }
}

fn control(f: &Fixture, allowed: bool) -> OwnerData<Arc<FakeDatasetFetcher>, SqliteStore, ManualClock> {
    let path = f.path.clone();
    OwnerData::new(
        Arc::clone(&f.fetcher),
        Box::new(move || SqliteStore::open(&path)),
        ManualClock::new(Timestamp::from_micros(9_000_000)),
        allowed,
        limits(),
    )
}

fn wait_until_idle(control: &impl OwnerControl) -> OwnerDataReport {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let report = control.report();
        if !report.refreshing {
            return report;
        }
        assert!(Instant::now() < deadline, "the refresh never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stored(f: &Fixture) -> SqliteStore {
    SqliteStore::open(&f.path).unwrap()
}

#[test]
fn before_any_download_there_is_no_dataset_and_nothing_is_running() {
    let f = fixture(FakeDatasetFetcher::new(&file(5)));

    let report = control(&f, true).report();

    assert_eq!(report, OwnerDataReport { dataset: None, refreshing: false, last_error: None, allowed: true });
    assert_eq!(f.fetcher.calls(), 0, "asking for the status never touches the network");
}

#[test]
fn a_refresh_answers_at_once_runs_in_the_background_and_then_reports_the_dataset() {
    let f = fixture(FakeDatasetFetcher::new(&file(5)).slow(Duration::from_millis(300)));
    let control = control(&f, true);

    let Response::OwnerData(started) = control.refresh() else { panic!("expected a report") };
    assert!(started.refreshing, "the answer does not wait for the download");

    let done = wait_until_idle(&control);
    let dataset = done.dataset.expect("a dataset after a good download");
    assert_eq!(
        (dataset.rows, dataset.downloaded_at_micros, dataset.source.as_str()),
        (5, 9_000_000, "iptoasn.com (test)")
    );
    assert_eq!(done.last_error, None);
    assert_eq!(stored(&f).lookup_owner("40.0.2.9".parse().unwrap()).unwrap().map(|o| o.asn), Some(202));
}

#[test]
fn asking_again_while_a_download_runs_does_not_start_a_second_one() {
    let f = fixture(FakeDatasetFetcher::new(&file(5)).slow(Duration::from_millis(300)));
    let control = control(&f, true);

    control.refresh();
    let Response::OwnerData(again) = control.refresh() else { panic!("expected a report") };
    wait_until_idle(&control);

    assert!(again.refreshing);
    assert_eq!(f.fetcher.calls(), 1);
}

#[test]
fn a_failed_download_is_reported_in_words_and_the_old_dataset_stays() {
    let f = fixture(FakeDatasetFetcher::new(&file(5)));
    let good = control(&f, true);
    good.refresh();
    wait_until_idle(&good);

    let failing = Fixture {
        _dir: tempfile::tempdir().unwrap(),
        path: f.path.clone(),
        fetcher: Arc::new(FakeDatasetFetcher::failing("timed out")),
    };
    let control = control(&failing, true);
    assert_eq!(control.report().dataset.map(|d| d.rows), Some(5), "the dataset is loaded from the database at start");
    control.refresh();
    let report = wait_until_idle(&control);

    assert!(report.last_error.as_deref().is_some_and(|e| e.contains("timed out")), "{report:?}");
    assert_eq!(report.dataset.map(|d| d.rows), Some(5));
    assert_eq!(stored(&f).dataset_info().unwrap().map(|i| i.rows), Some(5));
}

#[test]
fn a_file_that_is_not_the_dataset_is_reported_and_not_stored() {
    let f = fixture(FakeDatasetFetcher::new("<html>Service unavailable</html>"));
    let control = control(&f, true);

    control.refresh();
    let report = wait_until_idle(&control);

    assert!(report.last_error.as_deref().is_some_and(|e| e.contains("not used")), "{report:?}");
    assert_eq!(report.dataset, None);
    assert_eq!(stored(&f).dataset_info().unwrap(), None);
}

#[test]
fn a_later_good_download_clears_the_earlier_error() {
    let f = fixture(FakeDatasetFetcher::new("junk"));
    let first = control(&f, true);
    first.refresh();
    assert!(wait_until_idle(&first).last_error.is_some());

    let good = Fixture {
        _dir: tempfile::tempdir().unwrap(),
        path: f.path.clone(),
        fetcher: Arc::new(FakeDatasetFetcher::new(&file(4))),
    };
    let second = control(&good, true);
    second.refresh();

    assert_eq!(wait_until_idle(&second).last_error, None);
}

#[test]
fn when_downloading_is_switched_off_nothing_is_fetched() {
    let f = fixture(FakeDatasetFetcher::new(&file(5)));
    let control = control(&f, false);

    let response = control.refresh();

    assert!(
        matches!(response, Response::Error { ref message } if message.contains("owner_data_allowed")),
        "{response:?}"
    );
    assert_eq!(f.fetcher.calls(), 0);
    assert!(!control.report().allowed);
}

#[test]
fn the_pipe_handler_routes_the_owner_requests_and_says_so_when_there_is_no_owner_control() {
    let f = fixture(FakeDatasetFetcher::new(&file(5)));
    let status = StatusStub;
    let admin = KnownProductsAdmin::new(InMemoryRepository::default(), ManualClock::new(Timestamp::from_micros(1)));
    let with = ServiceHandler::new(StatusStub, admin).with_owner(Arc::new(control(&f, true)));

    assert!(matches!(with.handle(Request::OwnerDataStatus), Response::OwnerData(_)));
    assert!(matches!(with.handle(Request::OwnerDataRefresh), Response::OwnerData(_)));

    let without = ServiceHandler::new(
        status,
        KnownProductsAdmin::new(InMemoryRepository::default(), ManualClock::new(Timestamp::from_micros(1))),
    );
    assert!(matches!(without.handle(Request::OwnerDataStatus), Response::Error { .. }));
    assert!(matches!(without.handle(Request::OwnerDataRefresh), Response::Error { .. }));
}

/// Stands in for the status board.
struct StatusStub;

impl Handler for StatusStub {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "stub".into() },
            other => Response::Error { message: format!("status stub: {other:?}") },
        }
    }
}
