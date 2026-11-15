//! Turning DNS answers into permanent (address, name) pairs. Pure: rows in, rows out.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use syntrac_core::dns_names::{
    address_names, cap_newest, normalize_name, AddressName, DnsAnswerRow, MAX_NAMES_PER_ADDRESS,
};
use syntrac_core::rollup::{roll_up_hour, HourInput, HOUR_MICROS};
use syntrac_core::Timestamp;

const MIN: i64 = 60 * 1_000_000;
/// An exact UTC hour.
const H: i64 = 1_794_571_200_000_000 / HOUR_MICROS * HOUR_MICROS;

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_micros(H + minutes * MIN)
}

fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(a, b, c, d))
}

fn answer(minutes: i64, query: &str, addresses: Vec<IpAddr>) -> DnsAnswerRow {
    DnsAnswerRow { ts: at(minutes), query: query.to_owned(), addresses }
}

fn name(addr: IpAddr, text: &str, first: i64, last: i64) -> AddressName {
    AddressName { addr, name: text.to_owned(), first_seen: at(first), last_seen: at(last) }
}

// ---- names

#[test]
fn a_name_is_lower_cased_and_loses_its_trailing_dot() {
    assert_eq!(normalize_name("Example.COM."), Some("example.com".to_owned()));
    assert_eq!(normalize_name("  www.example.com  "), Some("www.example.com".to_owned()));
}

#[test]
fn an_empty_name_is_no_name() {
    assert_eq!(normalize_name(""), None);
    assert_eq!(normalize_name("   "), None);
    assert_eq!(normalize_name("."), None);
}

// ---- pairs

#[test]
fn one_answer_gives_a_pair_for_each_address() {
    let rows = address_names(&[answer(5, "Example.com", vec![v4(1, 1, 1, 1), v4(2, 2, 2, 2)])]);

    assert_eq!(rows, vec![name(v4(1, 1, 1, 1), "example.com", 5, 5), name(v4(2, 2, 2, 2), "example.com", 5, 5)]);
}

#[test]
fn a_lookup_with_no_answer_gives_nothing() {
    assert!(address_names(&[answer(5, "wpad", vec![])]).is_empty());
}

#[test]
fn a_blank_name_gives_nothing() {
    assert!(address_names(&[answer(5, "  ", vec![v4(1, 1, 1, 1)])]).is_empty());
}

#[test]
fn repeated_lookups_widen_first_and_last_seen() {
    let rows = address_names(&[
        answer(30, "example.com", vec![v4(1, 1, 1, 1)]),
        answer(5, "EXAMPLE.com.", vec![v4(1, 1, 1, 1)]),
        answer(50, "example.com", vec![v4(1, 1, 1, 1)]),
    ]);

    assert_eq!(rows, vec![name(v4(1, 1, 1, 1), "example.com", 5, 50)]);
}

#[test]
fn one_address_keeps_every_name_asked_for() {
    let rows = address_names(&[
        answer(1, "b.example.com", vec![v4(1, 1, 1, 1)]),
        answer(2, "a.example.com", vec![v4(1, 1, 1, 1)]),
    ]);

    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["a.example.com", "b.example.com"], "sorted by address, then name");
}

#[test]
fn an_ipv4_mapped_ipv6_address_is_the_ipv4_address() {
    let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0101, 0x0101));

    let rows = address_names(&[answer(5, "example.com", vec![mapped])]);

    assert_eq!(rows, vec![name(v4(1, 1, 1, 1), "example.com", 5, 5)]);
}

#[test]
fn the_result_does_not_depend_on_the_order_of_the_answers() {
    let a = answer(1, "a.example.com", vec![v4(1, 1, 1, 1), v4(2, 2, 2, 2)]);
    let b = answer(9, "b.example.com", vec![v4(2, 2, 2, 2)]);
    let c = answer(4, "a.example.com", vec![v4(1, 1, 1, 1)]);

    assert_eq!(address_names(&[a.clone(), b.clone(), c.clone()]), address_names(&[c, b, a]),);
}

#[test]
fn writing_the_same_answers_twice_changes_nothing() {
    let answers = [answer(1, "a.example.com", vec![v4(1, 1, 1, 1)]), answer(9, "a.example.com", vec![v4(1, 1, 1, 1)])];
    let once = address_names(&answers);

    let mut doubled = answers.to_vec();
    doubled.extend(answers);

    assert_eq!(address_names(&doubled), once);
}

// ---- the cap

#[test]
fn the_cap_is_fifty_names_per_address() {
    assert_eq!(MAX_NAMES_PER_ADDRESS, 50);
}

#[test]
fn over_the_cap_the_most_recently_seen_names_are_kept() {
    let addr = v4(1, 1, 1, 1);
    let rows: Vec<AddressName> = (0..60).map(|i| name(addr, &format!("host{i:02}.example.com"), i, i)).collect();

    let kept = cap_newest(rows);

    assert_eq!(kept.len(), MAX_NAMES_PER_ADDRESS);
    assert!(kept.iter().all(|r| r.last_seen >= at(10)), "the ten oldest were dropped");
}

#[test]
fn a_tie_on_last_seen_keeps_the_names_that_sort_first() {
    let addr = v4(1, 1, 1, 1);
    let rows: Vec<AddressName> = (0..51).map(|i| name(addr, &format!("host{i:02}.example.com"), 5, 5)).collect();

    let kept = cap_newest(rows);

    assert_eq!(kept.len(), 50);
    assert!(kept.iter().all(|r| r.name != "host50.example.com"));
}

#[test]
fn the_cap_counts_each_address_on_its_own() {
    let mut rows: Vec<AddressName> =
        (0..50).map(|i| name(v4(1, 1, 1, 1), &format!("a{i}.example.com"), i, i)).collect();
    rows.extend((0..50).map(|i| name(v4(2, 2, 2, 2), &format!("b{i}.example.com"), i, i)));

    assert_eq!(cap_newest(rows).len(), 100);
}

// ---- inside the hourly rollup

#[test]
fn an_hour_rollup_carries_the_names_asked_for_in_that_hour() {
    let mut input = HourInput::empty(Timestamp::from_micros(H));
    input.dns = vec![
        answer(5, "example.com", vec![v4(1, 1, 1, 1)]),
        // Outside the hour: a loader that returns too much must not leak another hour in.
        answer(65, "later.example.com", vec![v4(1, 1, 1, 1)]),
        answer(-1, "earlier.example.com", vec![v4(1, 1, 1, 1)]),
    ];

    let rollup = roll_up_hour(&input);

    assert_eq!(rollup.names, vec![name(v4(1, 1, 1, 1), "example.com", 5, 5)]);
}
