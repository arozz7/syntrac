//! Contract suite for destination trends: every `DestinationTrendRepository` (in-memory, SQLite, ...) must pass these, so the two
//! cannot drift apart.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::destination_trend::{
    DestinationProgram, DestinationTotals, NewDestination, NewPair, ProgramDestination, TrafficPair,
};
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
    the_destinations_of_one_program_are_summed_per_address_within_the_range(make());
    the_traffic_of_every_program_to_every_address_comes_back_in_one_read(make());
    the_pairs_first_seen_after_a_time_come_from_the_permanent_ledger(make());
    the_programs_first_online_after_a_time_are_those_whose_earliest_pair_is_that_late(make());
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

fn the_destinations_of_one_program_are_summed_per_address_within_the_range<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);
    let plain = |mut rows: Vec<ProgramDestination>| {
        rows.sort_by_key(|row| row.addr);
        rows.into_iter()
            .map(|row| {
                (
                    row.addr,
                    row.bytes_sent,
                    row.bytes_received,
                    row.flows,
                    row.first_seen.as_micros(),
                    row.last_seen.as_micros(),
                )
            })
            .collect::<Vec<_>>()
    };

    let mut expected = vec![
        (ip(BEACON_TARGET), 150, 250, 2, micros(0, 10), micros(2, 10)),
        (ip(OTHER_TARGET), 1, 1, 1, micros(0, 30), micros(0, 30)),
        (ip(TIED), 2, 2, 1, micros(1, 10), micros(1, 10)),
    ];
    expected.sort_by_key(|row| row.0);
    assert_eq!(
        plain(repo.program_destinations(A_EXE, t(0, 0), t(5, 0)).unwrap()),
        expected,
        "one row per address, loopback left out"
    );
    assert_eq!(
        plain(repo.program_destinations(&A_EXE.to_uppercase(), t(0, 0), t(5, 0)).unwrap()),
        expected,
        "the path is matched without regard to case"
    );

    assert_eq!(
        plain(repo.program_destinations(A_EXE, t(2, 0), t(5, 0)).unwrap()),
        vec![(ip(BEACON_TARGET), 50, 50, 1, micros(2, 10), micros(2, 10))],
        "only the hours in the range count, and first and last are within the range"
    );
    let b = plain(repo.program_destinations(B_EXE, t(0, 0), t(5, 0)).unwrap());
    assert_eq!(b.iter().map(|row| row.0).collect::<Vec<_>>().len(), 2, "B talked to two addresses");
    assert!(repo.program_destinations(r"C:\nowhere\none.exe", t(0, 0), t(5, 0)).unwrap().is_empty());
    assert!(repo.program_destinations("", t(0, 0), t(5, 0)).unwrap().is_empty());
}

fn the_pairs_first_seen_after_a_time_come_from_the_permanent_ledger<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);
    let plain = |mut pairs: Vec<NewPair>| {
        pairs.sort_by(|a, b| a.exe.path.cmp(&b.exe.path).then(a.addr.cmp(&b.addr)));
        pairs.into_iter().map(|p| (p.exe.path, p.addr, p.first_seen.as_micros())).collect::<Vec<_>>()
    };

    let from_hour_one = plain(repo.new_pairs(t(1, 0)).unwrap());
    assert_eq!(
        from_hour_one,
        vec![(A_EXE.to_owned(), ip(TIED), micros(1, 10)), (B_EXE.to_owned(), ip(TIED), micros(1, 10)),],
        "the pairs whose FIRST sighting is at or after the time, with that first sighting"
    );

    let all = plain(repo.new_pairs(t(0, 0)).unwrap());
    assert_eq!(all.len(), 5, "A to three addresses, B to two; nothing for loopback or the unidentified program");
    assert!(all.iter().all(|(_, addr, _)| *addr != ip(Ipv4Addr::LOCALHOST)));
    assert!(repo.new_pairs(t(5, 0)).unwrap().is_empty());
}

/// A talks at hour 0 and again at hour 2; C only at hour 2; D only to loopback at hour 2; B never.
fn the_programs_first_online_after_a_time_are_those_whose_earliest_pair_is_that_late<R: DestinationSubject>(
    mut repo: R,
) {
    let a = start_process(&mut repo, 10, t(0, 0), A_EXE);
    start_process(&mut repo, 11, t(0, 0), B_EXE);
    let c = start_process(&mut repo, 12, t(0, 0), r"C:AppsC.exe");
    let d = start_process(&mut repo, 13, t(0, 0), r"C:AppsD.exe");
    let mut f = EventFactory::default();
    repo.append(&[
        flow_event(&mut f, Some(a.id), BEACON_TARGET, 1, 1, secs(0, 10)),
        flow_event(&mut f, Some(a.id), OTHER_TARGET, 1, 1, secs(2, 10)),
        flow_event(&mut f, Some(c.id), OTHER_TARGET, 1, 1, secs(2, 20)),
        flow_event(&mut f, Some(d.id), Ipv4Addr::LOCALHOST, 1, 1, secs(2, 30)),
    ])
    .unwrap();
    for hour in 0..3 {
        sample_at(&mut repo, hour, 1, 1.0);
    }
    roll_up_due(&mut repo, t(4, 0), &settings(10)).unwrap();
    let paths = |since| {
        let mut paths: Vec<String> = repo.first_online_since(since).unwrap().into_iter().map(|exe| exe.path).collect();
        paths.sort();
        paths
    };

    assert_eq!(
        paths(t(1, 0)),
        vec![r"C:AppsC.exe".to_owned()],
        "A was online at hour 0; loopback is not the network; B never was"
    );
    assert_eq!(paths(t(0, 0)).len(), 2, "A and C, whose first sighting is at or after the time");
    assert!(paths(t(3, 0)).is_empty());
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

fn pair(
    path: Option<&str>,
    addr: Ipv4Addr,
    sent: u64,
    received: u64,
    flows: u64,
) -> (Option<String>, IpAddr, u64, u64, u64) {
    (path.map(str::to_owned), ip(addr), sent, received, flows)
}

fn plain_pairs(rows: Vec<TrafficPair>) -> Vec<(Option<String>, IpAddr, u64, u64, u64)> {
    let mut rows: Vec<_> = rows
        .into_iter()
        .map(|row| (row.exe.map(|exe| exe.path), row.addr, row.bytes_sent, row.bytes_received, row.flows))
        .collect();
    rows.sort();
    rows
}

fn the_traffic_of_every_program_to_every_address_comes_back_in_one_read<R: DestinationSubject>(mut repo: R) {
    scenario(&mut repo);

    let mut expected = vec![
        pair(Some(A_EXE), BEACON_TARGET, 150, 250, 2),
        pair(Some(A_EXE), OTHER_TARGET, 1, 1, 1),
        pair(Some(A_EXE), TIED, 2, 2, 1),
        pair(Some(B_EXE), BEACON_TARGET, 10, 10, 1),
        pair(Some(B_EXE), TIED, 2, 2, 1),
        pair(None, BEACON_TARGET, 3, 0, 1),
        pair(None, UNATTRIBUTED_ONLY, 5, 5, 1),
    ];
    expected.sort();
    assert_eq!(
        plain_pairs(repo.traffic_pairs(t(0, 0), t(5, 0)).unwrap()),
        expected,
        "one row per program and address, the unidentified bucket as no program, loopback left out"
    );
    assert_eq!(
        plain_pairs(repo.traffic_pairs(t(2, 0), t(5, 0)).unwrap()),
        vec![pair(Some(A_EXE), BEACON_TARGET, 50, 50, 1)],
        "only the hours in the range count"
    );
    assert!(repo.traffic_pairs(t(6, 0), t(9, 0)).unwrap().is_empty());
}
