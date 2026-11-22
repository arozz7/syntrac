//! The connection map view (docs/PHASE5-PLAN.md, A2): which programs talked to which networks over a range.

use std::net::Ipv4Addr;
use std::time::Duration;

use syntrac_app_lib::history_map::{connection_map, ConnectionMapView, MAX_MAP_NODES};
use syntrac_core::{
    parse_ip2asn, roll_up_due, AddressScope, DatasetInfo, EventRepository, MetricsRepository, OwnerRepository,
    ProcessRepository, RollupSettings, Timestamp,
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
const OTHER_SITE: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 10);
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

/// Chrome talks to SITE (and OTHER_SITE, the same network) a lot, Code to SITE a little, to LATER_SITE (no owner) and to the router
/// (this network). 203.0.113.0/24 belongs to one network. Hour 0, except one old flow in hour 1 that a one-day range excludes.
fn scenario() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let chrome = start(&mut repo, 10, CHROME);
    let code = start(&mut repo, 11, CODE);
    repo.append_metrics(&metrics_sample(secs(0, 10), 10.0, vec![process_metric(10, Some(chrome.id), 5.0)], 0.0, 0))
        .unwrap();
    let mut f = EventFactory::default();
    let mut flow = |owner: &syntrac_core::ProcessRecord, target: Ipv4Addr, sent: u64, received: u64, at: i64| {
        let mut event = f.make(at_secs(at), net_flow(owner.pid, target, sent, received, at));
        event.process_instance_id = Some(owner.id);
        event
    };
    let events = vec![
        flow(&chrome, SITE, 100, 900, secs(0, 15)),
        flow(&chrome, OTHER_SITE, 10, 90, secs(0, 20)),
        flow(&code, SITE, 1, 1, secs(0, 40)),
        flow(&code, LATER_SITE, 7, 3, secs(0, 45)),
        flow(&code, ROUTER, 5000, 5000, secs(0, 50)),
        flow(&code, LATER_SITE, 1000, 0, secs(40, 15)),
    ];
    repo.append(&events).unwrap();
    repo.append(&[f.make(at_secs(secs(0, 12)), dns_query(10, "www.example.com", &[SITE]))]).unwrap();
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
    roll_up_due(&mut repo, at_secs(secs(50, 0)), &settings).unwrap();
    with_dataset(repo)
}

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

fn view(repo: &InMemoryRepository, range_secs: u64, scope: AddressScope) -> ConnectionMapView {
    connection_map(repo, now(), range_secs, scope, None).unwrap()
}

fn program_labels(view: &ConnectionMapView) -> Vec<&str> {
    view.programs.iter().map(|node| node.name.as_str()).collect()
}

fn network_labels(view: &ConnectionMapView) -> Vec<&str> {
    view.networks.iter().map(|node| node.label.as_str()).collect()
}

#[test]
fn programs_and_the_networks_they_reached_are_joined_by_links_of_their_traffic() {
    let result = view(&scenario(), WEEK, AddressScope::Internet);

    assert_eq!(program_labels(&result), ["chrome.exe", "code.exe"]);
    assert_eq!(network_labels(&result), ["EXAMPLE-NET Example Networks, Inc.", "No owner found"]);
    let find = |program: &str, network: usize| {
        let program = result.programs.iter().position(|node| node.name == program).unwrap();
        result
            .links
            .iter()
            .find(|link| link.program == program && link.network == network)
            .map(|l| (l.bytes_sent, l.bytes_received))
    };
    assert_eq!(find("chrome.exe", 0), Some((110, 990)), "SITE and OTHER_SITE are one network");
    assert_eq!(find("code.exe", 0), Some((1, 1)));
    assert_eq!(
        find("code.exe", 1),
        Some((1007, 3)),
        "LATER_SITE has no owner, and the flow in hour 40 is inside a week"
    );
    assert_eq!(find("chrome.exe", 1), None);
}

#[test]
fn a_network_names_its_owner_its_country_and_its_biggest_addresses_with_the_name_asked_for() {
    let result = view(&scenario(), WEEK, AddressScope::Internet);

    let owned = &result.networks[0];
    let owner = owned.owner.as_ref().expect("an owner");
    assert_eq!((owner.asn, owner.country.as_str()), (64500, "US"));
    assert_eq!(owned.addresses, 2);
    assert_eq!(owned.top[0].addr, "203.0.113.9");
    assert_eq!(owned.top[0].name.as_deref(), Some("www.example.com"));
    assert_eq!(owned.top[1].name, None, "no name was asked for this address");
    assert_eq!(result.networks[1].owner, None, "an address no range covers has no owner, never a guess");
}

#[test]
fn this_network_is_left_out_unless_asked_for() {
    let internet = view(&scenario(), WEEK, AddressScope::Internet);
    let router_bytes: u64 = 10_000;
    let everything = view(&scenario(), WEEK, AddressScope::All);
    let no_owner = everything.networks.iter().find(|node| node.label == "No owner found").unwrap();
    assert_eq!(no_owner.bytes_sent + no_owner.bytes_received, router_bytes + 1007 + 3);
    let without = internet.networks.iter().find(|node| node.label == "No owner found").unwrap();
    assert_eq!(without.bytes_sent + without.bytes_received, 1010, "the router's traffic is not in it");
}

#[test]
fn only_the_hours_in_the_range_count() {
    let result = view(&scenario(), 24 * 3600, AddressScope::Internet);
    let no_owner = result.networks.iter().find(|node| node.label == "No owner found").unwrap();
    assert_eq!(no_owner.bytes_sent + no_owner.bytes_received, 1000, "only the flow of hour 40 is within a day");
    assert_eq!(network_labels(&result), ["No owner found"], "the flows of hour 0 are older than a day");
}

#[test]
fn programs_beyond_the_limit_are_folded_into_one_node_that_says_how_many() {
    let result = connection_map(&scenario(), now(), WEEK, AddressScope::Internet, Some((1, 5))).unwrap();
    assert_eq!(program_labels(&result), ["chrome.exe", "1 other program"]);
    assert_eq!(result.total_programs, 2);
    assert_eq!(result.programs[1].kind, "others");
    assert_eq!(result.programs[1].path, None);
}

#[test]
fn the_number_of_nodes_is_limited() {
    let result = connection_map(&scenario(), now(), WEEK, AddressScope::Internet, Some((1_000, 1_000))).unwrap();
    assert!(result.programs.len() <= MAX_MAP_NODES + 1 && result.networks.len() <= MAX_MAP_NODES + 1);
}

#[test]
fn a_program_node_has_the_path_that_opens_it_and_an_unidentified_one_has_none() {
    let mut repo = InMemoryRepository::default();
    let chrome = start(&mut repo, 10, CHROME);
    let mut f = EventFactory::default();
    let mut known = f.make(at_secs(secs(0, 15)), net_flow(10, SITE, 100, 900, secs(0, 15)));
    known.process_instance_id = Some(chrome.id);
    let unknown = f.make(at_secs(secs(0, 30)), net_flow(99, SITE, 5, 5, secs(0, 30)));
    repo.append(&[known, unknown]).unwrap();
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 100 };
    roll_up_due(&mut repo, at_secs(secs(50, 0)), &settings).unwrap();

    let result = view(&with_dataset(repo), WEEK, AddressScope::Internet);

    let chrome = result.programs.iter().find(|node| node.name == "chrome.exe").unwrap();
    assert_eq!((chrome.kind, chrome.path.as_deref()), ("program", Some(CHROME)));
    let unknown = result.programs.iter().find(|node| node.kind == "unidentified").unwrap();
    assert_eq!((unknown.name.as_str(), unknown.path.as_deref()), ("Unknown program", None));
}

#[test]
fn an_empty_store_gives_an_empty_map_and_describes_no_dataset() {
    let result = view(&InMemoryRepository::default(), WEEK, AddressScope::Internet);
    assert!(result.programs.is_empty() && result.networks.is_empty() && result.links.is_empty());
    assert_eq!(result.owner_data, None);
}

#[test]
fn the_dataset_is_described_when_there_is_one() {
    let data = view(&scenario(), WEEK, AddressScope::Internet).owner_data.expect("described");
    assert_eq!((data.source.as_str(), data.rows), ("iptoasn.com", 1));
}
