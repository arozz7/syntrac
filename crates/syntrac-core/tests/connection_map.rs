//! The connection map (docs/PHASE5-PLAN.md, A2): which programs talked to which networks, from the per-program traffic sums. All pure.

use std::cell::Cell;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::connection_map::{build_map, ConnectionMap, MapLimits, NetworkKind, ProgramKind};
use syntrac_core::destination_trend::{AddressScope, TrafficPair};
use syntrac_core::owner_dataset::Owner;
use syntrac_core::Executable;

fn addr(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, last))
}

fn exe(name: &str) -> Executable {
    Executable { key: name.to_lowercase(), path: format!(r"C:\apps\{name}") }
}

fn pair(program: Option<&str>, last: u8, sent: u64, received: u64) -> TrafficPair {
    TrafficPair { exe: program.map(exe), addr: addr(last), bytes_sent: sent, bytes_received: received, flows: 1 }
}

fn owner(asn: u32, org: &str) -> Owner {
    Owner { asn, country: "US".into(), org: org.into() }
}

/// 203.0.113.1 to .9 belong to AS 100, .10 to .19 to AS 200, .20 to .29 to AS 300, the rest to no one.
fn lookup(address: IpAddr) -> Result<Option<Owner>, Infallible> {
    let IpAddr::V4(v4) = address else { return Ok(None) };
    Ok(match v4.octets()[3] {
        1..=9 => Some(owner(100, "ALPHA")),
        10..=19 => Some(owner(200, "BETA")),
        20..=29 => Some(owner(300, "GAMMA")),
        _ => None,
    })
}

fn limits(programs: usize, networks: usize) -> MapLimits {
    MapLimits { programs, networks }
}

fn map_with(pairs: Vec<TrafficPair>, limits: MapLimits) -> ConnectionMap {
    build_map(pairs, AddressScope::Internet, limits, lookup).unwrap()
}

fn map(pairs: Vec<TrafficPair>) -> ConnectionMap {
    map_with(pairs, limits(10, 10))
}

fn program_names(map: &ConnectionMap) -> Vec<String> {
    map.programs
        .iter()
        .map(|node| match &node.kind {
            ProgramKind::Program(exe) => exe.path.rsplit('\\').next().unwrap().to_owned(),
            ProgramKind::Unidentified => "(unidentified)".to_owned(),
            ProgramKind::Others => "(others)".to_owned(),
        })
        .collect()
}

fn network_asns(map: &ConnectionMap) -> Vec<String> {
    map.networks
        .iter()
        .map(|node| match &node.kind {
            NetworkKind::Owner(owner) => owner.asn.to_string(),
            NetworkKind::NoOwner => "none".to_owned(),
            NetworkKind::Others => "others".to_owned(),
        })
        .collect()
}

/// The (program, network, sent, received) of every link, by name.
fn links(map: &ConnectionMap) -> Vec<(String, String, u64, u64)> {
    let programs = program_names(map);
    let networks = network_asns(map);
    let mut found: Vec<_> = map
        .links
        .iter()
        .map(|link| {
            (programs[link.program].clone(), networks[link.network].clone(), link.bytes_sent, link.bytes_received)
        })
        .collect();
    found.sort();
    found
}

fn link(program: &str, network: &str, sent: u64, received: u64) -> (String, String, u64, u64) {
    (program.to_owned(), network.to_owned(), sent, received)
}

#[test]
fn nothing_in_gives_an_empty_map() {
    let result = map(vec![]);
    assert!(result.programs.is_empty() && result.networks.is_empty() && result.links.is_empty());
    assert_eq!((result.total_programs, result.total_networks), (0, 0));
}

#[test]
fn a_program_and_the_network_it_talked_to_are_joined_by_a_link_of_its_traffic() {
    let result = map(vec![pair(Some("a.exe"), 1, 10, 90)]);
    assert_eq!(program_names(&result), ["a.exe"]);
    assert_eq!(network_asns(&result), ["100"]);
    assert_eq!(links(&result), [link("a.exe", "100", 10, 90)]);
    assert_eq!((result.programs[0].bytes_sent, result.programs[0].bytes_received), (10, 90));
}

#[test]
fn addresses_of_one_owner_are_one_network_and_are_counted() {
    let result = map(vec![pair(Some("a.exe"), 1, 10, 0), pair(Some("a.exe"), 2, 5, 5), pair(Some("b.exe"), 3, 1, 1)]);
    assert_eq!(network_asns(&result), ["100"]);
    assert_eq!(result.networks[0].addresses, 3);
    assert_eq!(links(&result), [link("a.exe", "100", 15, 5), link("b.exe", "100", 1, 1)]);
    match &result.networks[0].kind {
        NetworkKind::Owner(owner) => assert_eq!(owner.org, "ALPHA"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn programs_and_networks_are_ranked_by_bytes_with_ties_broken_by_name_and_as_number() {
    let result = map(vec![
        pair(Some("small.exe"), 11, 1, 1),
        pair(Some("big.exe"), 1, 100, 0),
        pair(Some("b-tie.exe"), 21, 5, 5),
        pair(Some("a-tie.exe"), 21, 5, 5),
    ]);
    assert_eq!(program_names(&result), ["big.exe", "a-tie.exe", "b-tie.exe", "small.exe"]);
    assert_eq!(network_asns(&result), ["100", "300", "200"]);
}

#[test]
fn programs_beyond_the_limit_become_one_others_node_that_keeps_their_links() {
    let result = map_with(
        vec![
            pair(Some("a.exe"), 1, 100, 0),
            pair(Some("b.exe"), 11, 50, 0),
            pair(Some("c.exe"), 1, 4, 0),
            pair(Some("d.exe"), 1, 3, 0),
            pair(Some("e.exe"), 11, 2, 0),
        ],
        limits(2, 10),
    );
    assert_eq!(program_names(&result), ["a.exe", "b.exe", "(others)"]);
    assert_eq!(result.programs[2].programs, 3, "c, d and e");
    assert_eq!((result.programs[2].bytes_sent, result.programs[2].bytes_received), (9, 0));
    assert_eq!(
        links(&result),
        [
            link("(others)", "100", 7, 0),
            link("(others)", "200", 2, 0),
            link("a.exe", "100", 100, 0),
            link("b.exe", "200", 50, 0)
        ],
        "two others to the same network are one link"
    );
    assert_eq!(result.total_programs, 5);
}

#[test]
fn networks_beyond_the_limit_become_one_others_node() {
    let result = map_with(
        vec![
            pair(Some("a.exe"), 1, 100, 0),
            pair(Some("a.exe"), 11, 50, 0),
            pair(Some("a.exe"), 21, 4, 0),
            pair(Some("a.exe"), 22, 3, 0),
        ],
        limits(10, 1),
    );
    assert_eq!(network_asns(&result), ["100", "others"]);
    assert_eq!(result.networks[1].networks, 2, "AS 200 and AS 300");
    assert_eq!(result.networks[1].addresses, 3);
    assert_eq!(links(&result), [link("a.exe", "100", 100, 0), link("a.exe", "others", 57, 0)]);
    assert_eq!(result.total_networks, 3);
}

#[test]
fn no_others_node_appears_when_everything_fits() {
    let result = map_with(vec![pair(Some("a.exe"), 1, 1, 1), pair(Some("b.exe"), 11, 1, 1)], limits(2, 2));
    assert!(!program_names(&result).contains(&"(others)".to_owned()));
    assert!(!network_asns(&result).contains(&"others".to_owned()));
}

#[test]
fn an_unidentified_program_is_a_node_of_its_own() {
    let result = map(vec![pair(None, 1, 10, 10), pair(Some("a.exe"), 1, 1, 1)]);
    assert_eq!(program_names(&result), ["(unidentified)", "a.exe"]);
}

#[test]
fn addresses_with_no_owner_are_one_network_and_are_never_guessed() {
    let result = map(vec![pair(Some("a.exe"), 100, 10, 0), pair(Some("a.exe"), 101, 10, 0)]);
    assert_eq!(network_asns(&result), ["none"]);
    assert_eq!(result.networks[0].addresses, 2);
}

#[test]
fn this_network_is_left_out_unless_asked_for_and_then_it_is_not_looked_up() {
    let local = TrafficPair {
        exe: Some(exe("a.exe")),
        addr: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
        bytes_sent: 7,
        bytes_received: 7,
        flows: 1,
    };
    let internet = pair(Some("a.exe"), 1, 1, 1);

    let only_internet =
        build_map(vec![local.clone(), internet.clone()], AddressScope::Internet, limits(10, 10), lookup).unwrap();
    assert_eq!(network_asns(&only_internet), ["100"]);

    let looked_up = Cell::new(0);
    let everything = build_map(vec![local, internet], AddressScope::All, limits(10, 10), |address| {
        looked_up.set(looked_up.get() + 1);
        lookup(address)
    })
    .unwrap();
    assert_eq!(network_asns(&everything), ["none", "100"]);
    assert_eq!(looked_up.get(), 1, "only the internet address is looked up");
}

#[test]
fn an_address_is_looked_up_once_however_many_programs_used_it() {
    let looked_up = Cell::new(0);
    let pairs = vec![pair(Some("a.exe"), 1, 1, 1), pair(Some("b.exe"), 1, 1, 1), pair(Some("c.exe"), 1, 1, 1)];
    build_map(pairs, AddressScope::Internet, limits(10, 10), |address| {
        looked_up.set(looked_up.get() + 1);
        lookup(address)
    })
    .unwrap();
    assert_eq!(looked_up.get(), 1);
}

#[test]
fn a_pair_with_no_bytes_is_left_out() {
    let result = map(vec![pair(Some("quiet.exe"), 1, 0, 0), pair(Some("a.exe"), 1, 1, 0)]);
    assert_eq!(program_names(&result), ["a.exe"]);
    assert_eq!(result.total_programs, 1);
}

#[test]
fn the_links_always_add_up_to_what_was_given_however_much_is_cut() {
    let pairs: Vec<TrafficPair> = (1..=25u8)
        .flat_map(|last| ["a.exe", "b.exe", "c.exe", "d.exe"].map(|name| pair(Some(name), last, u64::from(last), 1)))
        .collect();
    let given: u64 = pairs.iter().map(|p| p.bytes_sent + p.bytes_received).sum();

    let result = map_with(pairs, limits(2, 2));

    let linked: u64 = result.links.iter().map(|l| l.bytes_sent + l.bytes_received).sum();
    let in_programs: u64 = result.programs.iter().map(|n| n.bytes_sent + n.bytes_received).sum();
    let in_networks: u64 = result.networks.iter().map(|n| n.bytes_sent + n.bytes_received).sum();
    assert_eq!((linked, in_programs, in_networks), (given, given, given));
}

#[test]
fn a_failed_lookup_is_reported() {
    let failed: Result<ConnectionMap, &str> =
        build_map(vec![pair(Some("a.exe"), 1, 1, 1)], AddressScope::Internet, limits(10, 10), |_| {
            Err("store read failed")
        });
    assert_eq!(failed.unwrap_err(), "store read failed");
}

#[test]
fn a_zero_limit_still_shows_something_rather_than_nothing() {
    let result = map_with(vec![pair(Some("a.exe"), 1, 1, 1), pair(Some("b.exe"), 11, 1, 1)], limits(0, 0));
    assert_eq!(program_names(&result), ["a.exe", "(others)"], "the limit is at least one");
    assert_eq!(network_asns(&result), ["100", "others"]);
}

#[test]
fn a_network_names_its_biggest_addresses_so_one_can_be_opened() {
    let mut pairs = vec![pair(Some("a.exe"), 1, 100, 0), pair(Some("a.exe"), 2, 50, 0), pair(Some("b.exe"), 2, 30, 0)];
    pairs.extend((3..=9).map(|last| pair(Some("a.exe"), last, 1, 0)));

    let result = map(pairs);

    let top: Vec<IpAddr> = result.networks[0].top.clone();
    assert_eq!(top.len(), 5, "at most five, so the map stays small");
    assert_eq!(&top[..2], &[addr(1), addr(2)], "biggest first, an address's programs added together");
    assert_eq!(result.networks[0].addresses, 9);
}

#[test]
fn the_others_network_names_no_addresses() {
    let result = map_with(vec![pair(Some("a.exe"), 1, 100, 0), pair(Some("a.exe"), 11, 1, 0)], limits(10, 1));
    assert!(result.networks[1].top.is_empty());
}
