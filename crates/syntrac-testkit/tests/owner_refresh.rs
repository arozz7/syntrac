//! Refreshing the owner dataset: a good file replaces the old one, a bad one or a failed download changes nothing.

use syntrac_core::{parse_ip2asn, refresh_owner_data, OwnerRefreshError, OwnerRepository, RefreshLimits, Timestamp};
use syntrac_testkit::{FakeDatasetFetcher, InMemoryRepository};

fn file(blocks: u32) -> String {
    (0..blocks).map(|n| format!("30.0.{n}.0\t30.0.{n}.255\t{}\tUS\tOwner {n}\n", 100 + n)).collect()
}

fn limits() -> RefreshLimits {
    RefreshLimits { min_ranges: 10, max_bad_percent: 5 }
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

#[test]
fn a_good_file_is_stored_with_where_and_when_it_came_from() {
    let mut repo = InMemoryRepository::default();
    let fetcher = FakeDatasetFetcher::new(&file(50));

    let outcome = refresh_owner_data(&fetcher, &mut repo, at(5_000), &limits()).unwrap();

    assert_eq!((outcome.rows, outcome.bad_lines, outcome.unrouted), (50, 0, 0));
    let info = repo.dataset_info().unwrap().unwrap();
    assert_eq!((info.rows, info.downloaded_at, info.source.as_str()), (50, at(5_000), "iptoasn.com (test)"));
    assert_eq!(repo.lookup_owner("30.0.7.9".parse().unwrap()).unwrap().map(|o| o.asn), Some(107));
}

#[test]
fn a_failed_download_leaves_the_stored_dataset_as_it_was() {
    let mut repo = InMemoryRepository::default();
    refresh_owner_data(&FakeDatasetFetcher::new(&file(50)), &mut repo, at(1_000), &limits()).unwrap();

    let error =
        refresh_owner_data(&FakeDatasetFetcher::failing("timed out"), &mut repo, at(2_000), &limits()).unwrap_err();

    assert!(matches!(error, OwnerRefreshError::Fetch(ref message) if message == "timed out"), "{error}");
    assert_eq!(repo.dataset_info().unwrap().unwrap().downloaded_at, at(1_000));
    assert!(repo.lookup_owner("30.0.7.9".parse().unwrap()).unwrap().is_some());
}

#[test]
fn a_file_with_too_few_ranges_is_not_the_dataset_and_changes_nothing() {
    let mut repo = InMemoryRepository::default();
    refresh_owner_data(&FakeDatasetFetcher::new(&file(50)), &mut repo, at(1_000), &limits()).unwrap();

    for text in ["", "<html>Service unavailable</html>", &file(3)] {
        let error = refresh_owner_data(&FakeDatasetFetcher::new(text), &mut repo, at(2_000), &limits()).unwrap_err();
        assert!(matches!(error, OwnerRefreshError::Rejected(_)), "{error}");
    }

    assert_eq!(repo.dataset_info().unwrap().unwrap().rows, 50);
}

#[test]
fn a_file_where_too_many_lines_are_unreadable_is_rejected() {
    let mut repo = InMemoryRepository::default();
    let mut text = file(20);
    for n in 0..5 {
        text.push_str(&format!("junk line {n}\n"));
    }

    let error = refresh_owner_data(&FakeDatasetFetcher::new(&text), &mut repo, at(1_000), &limits()).unwrap_err();

    assert!(matches!(error, OwnerRefreshError::Rejected(ref why) if why.contains("5 of its 25 lines")), "{error}");
    assert_eq!(repo.dataset_info().unwrap(), None);
}

#[test]
fn a_few_unreadable_lines_among_many_are_tolerated_and_counted() {
    let mut repo = InMemoryRepository::default();
    let mut text = file(100);
    text.push_str("junk\n");

    let outcome = refresh_owner_data(&FakeDatasetFetcher::new(&text), &mut repo, at(1_000), &limits()).unwrap();

    assert_eq!((outcome.rows, outcome.bad_lines), (100, 1));
    assert_eq!(parse_ip2asn(&text).bad_lines, 1);
}

#[test]
fn unrouted_ranges_are_counted_and_not_kept() {
    let mut repo = InMemoryRepository::default();
    let mut text = file(20);
    text.push_str("31.0.0.0\t31.0.0.255\t0\tNone\tNot routed\n");

    let outcome = refresh_owner_data(&FakeDatasetFetcher::new(&text), &mut repo, at(1_000), &limits()).unwrap();

    assert_eq!((outcome.rows, outcome.unrouted), (20, 1));
    assert_eq!(repo.lookup_owner("31.0.0.9".parse().unwrap()).unwrap(), None);
}
