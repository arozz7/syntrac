//! The Network history: destinations ranked over a range, destinations that are new, and one destination's history.

use std::net::Ipv4Addr;
use std::time::Duration;

use syntrac_app_lib::history_network::{
    destination_history, destination_ranking, new_destinations_view, DEFAULT_DESTINATION_ROWS, MAX_DESTINATION_ROWS,
    UNKNOWN_PROGRAM,
};
use syntrac_core::{
    roll_up_due, AddressScope, DestinationOrder, EventRepository, MetricsRepository, ProcessRepository, RepoError,
    RollupSettings, Timestamp,
};
use syntrac_testkit::fixtures::{
    at_secs, dns_query, metrics_sample, net_flow, process_metric, running_process, EventFactory,
};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const WEEK: u64 = 7 * 24 * 3600;
const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";
const CODE: &str = r"C:\Apps\code.exe";

const SITE: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 9);
const LATER_SITE: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 7);
const ROUTER: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 60 * HOUR)
}

fn start<R: ProcessRepository>(repo: &mut R, pid: u32, path: &str) -> syntrac_core::ProcessRecord {
    let mut record = running_process(pid, at_secs(secs(0, 0)));
    record.image_path = path.to_owned();
    repo.record_start(record.clone()).unwrap();
    record
}

/// History begins in hour 0. Chrome talks to SITE (hour 0 and hour 2), Code to SITE once (hour 0) and to the router, and in
/// hour 30 Code talks to LATER_SITE for the first time. SITE was asked for as two names; LATER_SITE as one.
fn scenario() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let chrome = start(&mut repo, 10, CHROME);
    let code = start(&mut repo, 11, CODE);
    for (hour, minutes) in [(0, 10), (2, 10), (30, 10)] {
        repo.append_metrics(&metrics_sample(
            secs(hour, minutes),
            10.0,
            vec![process_metric(10, Some(chrome.id), 5.0)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let mut f = EventFactory::default();
    let mut events = Vec::new();
    let mut flow = |owner: &syntrac_core::ProcessRecord, target: Ipv4Addr, sent: u64, received: u64, at: i64| {
        let mut event = f.make(at_secs(at), net_flow(owner.pid, target, sent, received, at));
        event.process_instance_id = Some(owner.id);
        event
    };
    events.push(flow(&chrome, SITE, 100, 900, secs(0, 15)));
    events.push(flow(&chrome, SITE, 10, 90, secs(2, 15)));
    events.push(flow(&code, SITE, 1, 1, secs(0, 40)));
    events.push(flow(&code, ROUTER, 5000, 5000, secs(0, 50)));
    events.push(flow(&code, LATER_SITE, 7, 3, secs(30, 15)));
    repo.append(&events).unwrap();
    repo.append(&[
        f.make(at_secs(secs(0, 12)), dns_query(10, "Old.example.com", &[SITE])),
        f.make(at_secs(secs(2, 12)), dns_query(10, "www.example.com", &[SITE])),
        f.make(at_secs(secs(30, 12)), dns_query(11, "later.example.org", &[LATER_SITE])),
    ])
    .unwrap();
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
    roll_up_due(&mut repo, at_secs(secs(40, 0)), &settings).unwrap();
    repo
}

// ---- the ranking

#[test]
fn destinations_are_ranked_by_bytes_with_the_newest_name_and_how_many_other_names_there_are() {
    let view =
        destination_ranking(&scenario(), now(), WEEK, DestinationOrder::Bytes, AddressScope::Internet, 10).unwrap();

    let first = &view.destinations[0];
    assert_eq!(first.addr, "203.0.113.9");
    assert_eq!((first.bytes_sent, first.bytes_received, first.flows, first.programs), (111, 991, 3, 2));
    assert_eq!(first.name.as_deref(), Some("www.example.com"), "the name asked for most recently");
    assert_eq!(first.other_names, 1);
    assert_eq!(first.class, "public");
    assert_eq!(first.first_seen_ms, Some(secs(0, 15) * 1000));
    assert_eq!(first.last_seen_ms, Some(secs(2, 15) * 1000));
}

#[test]
fn an_address_nobody_asked_a_name_for_has_none() {
    let mut repo = scenario();
    let mut f = EventFactory::after(1000);
    let chrome = start(&mut repo, 12, CHROME);
    // Hour 30 is already rolled up, so this flow is in a later hour.
    let mut later = f.make(at_secs(secs(45, 20)), net_flow(12, Ipv4Addr::new(192, 0, 2, 50), 1, 1, secs(45, 20)));
    later.process_instance_id = Some(chrome.id);
    repo.append(&[later]).unwrap();
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
    roll_up_due(&mut repo, at_secs(secs(50, 0)), &settings).unwrap();

    let view = destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::Internet, 100).unwrap();

    let row = view.destinations.iter().find(|row| row.addr == "192.0.2.50").unwrap();
    assert_eq!((row.name.as_deref(), row.other_names), (None, 0));
}

#[test]
fn the_router_is_hidden_unless_every_address_is_asked_for() {
    let repo = scenario();

    let internet =
        destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::Internet, 10).unwrap();
    let all = destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::All, 10).unwrap();

    assert!(internet.destinations.iter().all(|row| row.addr != "192.168.1.1"));
    assert_eq!(all.destinations[0].addr, "192.168.1.1", "10,000 bytes: the most of all");
    assert_eq!(all.destinations[0].class, "private");
}

#[test]
fn the_view_says_when_history_begins_and_how_many_hours_have_data() {
    let view = destination_ranking(&scenario(), now(), WEEK, DestinationOrder::Flows, AddressScope::All, 10).unwrap();

    assert_eq!(view.history_starts_ms, Some(secs(0, 0) * 1000));
    assert_eq!(view.hours_in_range, 3);
}

#[test]
fn the_number_of_rows_is_limited() {
    let repo = scenario();
    let one = destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::All, 1).unwrap();
    let huge = destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::All, usize::MAX).unwrap();

    assert_eq!(one.destinations.len(), 1);
    assert!(huge.destinations.len() <= MAX_DESTINATION_ROWS);
    const { assert!(DEFAULT_DESTINATION_ROWS <= MAX_DESTINATION_ROWS) };
}

#[test]
fn an_empty_database_ranks_nothing() {
    let view = destination_ranking(
        &InMemoryRepository::default(),
        now(),
        WEEK,
        DestinationOrder::Bytes,
        AddressScope::Internet,
        10,
    )
    .unwrap();

    assert!(view.destinations.is_empty());
    assert_eq!(view.history_starts_ms, None);
}

// ---- what is new

#[test]
fn the_first_day_of_history_is_the_baseline_and_later_first_sightings_are_new() {
    let view = new_destinations_view(&scenario(), now(), WEEK, AddressScope::Internet, 10).unwrap();

    assert_eq!(view.history_starts_ms, Some(secs(0, 0) * 1000));
    assert_eq!(view.baseline_until_ms, Some(secs(24, 0) * 1000));
    let addresses: Vec<&str> = view.destinations.iter().map(|row| row.addr.as_str()).collect();
    assert_eq!(addresses, ["198.51.100.7"], "SITE was first seen in hour 0: baseline");
    let row = &view.destinations[0];
    assert_eq!(row.first_program.as_deref(), Some("code.exe"));
    assert_eq!(row.name.as_deref(), Some("later.example.org"));
    assert_eq!(row.first_seen_ms, secs(30, 15) * 1000);
}

#[test]
fn a_shorter_window_than_the_baseline_allows_does_not_reach_back_into_the_baseline() {
    // The window is 40 hours, so it starts at hour 20, inside the baseline (hours 0 to 24): the baseline still wins.
    let view = new_destinations_view(&scenario(), now(), 40 * 3600, AddressScope::All, 10).unwrap();

    let addresses: Vec<&str> = view.destinations.iter().map(|row| row.addr.as_str()).collect();
    assert_eq!(addresses, ["198.51.100.7"]);
}

#[test]
fn a_window_that_starts_after_the_first_sighting_shows_nothing() {
    // 20 hours: starts at hour 40; the new destination was first seen at hour 30.
    let view = new_destinations_view(&scenario(), now(), 20 * 3600, AddressScope::All, 10).unwrap();

    assert!(view.destinations.is_empty());
}

#[test]
fn with_no_history_nothing_is_new() {
    let view = new_destinations_view(&InMemoryRepository::default(), now(), WEEK, AddressScope::All, 10).unwrap();

    assert!(view.destinations.is_empty());
    assert_eq!(view.baseline_until_ms, None);
}

// ---- one destination

#[test]
fn a_destination_shows_its_names_its_programs_and_its_hours() {
    let view = destination_history(&scenario(), now(), WEEK, "203.0.113.9").unwrap();

    assert_eq!(view.addr, "203.0.113.9");
    assert_eq!(view.class, "public");
    let names: Vec<&str> = view.names.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["www.example.com", "old.example.com"], "newest first");
    assert_eq!(view.names[1].first_seen_ms, secs(0, 12) * 1000);
    assert_eq!((view.first_seen_ms, view.last_seen_ms), (Some(secs(0, 15) * 1000), Some(secs(2, 15) * 1000)));

    let programs: Vec<(&str, u64)> =
        view.programs.iter().map(|p| (p.name.as_str(), p.bytes_sent + p.bytes_received)).collect();
    assert_eq!(programs, [("chrome.exe", 1100), ("code.exe", 2)], "most bytes first");
    assert_eq!(view.programs[0].path.as_deref(), Some(CHROME));
}

#[test]
fn a_destination_has_one_row_per_hour_with_data_and_zero_in_the_hours_it_was_quiet() {
    let view = destination_history(&scenario(), now(), WEEK, "203.0.113.9").unwrap();

    assert_eq!(view.hours.len(), 3, "the three hours that have any system data");
    let by_hour: Vec<(i64, u64, u64)> =
        view.hours.iter().map(|h| (h.hour_ms, h.bytes_sent, h.bytes_received)).collect();
    assert_eq!(by_hour, [(secs(0, 0) * 1000, 101, 901), (secs(2, 0) * 1000, 10, 90), (secs(30, 0) * 1000, 0, 0),]);
}

#[test]
fn an_address_that_was_never_seen_has_an_empty_history_not_an_error() {
    let view = destination_history(&scenario(), now(), WEEK, "192.0.2.200").unwrap();

    assert!(view.names.is_empty() && view.programs.is_empty());
    assert_eq!((view.first_seen_ms, view.last_seen_ms), (None, None));
}

#[test]
fn text_that_is_not_an_address_is_refused_in_plain_words() {
    let error = destination_history(&scenario(), now(), WEEK, "not an address").unwrap_err();

    match error {
        RepoError::Backend(message) => assert!(message.contains("not an address"), "{message}"),
        other => panic!("unexpected error {other:?}"),
    }
}

#[test]
fn traffic_with_no_identified_program_is_called_unknown_program() {
    let mut repo = InMemoryRepository::default();
    repo.append_metrics(&metrics_sample(secs(0, 10), 5.0, vec![], 0.0, 0)).unwrap();
    let mut f = EventFactory::default();
    repo.append(&[f.make(at_secs(secs(0, 15)), net_flow(99, SITE, 4, 6, secs(0, 15)))]).unwrap();
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();

    let view = destination_history(&repo, now(), WEEK, "203.0.113.9").unwrap();

    assert_eq!(view.programs[0].name, UNKNOWN_PROGRAM);
    assert_eq!(view.programs[0].path, None);
}

// ---- the backend, with a real database

mod backend {
    use super::*;
    use syntrac_app_lib::backend::{Backend, Data};
    use syntrac_app_lib::history_network::DestinationOrderParam;
    use syntrac_app_lib::settings::AppSettings;
    use syntrac_store::SqliteStore;
    use syntrac_testkit::ManualClock;

    fn settings(path: &std::path::Path, tag: &str) -> AppSettings {
        AppSettings::for_database(path.to_path_buf(), &format!("syntrac-network-{tag}-{}", std::process::id()))
    }

    fn real_database(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let path = dir.path().join("syntrac.db");
        let mut store = SqliteStore::open(&path).unwrap();
        // The same scenario as the in-memory tests, written through the real store.
        let chrome = start(&mut store, 10, CHROME);
        store
            .append_metrics(&metrics_sample(secs(0, 10), 10.0, vec![process_metric(10, Some(chrome.id), 5.0)], 0.0, 0))
            .unwrap();
        let mut f = EventFactory::default();
        let mut flow = f.make(at_secs(secs(0, 15)), net_flow(10, SITE, 100, 900, secs(0, 15)));
        flow.process_instance_id = Some(chrome.id);
        store.append(&[flow, f.make(at_secs(secs(0, 12)), dns_query(10, "www.example.com", &[SITE]))]).unwrap();
        let rollup = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
        roll_up_due(&mut store, at_secs(secs(5, 0)), &rollup).unwrap();
        path
    }

    fn rows(backend: &Backend<ManualClock>) -> Vec<String> {
        match backend.top_destinations(WEEK, DestinationOrderParam::Bytes, AddressScope::Internet, Some(10)) {
            Data::Ok { data } => data.destinations.into_iter().map(|row| row.addr).collect(),
            other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
        }
    }

    #[test]
    fn the_backend_serves_the_three_views_from_a_real_database_and_reports_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = real_database(&dir);
        let backend = Backend::new(settings(&path, "serve"), ManualClock::new(now()));

        assert_eq!(rows(&backend), ["203.0.113.9"]);
        match backend.destination_history(WEEK, "203.0.113.9".to_owned()) {
            Data::Ok { data } => assert_eq!(data.names[0].name, "www.example.com"),
            other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
        }
        assert!(matches!(backend.new_destinations(WEEK, AddressScope::Internet, None), Data::Ok { .. }));
        assert!(
            matches!(backend.destination_history(WEEK, "no".to_owned()), Data::Error { .. }),
            "text that is not an address is an error state, not a crash"
        );

        let missing = Backend::new(settings(&dir.path().join("nope.db"), "missing"), ManualClock::new(now()));
        assert!(matches!(
            missing.top_destinations(3600, DestinationOrderParam::Bytes, AddressScope::All, None),
            Data::NoDatabase { .. }
        ));
    }

    #[test]
    fn a_newly_rolled_up_hour_shows_up_even_though_the_sums_of_a_range_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = real_database(&dir);
        let backend = Backend::new(settings(&path, "cache"), ManualClock::new(now()));
        assert_eq!(rows(&backend), ["203.0.113.9"]);
        assert_eq!(rows(&backend), ["203.0.113.9"], "asked twice: the kept sums are used");

        {
            let mut store = SqliteStore::open(&path).unwrap();
            let code = start(&mut store, 11, CODE);
            let mut f = EventFactory::after(1000);
            let mut flow = f.make(at_secs(secs(6, 15)), net_flow(11, LATER_SITE, 5000, 5000, secs(6, 15)));
            flow.process_instance_id = Some(code.id);
            store.append(&[flow]).unwrap();
            let rollup = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
            roll_up_due(&mut store, at_secs(secs(8, 0)), &rollup).unwrap();
        }

        assert_eq!(rows(&backend), ["198.51.100.7", "203.0.113.9"], "the new hour is not hidden by kept sums");
    }
}
