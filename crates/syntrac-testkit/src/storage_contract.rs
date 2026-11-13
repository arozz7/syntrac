//! Contract for `StorageStats`: what the guard and the status report read. Every store (in-memory, SQLite) must pass it.

use std::time::Duration;

use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{
    roll_up_due, EventRepository, MetricsRepository, ProcessRepository, Retention, RollupRepository, RollupSettings,
    Timestamp,
};

use crate::fixtures::{dns_query, metrics_sample, EventFactory, SEC};

const T0: i64 = 1_794_571_200 / 3600 * 3600;

fn t(hour: i64, minutes: i64) -> Timestamp {
    Timestamp::from_micros((T0 + hour * 3600 + minutes * 60) * SEC)
}

pub trait StorageSubject:
    StorageStats + RollupRepository + EventRepository + MetricsRepository + ProcessRepository + Retention
{
}
impl<T> StorageSubject for T where
    T: StorageStats + RollupRepository + EventRepository + MetricsRepository + ProcessRepository + Retention
{
}

pub fn storage_stats_contract<R: StorageSubject>(make: impl Fn() -> R) {
    an_empty_store_has_no_ages(make());
    the_oldest_detail_and_the_oldest_rollup_are_reported(make());
    purging_moves_the_oldest_detail_but_not_the_oldest_rollup(make());
    size_follows_the_data(make());
    reclaiming_space_is_harmless(make());
}

fn an_empty_store_has_no_ages<R: StorageSubject>(repo: R) {
    let usage = repo.usage().unwrap();
    assert_eq!((usage.oldest_detail, usage.oldest_rollup), (None, None));
    assert_eq!(usage.reclaimable_bytes, 0);
}

fn fill<R: StorageSubject>(repo: &mut R) {
    let mut f = EventFactory::default();
    repo.append(&[f.make(t(2, 10), dns_query(1, "example.org", &[]))]).unwrap();
    for hour in 0..4 {
        repo.append_metrics(&metrics_sample(T0 + hour * 3600 + 600, 10.0, vec![], 0.0, 0)).unwrap();
    }
}

fn the_oldest_detail_and_the_oldest_rollup_are_reported<R: StorageSubject>(mut repo: R) {
    fill(&mut repo);
    assert_eq!(repo.usage().unwrap().oldest_detail, Some(t(0, 10)));
    assert_eq!(repo.usage().unwrap().oldest_rollup, None, "nothing rolled up yet");

    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, t(5, 0), &settings).unwrap();
    assert_eq!(repo.usage().unwrap().oldest_rollup, Some(t(0, 0)));
}

fn purging_moves_the_oldest_detail_but_not_the_oldest_rollup<R: StorageSubject>(mut repo: R) {
    fill(&mut repo);
    let settings = RollupSettings { settle: Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, t(5, 0), &settings).unwrap();

    repo.purge_before(t(2, 0)).unwrap();

    let usage = repo.usage().unwrap();
    assert_eq!(usage.oldest_detail, Some(t(2, 10)), "hours 0 and 1 of detail are gone");
    assert_eq!(usage.oldest_rollup, Some(t(0, 0)), "their summaries are not");
}

fn size_follows_the_data<R: StorageSubject>(mut repo: R) {
    let empty = repo.usage().unwrap().live_bytes;
    // Enough rows to need new pages in a real database (a handful would fit in the pages an empty one already has).
    let mut f = EventFactory::default();
    let events: Vec<_> =
        (0..3_000).map(|i| f.make(t(0, 1), dns_query(1, &format!("host-number-{i}.example.org"), &[]))).collect();
    repo.append(&events).unwrap();
    let full = repo.usage().unwrap().live_bytes;
    assert!(full > empty, "{full} should be more than {empty}");

    repo.purge_before(t(100, 0)).unwrap();
    repo.reclaim_space().unwrap();
    assert!(repo.usage().unwrap().live_bytes < full, "deleting the data lowers the live size");
}

fn reclaiming_space_is_harmless<R: StorageSubject>(mut repo: R) {
    fill(&mut repo);
    repo.reclaim_space().unwrap();
    repo.reclaim_space().unwrap();
    assert_eq!(repo.usage().unwrap().oldest_detail, Some(t(0, 10)), "no data was lost");
}
