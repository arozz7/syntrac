//! Contract suite for destination trends: every `DestinationTrendRepository` (in-memory, SQLite, ...) must pass these, so the two
//! cannot drift apart.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::destination_trend::{DestinationProgram, DestinationTotals, NewDestination};
use syntrac_core::{
    roll_up_due, DestinationTrendRepository, EventRepository, MetricsRepository, ProcessRepository, Retention,
    RollupRepository,
};

use crate::fixtures::{EventFactory, BEACON_TARGET};
use crate::rollup_contract::{flow_event, sample_at, secs, settings, start_process, t, A_EXE, B_EXE, OTHER_TARGET};

pub trait DestinationSubject:
    DestinationTrendRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}
impl<T> DestinationSubject for T where
    T: DestinationTrendRepository
        + RollupRepository
        + EventRepository
        + ProcessRepository
        + MetricsRepository
        + Retention
{
}

const UNATTRIBUTED_ONLY: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 77);
const TIED: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 88);

pub fn destination_contract<R: DestinationSubject>(make: impl Fn() -> R) {
    an_empty_database_has_no_destinations(make());
    totals_add_up_programs_and_leave_out_loopback(make());
    only_the_hours_in_the_range_count(make());
    new_destinations_come_from_the_permanent_ledger_with_the_first_program(make());
    new_destinations_survive_the_purge_of_detail(make());
    the_programs_of_one_destination_are_summed_within_the_range(make());
    the_programs_of_many_destinations_come_back_in_one_read(make());
}

fn ip(addr: Ipv4Addr) -> IpAddr {
    IpAddr::V4(addr)
}

/// Hour 0: A and B and an unidentified program talk to BEACON, A to OTHER, A to loopback. Hour 1: an unidentified program to
/// UNATTRIBUTED_ONLY, and A and B at the same second to TIED. Hour 2: A to BEACON again.
fn scenario<R: DestinationSubject>(repo: &mut R) {
    let a = start_process(repo, 10, t(0, 0), A_EXE);
    let b = start_process(repo, 11, t(0, 0), B_EXE);
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 100, 200, secs(0, 10)),
        flow_event(&mut f, Some(b.id), BEACON_TARGET, 10, 10, secs(0, 20)),
        flow_event(&mut f, Some(a.id), OTHER_TARGET, 1, 1, secs(0, 30)),
        flow_event(&mut f, None, BEACON_TARGET, 3, 0, secs(0, 40)),
        flow_event(&mut f, Some(a.id), Ipv4Addr::LOCALHOST, 999, 999, secs(0, 50)),
        flow_event(&mut f, None, UNATTRIBUTED_ONLY, 5, 5, secs(1, 5)),
        flow_event(&mut f, Some(b.id), TIED, 2, 2, secs(1, 10)),
        flow_event(&mut f, Some(a.id), TIED, 2, 2, secs(1, 10)),
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 50, 50, secs(2, 10)),
    ])
    .unwrap();
    for hour in 0..3 {
        sample_at(repo, hour, 1, 1.0);
    }
    roll_up_due(repo, t(4, 0), &settings(10)).unwrap();
}

fn sorted_totals(mut rows: Vec<DestinationTotals>) -> Vec<DestinationTotals> {
    rows.sort_by_key(|row| row.addr);
    rows
}

fn totals(addr: Ipv4Addr, sent: u64, received: u64, flows: u64, programs: u32) -> DestinationTotals {
    DestinationTotals { addr: ip(addr), bytes_sent: sent, bytes_received: received, flows, programs }
}

fn an_empty_database_has_no_destinations<R: DestinationSubject>(repo: R) {
    assert!(repo.destination_totals(t(0, 0), t(9, 0)).unwrap().is_empty());
    assert!(repo.new_destinations(t(0, 0)).unwrap().is_empty());
    assert!(repo.destination_programs(ip(BEACON_TARGET), t(0, 0), t(9, 0)).unwrap().is_empty());
}

fn totals_add_up_programs_and_leave_out_loopback<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);

    let rows = sorted_totals(repo.destination_totals(t(0, 0), t(5, 0)).unwrap());

    let expected = sorted_totals(vec![
        // BEACON: A (two hours), B and the unidentified bucket are three programs.
        totals(BEACON_TARGET, 163, 260, 4, 3),
        totals(OTHER_TARGET, 1, 1, 1, 1),
        totals(UNATTRIBUTED_ONLY, 5, 5, 1, 1),
        totals(TIED, 4, 4, 2, 2),
    ]);
    assert_eq!(rows, expected, "and no row for 127.0.0.1");
}

fn only_the_hours_in_the_range_count<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);

    let rows = sorted_totals(repo.destination_totals(t(2, 0), t(5, 0)).unwrap());

    assert_eq!(rows, vec![totals(BEACON_TARGET, 50, 50, 1, 1)]);
    assert!(repo.destination_totals(t(6, 0), t(9, 0)).unwrap().is_empty());
}

fn by_addr(mut rows: Vec<NewDestination>) -> Vec<NewDestination> {
    rows.sort_by_key(|row| row.addr);
    rows
}

fn first_program_path(row: &NewDestination) -> Option<&str> {
    row.first_program.as_ref().map(|exe| exe.path.as_str())
}

fn new_destinations_come_from_the_permanent_ledger_with_the_first_program<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);

    let everything = by_addr(repo.new_destinations(t(0, 0)).unwrap());
    let addresses: Vec<IpAddr> = everything.iter().map(|row| row.addr).collect();
    let mut expected = vec![ip(BEACON_TARGET), ip(OTHER_TARGET), ip(UNATTRIBUTED_ONLY), ip(TIED)];
    expected.sort();
    assert_eq!(addresses, expected, "loopback is never a destination");

    let beacon = everything.iter().find(|row| row.addr == ip(BEACON_TARGET)).unwrap();
    assert_eq!((beacon.first_seen, beacon.last_seen), (t(0, 10), t(2, 10)));
    assert_eq!(first_program_path(beacon), Some(A_EXE), "A was first, at 0:10; B came at 0:20");

    let unattributed = everything.iter().find(|row| row.addr == ip(UNATTRIBUTED_ONLY)).unwrap();
    assert_eq!(first_program_path(unattributed), None, "only an unidentified program ever talked to it");

    let tied = everything.iter().find(|row| row.addr == ip(TIED)).unwrap();
    assert_eq!(first_program_path(tied), Some(A_EXE), "same second: the path that sorts first");

    // since is inclusive and compares the FIRST sighting, not the last.
    let later = by_addr(repo.new_destinations(t(0, 30)).unwrap());
    let later_addresses: Vec<IpAddr> = later.iter().map(|row| row.addr).collect();
    let mut expected_later = vec![ip(OTHER_TARGET), ip(UNATTRIBUTED_ONLY), ip(TIED)];
    expected_later.sort();
    assert_eq!(later_addresses, expected_later, "BEACON was first seen at 0:10 even though it was seen again at 2:10");
}

fn new_destinations_survive_the_purge_of_detail<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);
    let before = by_addr(repo.new_destinations(t(0, 0)).unwrap());

    repo.purge_before(t(100, 0)).unwrap();

    assert_eq!(by_addr(repo.new_destinations(t(0, 0)).unwrap()), before);
}

fn program(
    path: Option<&str>,
    sent: u64,
    received: u64,
    flows: u64,
    first: i64,
    last: i64,
) -> (Option<String>, u64, u64, u64, i64, i64) {
    (path.map(str::to_owned), sent, received, flows, first, last)
}

fn plain(rows: Vec<DestinationProgram>) -> Vec<(Option<String>, u64, u64, u64, i64, i64)> {
    let mut rows: Vec<_> = rows
        .into_iter()
        .map(|row| {
            (
                row.exe.map(|exe| exe.path),
                row.bytes_sent,
                row.bytes_received,
                row.flows,
                row.first_seen.as_micros(),
                row.last_seen.as_micros(),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn micros(hour: i64, minutes: i64) -> i64 {
    t(hour, minutes).as_micros()
}

fn the_programs_of_one_destination_are_summed_within_the_range<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);

    let all = plain(repo.destination_programs(ip(BEACON_TARGET), t(0, 0), t(5, 0)).unwrap());
    let mut expected = vec![
        program(Some(A_EXE), 150, 250, 2, micros(0, 10), micros(2, 10)),
        program(Some(B_EXE), 10, 10, 1, micros(0, 20), micros(0, 20)),
        program(None, 3, 0, 1, micros(0, 40), micros(0, 40)),
    ];
    expected.sort();
    assert_eq!(all, expected);

    let hour_two = plain(repo.destination_programs(ip(BEACON_TARGET), t(2, 0), t(5, 0)).unwrap());
    assert_eq!(
        hour_two,
        vec![program(Some(A_EXE), 50, 50, 1, micros(2, 10), micros(2, 10))],
        "first and last are within the range"
    );
}

fn the_programs_of_many_destinations_come_back_in_one_read<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);
    let asked = [ip(BEACON_TARGET), ip(TIED), ip(Ipv4Addr::new(192, 0, 2, 200))];

    let rows = repo.programs_of_destinations(&asked, t(0, 0), t(5, 0)).unwrap();

    let mut got: Vec<(IpAddr, Option<String>, u64)> = rows
        .into_iter()
        .map(|(addr, row)| (addr, row.exe.map(|exe| exe.path), row.bytes_sent + row.bytes_received))
        .collect();
    got.sort();
    let mut expected = vec![
        (ip(BEACON_TARGET), Some(A_EXE.to_owned()), 400),
        (ip(BEACON_TARGET), Some(B_EXE.to_owned()), 20),
        (ip(BEACON_TARGET), None, 3),
        (ip(TIED), Some(A_EXE.to_owned()), 4),
        (ip(TIED), Some(B_EXE.to_owned()), 4),
    ];
    expected.sort();
    assert_eq!(got, expected, "only the addresses asked for, and none for an address nobody talked to");

    let narrow = repo.programs_of_destinations(&[ip(BEACON_TARGET)], t(2, 0), t(5, 0)).unwrap();
    assert_eq!(narrow.len(), 1, "only the hours in the range count");
    assert!(
        repo.programs_of_destinations(&[], t(0, 0), t(5, 0)).unwrap().is_empty(),
        "asking for nothing reads nothing"
    );
}
