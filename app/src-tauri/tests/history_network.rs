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
fn a_row_names_the_programs_that_talked_to_it_biggest_first() {
    let view = destination_ranking(&scenario(), now(), WEEK, DestinationOrder::Bytes, AddressScope::All, 10).unwrap();

    let site = view.destinations.iter().find(|row| row.addr == "203.0.113.9").unwrap();
    let names: Vec<&str> = site.top_programs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["chrome.exe", "code.exe"]);
    assert_eq!(site.top_programs[0].path.as_deref(), Some(CHROME));
    assert_eq!(site.more_programs, 0);
    let router = view.destinations.iter().find(|row| row.addr == "192.168.1.1").unwrap();
    assert_eq!(router.top_programs.len(), 1);
}

#[test]
fn a_row_with_many_programs_shows_the_biggest_few_and_counts_the_rest_and_names_the_unidentified_bucket() {
    let mut repo = InMemoryRepository::default();
    let mut f = EventFactory::default();
    let mut events = Vec::new();
    for (index, bytes) in [400_u64, 300, 200, 100, 50].into_iter().enumerate() {
        let owner = start(&mut repo, 20 + index as u32, &format!(r"C:\Apps\app{index}.exe"));
        let mut event = f.make(at_secs(secs(0, 10)), net_flow(owner.pid, SITE, bytes, 0, secs(0, 10)));
        event.process_instance_id = Some(owner.id);
        events.push(event);
    }
    events.push(f.make(at_secs(secs(0, 20)), net_flow(99, SITE, 1, 0, secs(0, 20))));
    repo.append(&events).unwrap();
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
    roll_up_due(&mut repo, at_secs(secs(40, 0)), &settings).unwrap();

    let view = destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::All, 10).unwrap();

    let row = &view.destinations[0];
    let names: Vec<&str> = row.top_programs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["app0.exe", "app1.exe", "app2.exe"]);
    assert_eq!(row.more_programs, 3, "two more identified programs and the unidentified bucket");
    assert_eq!(row.programs, 6);
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

// ---- who owns it (the owner dataset, docs/OWNER-PLAN.md)

use syntrac_core::{parse_ip2asn, DatasetInfo, OwnerRepository};

/// A dataset that says 203.0.113.0/24 (SITE's network) belongs to one network, and nothing else is in it.
fn with_dataset(mut repo: InMemoryRepository) -> InMemoryRepository {
    let ranges = parse_ip2asn("203.0.113.0\t203.0.113.255\t64500\tus\tEXAMPLE-NET Example Networks, Inc.\n").ranges;
    let info = DatasetInfo {
        source: "iptoasn.com".into(),
        downloaded_at: at_secs(secs(50, 0)),
        rows: 1,
        bad_lines: 0,
        sha256: "00".into(),
    };
    repo.replace_dataset(&ranges, &info).unwrap();
    repo
}

#[test]
fn a_destination_row_names_the_owner_of_its_network_range_and_says_when_the_data_was_downloaded() {
    let view = destination_ranking(
        &with_dataset(scenario()),
        now(),
        WEEK,
        DestinationOrder::Bytes,
        AddressScope::Internet,
        10,
    )
    .unwrap();

    let site = view.destinations.iter().find(|row| row.addr == "203.0.113.9").unwrap();
    let owner = site.owner.as_ref().expect("an owner");
    assert_eq!(
        (owner.asn, owner.org.as_str(), owner.country.as_str()),
        (64500, "EXAMPLE-NET Example Networks, Inc.", "US"),
        "the country code is upper case"
    );
    let later = view.destinations.iter().find(|row| row.addr == "198.51.100.7").unwrap();
    assert_eq!(later.owner, None, "an address outside every range has no owner");
    let data = view.owner_data.expect("the dataset is described");
    assert_eq!((data.source.as_str(), data.rows, data.downloaded_ms), ("iptoasn.com", 1, secs(50, 0) * 1000));
}

#[test]
fn without_a_dataset_there_is_no_owner_and_no_dataset_to_describe() {
    let view =
        destination_ranking(&scenario(), now(), WEEK, DestinationOrder::Bytes, AddressScope::Internet, 10).unwrap();

    assert!(view.destinations.iter().all(|row| row.owner.is_none()));
    assert_eq!(view.owner_data, None);
}

#[test]
fn an_address_on_this_network_is_never_looked_up_for_an_owner() {
    // Even a dataset that (wrongly) covers a private range does not give the router an owner.
    let mut repo = scenario();
    let ranges = parse_ip2asn("192.168.0.0\t192.168.255.255\t1\tUS\tWrong\n").ranges;
    let info = DatasetInfo {
        source: "x".into(),
        downloaded_at: at_secs(secs(50, 0)),
        rows: 1,
        bad_lines: 0,
        sha256: "00".into(),
    };
    repo.replace_dataset(&ranges, &info).unwrap();

    let view = destination_ranking(&repo, now(), WEEK, DestinationOrder::Bytes, AddressScope::All, 10).unwrap();

    let router = view.destinations.iter().find(|row| row.addr == "192.168.1.1").unwrap();
    assert_eq!(router.owner, None);
}

#[test]
fn the_detail_and_the_new_destinations_name_the_owner_too() {
    let repo = with_dataset(scenario());

    let detail = destination_history(&repo, now(), WEEK, "203.0.113.9").unwrap();
    let new = new_destinations_view(&repo, now(), WEEK, AddressScope::Internet, 10).unwrap();

    assert_eq!(detail.owner.as_ref().map(|o| o.asn), Some(64500));
    assert!(new.destinations.iter().all(|row| row.owner.is_none()), "198.51.100.7 is in no range");
    let none = destination_history(&scenario(), now(), WEEK, "203.0.113.9").unwrap();
    assert_eq!(none.owner, None);
}

// ---- top owners (docs/ALERTS-PLAN.md, N1)

use syntrac_app_lib::history_network::owner_ranking;

#[test]
fn owners_are_ranked_with_their_destinations_and_the_dataset_is_described() {
    let view = owner_ranking(&with_dataset(scenario()), now(), WEEK, AddressScope::Internet, 10).unwrap();

    assert_eq!(view.owners.len(), 2, "the network that owns SITE, and the group nobody owns");
    let site = &view.owners[0];
    let owner = site.owner.as_ref().expect("an owner");
    assert_eq!(
        (owner.asn, owner.org.as_str(), owner.country.as_str()),
        (64500, "EXAMPLE-NET Example Networks, Inc.", "US")
    );
    assert_eq!((site.bytes_sent, site.bytes_received, site.flows, site.destinations), (111, 991, 3, 1));
    assert_eq!(site.top_destinations[0].addr, "203.0.113.9");
    assert_eq!(site.top_destinations[0].name.as_deref(), Some("www.example.com"), "the name asked for most recently");
    assert_eq!(site.more_destinations, 0);
    let rest = &view.owners[1];
    assert_eq!(rest.owner, None, "198.51.100.7 is in no range");
    assert_eq!((rest.bytes_sent, rest.bytes_received, rest.destinations), (7, 3, 1));
    assert_eq!(view.owner_data.expect("described").source, "iptoasn.com");
}

#[test]
fn without_a_dataset_every_destination_is_in_the_group_with_no_owner() {
    let view = owner_ranking(&scenario(), now(), WEEK, AddressScope::Internet, 10).unwrap();

    assert_eq!(view.owners.len(), 1);
    assert_eq!(view.owners[0].owner, None);
    assert_eq!(view.owners[0].destinations, 2);
    assert_eq!(view.owner_data, None);
}

#[test]
fn this_network_is_left_out_of_the_owners_unless_every_address_is_asked_for() {
    let internet = owner_ranking(&scenario(), now(), WEEK, AddressScope::Internet, 10).unwrap();
    let all = owner_ranking(&scenario(), now(), WEEK, AddressScope::All, 10).unwrap();

    let count = |view: &syntrac_app_lib::history_network::OwnersView| -> u32 {
        view.owners.iter().map(|row| row.destinations).sum()
    };
    assert_eq!(count(&internet), 2);
    assert_eq!(count(&all), 3, "the router counts when asked for");
}

#[test]
fn an_owner_shows_a_few_destinations_and_says_how_many_more() {
    let view = owner_ranking(&scenario(), now(), WEEK, AddressScope::All, 10).unwrap();

    let nobody = &view.owners[0];
    assert_eq!(nobody.destinations, 3);
    assert_eq!(nobody.top_destinations.len() as u32 + nobody.more_destinations, 3);
}

// ---- where does this program connect (docs/ALERTS-PLAN.md, N2)

use syntrac_app_lib::history_network::program_destinations_view;

#[test]
fn a_program_lists_where_it_connected_biggest_first_with_names_and_owners() {
    let view =
        program_destinations_view(&with_dataset(scenario()), now(), WEEK, CODE, AddressScope::Internet, 10).unwrap();

    let addrs: Vec<&str> = view.destinations.iter().map(|row| row.addr.as_str()).collect();
    assert_eq!(addrs, vec!["198.51.100.7", "203.0.113.9"], "biggest first; the router is not on the internet");
    let site = &view.destinations[1];
    assert_eq!((site.bytes_sent, site.bytes_received, site.flows), (1, 1, 1));
    assert_eq!(site.name.as_deref(), Some("www.example.com"));
    assert_eq!(site.owner.as_ref().map(|o| o.asn), Some(64500));
    assert_eq!(view.destinations[0].owner, None);
    assert_eq!(view.total_destinations, 2);
    assert!(view.owner_data.is_some());
}

#[test]
fn only_a_destination_first_reached_after_the_baseline_by_this_program_is_new() {
    let view = program_destinations_view(&scenario(), now(), WEEK, CODE, AddressScope::Internet, 10).unwrap();

    let later = view.destinations.iter().find(|row| row.addr == "198.51.100.7").unwrap();
    let site = view.destinations.iter().find(|row| row.addr == "203.0.113.9").unwrap();
    assert!(later.is_new, "first reached in hour 30, after the first day of history");
    assert!(!site.is_new, "reached on the first day of history: the baseline, not news");
    assert_eq!(later.first_seen_ms, Some(secs(30, 15) * 1000));
}

#[test]
fn the_path_is_matched_without_regard_to_case_and_an_unknown_program_has_no_destinations() {
    let upper =
        program_destinations_view(&scenario(), now(), WEEK, &CODE.to_uppercase(), AddressScope::Internet, 10).unwrap();
    assert_eq!(upper.destinations.len(), 2);

    let none = program_destinations_view(&scenario(), now(), WEEK, r"C:\Apps\nothing.exe", AddressScope::Internet, 10)
        .unwrap();
    assert!(none.destinations.is_empty());
    assert_eq!(none.total_destinations, 0);
}

#[test]
fn the_list_is_limited_and_says_how_many_destinations_there_are() {
    let view = program_destinations_view(&scenario(), now(), WEEK, CODE, AddressScope::All, 2).unwrap();

    assert_eq!(view.destinations.len(), 2);
    assert_eq!(view.total_destinations, 3, "the router counts when every address is asked for");
}

// ---- the digest of the last day (docs/ALERTS-PLAN.md, F4)

use syntrac_app_lib::history_findings::digest_view;

fn with_two_owners(mut repo: InMemoryRepository) -> InMemoryRepository {
    let ranges = parse_ip2asn(
        "198.51.100.0\t198.51.100.255\t64501\tus\tLATER-NET Later Networks\n203.0.113.0\t203.0.113.255\t64500\tus\tEXAMPLE-NET Example Networks\n",
    )
    .ranges;
    let info = DatasetInfo {
        source: "iptoasn.com".into(),
        downloaded_at: at_secs(secs(50, 0)),
        rows: 2,
        bad_lines: 0,
        sha256: "00".into(),
    };
    repo.replace_dataset(&ranges, &info).unwrap();
    repo
}

#[test]
fn the_digest_counts_the_internet_addresses_first_seen_in_the_last_day_and_the_networks_never_seen_before() {
    // Hour 33: LATER_SITE (198.51.100.7, first seen in hour 30) is within the last day and after the first day of history.
    let now = at_secs(secs(33, 0));

    let digest = digest_view(&with_two_owners(scenario()), now).unwrap();

    assert_eq!(digest.window_hours, 24);
    assert_eq!(digest.new_destinations, 1, "SITE is from the first day; the router is not on the internet");
    assert_eq!(digest.new_owners, 1, "LATER-NET had never been reached before");
    assert!(digest.owner_data);
}

#[test]
fn without_owner_data_the_digest_still_counts_destinations_and_says_it_cannot_count_networks() {
    let digest = digest_view(&scenario(), at_secs(secs(33, 0))).unwrap();

    assert_eq!((digest.new_destinations, digest.new_owners, digest.owner_data), (1, 0, false));
}

#[test]
fn an_address_in_a_network_already_reached_is_a_new_destination_but_not_a_new_network() {
    // A dataset that puts both addresses in ONE network: the later address adds no new owner.
    let mut repo = scenario();
    let ranges = parse_ip2asn("198.51.100.0\t203.0.113.255\t64500\tus\tONE-NET One Network\n").ranges;
    let info = DatasetInfo {
        source: "x".into(),
        downloaded_at: at_secs(secs(50, 0)),
        rows: 1,
        bad_lines: 0,
        sha256: "00".into(),
    };
    repo.replace_dataset(&ranges, &info).unwrap();

    let digest = digest_view(&repo, at_secs(secs(33, 0))).unwrap();

    assert_eq!((digest.new_destinations, digest.new_owners), (1, 0));
}

#[test]
fn an_empty_history_has_nothing_new() {
    let digest = digest_view(&InMemoryRepository::default(), at_secs(secs(33, 0))).unwrap();

    assert_eq!((digest.new_destinations, digest.new_owners), (0, 0));
}
