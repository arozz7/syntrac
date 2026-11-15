//! Ranking destinations and deciding which are "new". Pure: rows in, rows out.

use std::net::IpAddr;

use syntrac_core::destination_trend::{
    new_destination_cutoff, rank_destination_programs, rank_destinations, select_new, AddressScope, DestinationOrder,
    DestinationProgram, DestinationTotals, NewDestination, BASELINE_HOURS,
};
use syntrac_core::rollup::HOUR_MICROS;
use syntrac_core::{Executable, Timestamp};

fn addr(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn totals(text: &str, sent: u64, received: u64, flows: u64, programs: u32) -> DestinationTotals {
    DestinationTotals { addr: addr(text), bytes_sent: sent, bytes_received: received, flows, programs }
}

fn order_of(rows: &[DestinationTotals]) -> Vec<String> {
    rows.iter().map(|r| r.addr.to_string()).collect()
}

// ---- ranking

#[test]
fn destinations_are_ranked_by_bytes_sent_plus_received_highest_first() {
    let rows =
        vec![totals("8.8.4.4", 100, 100, 1, 1), totals("1.1.1.1", 10, 500, 1, 1), totals("9.9.9.9", 400, 50, 1, 1)];

    let ranked = rank_destinations(rows, DestinationOrder::Bytes, AddressScope::All, 10);

    assert_eq!(order_of(&ranked), ["1.1.1.1", "9.9.9.9", "8.8.4.4"]);
}

#[test]
fn destinations_can_be_ranked_by_flows_or_by_number_of_programs() {
    let rows = vec![totals("8.8.4.4", 1, 1, 5, 3), totals("1.1.1.1", 9, 9, 9, 1), totals("9.9.9.9", 5, 5, 2, 2)];

    let by_flows = rank_destinations(rows.clone(), DestinationOrder::Flows, AddressScope::All, 10);
    let by_programs = rank_destinations(rows, DestinationOrder::Programs, AddressScope::All, 10);

    assert_eq!(order_of(&by_flows), ["1.1.1.1", "8.8.4.4", "9.9.9.9"]);
    assert_eq!(order_of(&by_programs), ["8.8.4.4", "9.9.9.9", "1.1.1.1"]);
}

#[test]
fn a_tie_is_broken_by_address_so_the_order_never_depends_on_the_store() {
    let rows = vec![totals("9.9.9.9", 10, 10, 1, 1), totals("1.1.1.1", 10, 10, 1, 1), totals("5.5.5.5", 10, 10, 1, 1)];

    let ranked = rank_destinations(rows, DestinationOrder::Bytes, AddressScope::All, 10);

    assert_eq!(order_of(&ranked), ["1.1.1.1", "5.5.5.5", "9.9.9.9"]);
}

#[test]
fn the_internet_scope_leaves_out_private_link_local_and_multicast_addresses() {
    let rows = vec![
        totals("192.168.1.1", 1000, 1000, 9, 1),
        totals("224.0.0.251", 900, 900, 9, 1),
        totals("fe80::1", 800, 800, 9, 1),
        totals("8.8.8.8", 1, 1, 1, 1),
    ];

    let internet = rank_destinations(rows.clone(), DestinationOrder::Bytes, AddressScope::Internet, 10);
    let all = rank_destinations(rows, DestinationOrder::Bytes, AddressScope::All, 10);

    assert_eq!(order_of(&internet), ["8.8.8.8"]);
    assert_eq!(all.len(), 4);
}

#[test]
fn the_limit_applies_after_the_scope_so_hidden_addresses_do_not_use_up_rows() {
    let rows =
        vec![totals("192.168.1.1", 1000, 0, 1, 1), totals("8.8.8.8", 10, 0, 1, 1), totals("1.1.1.1", 5, 0, 1, 1)];

    let ranked = rank_destinations(rows, DestinationOrder::Bytes, AddressScope::Internet, 2);

    assert_eq!(order_of(&ranked), ["8.8.8.8", "1.1.1.1"]);
}

#[test]
fn an_address_with_nothing_for_the_order_is_left_out() {
    let rows = vec![totals("8.8.8.8", 0, 0, 0, 0), totals("1.1.1.1", 1, 0, 1, 1)];

    assert_eq!(order_of(&rank_destinations(rows, DestinationOrder::Bytes, AddressScope::All, 10)), ["1.1.1.1"]);
}

#[test]
fn byte_totals_saturate_instead_of_wrapping() {
    let rows = vec![totals("1.1.1.1", u64::MAX, u64::MAX, 1, 1), totals("8.8.8.8", 5, 5, 1, 1)];

    let ranked = rank_destinations(rows, DestinationOrder::Bytes, AddressScope::All, 10);

    assert_eq!(order_of(&ranked), ["1.1.1.1", "8.8.8.8"]);
}

#[test]
fn an_empty_range_ranks_nothing() {
    assert!(rank_destinations(Vec::new(), DestinationOrder::Bytes, AddressScope::All, 10).is_empty());
}

// ---- who talked to it

fn program(path: Option<&str>, sent: u64, received: u64) -> DestinationProgram {
    DestinationProgram {
        exe: path.map(|p| Executable { key: p.to_lowercase(), path: p.to_owned() }),
        bytes_sent: sent,
        bytes_received: received,
        flows: 1,
        first_seen: Timestamp::from_micros(1),
        last_seen: Timestamp::from_micros(2),
    }
}

#[test]
fn the_programs_that_talked_to_an_address_are_ranked_by_bytes_with_the_unattributed_row_last_on_a_tie() {
    let rows = vec![
        program(None, 10, 10),
        program(Some(r"C:\b.exe"), 10, 10),
        program(Some(r"C:\a.exe"), 10, 10),
        program(Some(r"C:\big.exe"), 500, 0),
    ];

    let ranked = rank_destination_programs(rows);

    let names: Vec<Option<&str>> = ranked.iter().map(|p| p.exe.as_ref().map(|e| e.path.as_str())).collect();
    assert_eq!(names, [Some(r"C:\big.exe"), Some(r"C:\a.exe"), Some(r"C:\b.exe"), None]);
}

// ---- what is new

const H: i64 = 1_794_571_200_000_000 / HOUR_MICROS * HOUR_MICROS;

fn at_hours(hours: i64) -> Timestamp {
    Timestamp::from_micros(H + hours * HOUR_MICROS)
}

fn new_destination(text: &str, first_hour: i64) -> NewDestination {
    NewDestination {
        addr: addr(text),
        first_seen: at_hours(first_hour),
        last_seen: at_hours(first_hour + 1),
        first_program: Some(Executable { key: r"c:\a.exe".into(), path: r"C:\a.exe".into() }),
    }
}

#[test]
fn the_first_day_of_history_is_the_baseline_not_news() {
    assert_eq!(BASELINE_HOURS, 24);
    // History began at hour 0; the user asked for "the last 7 days", which reaches back before it.
    let cutoff = new_destination_cutoff(Some(at_hours(0)), at_hours(-100));
    assert_eq!(cutoff, at_hours(24));
}

#[test]
fn a_window_that_starts_after_the_baseline_is_used_as_asked() {
    let cutoff = new_destination_cutoff(Some(at_hours(0)), at_hours(100));
    assert_eq!(cutoff, at_hours(100));
}

#[test]
fn without_any_history_nothing_is_new() {
    // No rollups yet: the cutoff is the end of time, so nothing can be at or after it.
    let cutoff = new_destination_cutoff(None, at_hours(0));
    assert!(select_new(vec![new_destination("8.8.8.8", 5)], cutoff, AddressScope::All, 10).is_empty());
}

#[test]
fn new_destinations_are_those_first_seen_at_or_after_the_cutoff_newest_first() {
    let rows = vec![
        new_destination("1.1.1.1", 23),
        new_destination("2.2.2.2", 24),
        new_destination("3.3.3.3", 50),
        new_destination("4.4.4.4", 30),
    ];

    let news = select_new(rows, at_hours(24), AddressScope::All, 10);

    let order: Vec<String> = news.iter().map(|n| n.addr.to_string()).collect();
    assert_eq!(order, ["3.3.3.3", "4.4.4.4", "2.2.2.2"], "the one at hour 23 is baseline; at hour 24 exactly is new");
}

#[test]
fn new_destinations_tie_on_time_by_address_and_respect_scope_and_limit() {
    let rows = vec![
        new_destination("9.9.9.9", 30),
        new_destination("192.168.1.9", 30),
        new_destination("1.1.1.1", 30),
        new_destination("5.5.5.5", 30),
    ];

    let news = select_new(rows, at_hours(24), AddressScope::Internet, 2);

    let order: Vec<String> = news.iter().map(|n| n.addr.to_string()).collect();
    assert_eq!(order, ["1.1.1.1", "5.5.5.5"]);
}
