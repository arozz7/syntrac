//! Contract suite for the permanent address-to-name table: the in-memory and SQLite repositories must pass these.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::dns_names::{backfill_names, MAX_NAMES_PER_ADDRESS};
use syntrac_core::rollup::roll_up_hour;
use syntrac_core::{roll_up_due, AddressName, EventRepository};

use crate::fixtures::{dns_query, EventFactory};
use crate::rollup_contract::{secs, settings, t, RollupSubject};

const SHARED: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 10);
const OTHER: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 20);

fn ip(addr: Ipv4Addr) -> IpAddr {
    IpAddr::V4(addr)
}

pub fn names_contract<R: RollupSubject>(make: impl Fn() -> R) {
    a_rolled_up_hour_stores_its_names_and_they_survive_a_purge(make());
    names_come_newest_first_and_widen_over_hours(make());
    a_lookup_with_no_answer_stores_nothing(make());
    one_address_keeps_at_most_fifty_names(make());
    load_dns_answers_is_half_open_and_oldest_first(make());
    the_backfill_stores_names_of_hours_already_rolled_up(make());
    the_backfill_runs_once(make());
    the_backfill_on_a_database_without_detail_is_marked_done(make());
    the_backfill_and_the_hourly_rollup_agree(make());
}

fn lookup<R: EventRepository>(repo: &mut R, f: &mut EventFactory, at_secs: i64, name: &str, addr: Ipv4Addr) {
    let ts = syntrac_core::Timestamp::from_micros(at_secs * crate::fixtures::SEC);
    repo.append(&[f.make(ts, dns_query(10, name, &[addr]))]).unwrap();
}

fn name_of(rows: &[AddressName]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

fn a_rolled_up_hour_stores_its_names_and_they_survive_a_purge<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    lookup(&mut repo, &mut f, secs(0, 5), "Example.com.", SHARED);
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    repo.purge_before(t(2, 0)).unwrap();

    let rows = repo.address_names(ip(SHARED)).unwrap();
    assert_eq!(name_of(&rows), ["example.com"]);
    assert_eq!((rows[0].first_seen, rows[0].last_seen), (t(0, 5), t(0, 5)));
    assert!(repo.address_names(ip(OTHER)).unwrap().is_empty());
}

fn names_come_newest_first_and_widen_over_hours<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    lookup(&mut repo, &mut f, secs(0, 5), "old.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(0, 6), "both.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(1, 30), "both.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(1, 40), "new.example.com", SHARED);
    roll_up_due(&mut repo, t(3, 0), &settings(10)).unwrap();

    let rows = repo.address_names(ip(SHARED)).unwrap();

    assert_eq!(name_of(&rows), ["new.example.com", "both.example.com", "old.example.com"]);
    let both = &rows[1];
    assert_eq!((both.first_seen, both.last_seen), (t(0, 6), t(1, 30)), "first from hour 0, last from hour 1");
}

fn a_lookup_with_no_answer_stores_nothing<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    let ts = t(0, 5);
    repo.append(&[f.make(ts, dns_query(10, "wpad", &[]))]).unwrap();
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    assert!(repo.address_names(ip(SHARED)).unwrap().is_empty());
}

fn one_address_keeps_at_most_fifty_names<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    for i in 0..60 {
        lookup(&mut repo, &mut f, secs(0, 0) + i, &format!("host{i:02}.example.com"), SHARED);
    }
    roll_up_due(&mut repo, t(2, 0), &settings(4)).unwrap();

    let rows = repo.address_names(ip(SHARED)).unwrap();

    assert_eq!(rows.len(), MAX_NAMES_PER_ADDRESS);
    assert_eq!(rows[0].name, "host59.example.com", "the newest is kept");
    assert!(rows.iter().all(|r| r.name.as_str() >= "host10"), "the ten oldest were dropped");
}

fn load_dns_answers_is_half_open_and_oldest_first<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    lookup(&mut repo, &mut f, secs(0, 30), "late.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(0, 0), "start.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(1, 0), "next-hour.example.com", SHARED);

    let rows = repo.load_dns_answers(t(0, 0), t(1, 0)).unwrap();

    let names: Vec<&str> = rows.iter().map(|r| r.query.as_str()).collect();
    assert_eq!(names, ["start.example.com", "late.example.com"], "the end is exclusive");
    assert_eq!(rows[0].addresses, vec![ip(SHARED)]);
}

/// Rolls an hour up WITHOUT its names, as every hour was rolled before names were collected.
fn roll_hour_without_names<R: RollupSubject>(repo: &mut R, hour: i64) {
    let mut input = repo.load_hour(t(hour, 0)).unwrap();
    input.dns.clear();
    repo.store_hour(&roll_up_hour(&input)).unwrap();
}

fn the_backfill_stores_names_of_hours_already_rolled_up<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    lookup(&mut repo, &mut f, secs(0, 5), "a.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(1, 5), "b.example.com", OTHER);
    roll_hour_without_names(&mut repo, 0);
    roll_hour_without_names(&mut repo, 1);
    assert!(repo.address_names(ip(SHARED)).unwrap().is_empty(), "rolled before names were collected");

    let done = backfill_names(&mut repo, t(2, 30)).unwrap();

    assert!(done.ran);
    assert_eq!(done.names, 2);
    assert_eq!(name_of(&repo.address_names(ip(SHARED)).unwrap()), ["a.example.com"]);
    assert_eq!(name_of(&repo.address_names(ip(OTHER)).unwrap()), ["b.example.com"]);
    assert!(repo.names_backfilled().unwrap());
}

fn the_backfill_runs_once<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    lookup(&mut repo, &mut f, secs(0, 5), "a.example.com", SHARED);
    assert!(backfill_names(&mut repo, t(1, 30)).unwrap().ran);
    lookup(&mut repo, &mut f, secs(1, 10), "later.example.com", SHARED);

    let again = backfill_names(&mut repo, t(1, 30)).unwrap();

    assert!(!again.ran, "already done: nothing is read or written");
    assert_eq!(name_of(&repo.address_names(ip(SHARED)).unwrap()), ["a.example.com"]);
}

fn the_backfill_on_a_database_without_detail_is_marked_done<R: RollupSubject>(mut repo: R) {
    let done = backfill_names(&mut repo, t(5, 0)).unwrap();

    assert!(done.ran);
    assert_eq!(done.names, 0);
    assert!(repo.names_backfilled().unwrap());
}

fn the_backfill_and_the_hourly_rollup_agree<R: RollupSubject>(mut repo: R) {
    let mut f = EventFactory::default();
    lookup(&mut repo, &mut f, secs(0, 5), "a.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(1, 5), "a.example.com", SHARED);
    lookup(&mut repo, &mut f, secs(1, 6), "b.example.com", SHARED);
    roll_up_due(&mut repo, t(2, 30), &settings(10)).unwrap();
    let rolled = repo.address_names(ip(SHARED)).unwrap();

    backfill_names(&mut repo, t(2, 30)).unwrap();

    assert_eq!(repo.address_names(ip(SHARED)).unwrap(), rolled, "storing the same names again changes nothing");
}
