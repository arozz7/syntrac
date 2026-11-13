//! The retention gate and the storage-guard levels (pure rules).

use std::time::Duration;

use syntrac_core::retention::{assess, purge_cutoff, trim_cutoff, GuardLevel, GuardLimits, StorageReading};
use syntrac_core::Timestamp;

const GB: u64 = 1_000_000_000;
const HOUR: i64 = 3_600_000_000;
const DAY: i64 = 24 * HOUR;

fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

fn days(n: u64) -> Duration {
    Duration::from_secs(n * 24 * 3600)
}

// ---- the purge gate

#[test]
fn nothing_may_be_purged_before_anything_is_rolled_up() {
    assert_eq!(purge_cutoff(at(100 * DAY), days(14), None), None);
}

#[test]
fn the_cutoff_is_the_retention_window_when_rollups_are_ahead_of_it() {
    let now = at(100 * DAY);
    let rolled_up = Some(at(99 * DAY));
    assert_eq!(purge_cutoff(now, days(14), rolled_up), Some(at(86 * DAY)));
}

#[test]
fn the_cutoff_never_passes_the_rollup_watermark_even_when_retention_would_allow_it() {
    let now = at(100 * DAY);
    let stalled = Some(at(40 * DAY)); // rollups have stopped for 60 days
    assert_eq!(purge_cutoff(now, days(14), stalled), Some(at(40 * DAY)), "unsummarised detail is kept");
}

#[test]
fn a_huge_retention_does_not_overflow() {
    let cutoff = purge_cutoff(at(10), Duration::from_secs(u64::MAX / 2), Some(at(5))).unwrap();
    assert!(cutoff <= at(5));
}

// ---- one trimming step

#[test]
fn a_trim_step_is_one_hour_past_the_oldest_detail() {
    assert_eq!(trim_cutoff(Some(at(10 * HOUR)), Some(at(100 * HOUR))), Some(at(11 * HOUR)));
}

#[test]
fn a_trim_step_never_passes_the_watermark() {
    assert_eq!(trim_cutoff(Some(at(10 * HOUR)), Some(at(10 * HOUR + HOUR / 2))), Some(at(10 * HOUR + HOUR / 2)));
}

#[test]
fn nothing_is_eligible_when_the_oldest_detail_is_not_rolled_up() {
    assert_eq!(trim_cutoff(Some(at(10 * HOUR)), Some(at(10 * HOUR))), None);
    assert_eq!(trim_cutoff(Some(at(12 * HOUR)), Some(at(10 * HOUR))), None);
    assert_eq!(trim_cutoff(None, Some(at(10 * HOUR))), None, "no detail at all");
    assert_eq!(trim_cutoff(Some(at(10 * HOUR)), None), None, "no watermark yet");
}

// ---- guard levels

fn limits() -> GuardLimits {
    GuardLimits { max_db_bytes: 10 * GB, min_free_bytes: 20 * GB, hard_floor_bytes: 5 * GB }
}

fn reading(live_gb: u64, reclaimable_gb: u64, drive_free_gb: u64) -> StorageReading {
    StorageReading {
        live_bytes: live_gb * GB,
        reclaimable_bytes: reclaimable_gb * GB,
        drive_free_bytes: drive_free_gb * GB,
    }
}

#[test]
fn plenty_of_room_is_ok() {
    assert_eq!(assess(&limits(), &reading(3, 0, 200), GuardLevel::Ok), GuardLevel::Ok);
}

#[test]
fn over_the_size_cap_starts_trimming() {
    assert_eq!(assess(&limits(), &reading(11, 0, 200), GuardLevel::Ok), GuardLevel::Trimming);
}

#[test]
fn short_of_free_space_starts_trimming() {
    assert_eq!(assess(&limits(), &reading(3, 0, 19), GuardLevel::Ok), GuardLevel::Trimming);
}

#[test]
fn below_the_hard_floor_collection_pauses() {
    assert_eq!(assess(&limits(), &reading(3, 0, 4), GuardLevel::Ok), GuardLevel::Paused);
    assert_eq!(assess(&limits(), &reading(3, 0, 4), GuardLevel::Trimming), GuardLevel::Paused);
}

#[test]
fn space_the_database_has_freed_but_not_yet_returned_counts_as_free() {
    // The drive shows 4 GB (below the floor) but 30 GB of freed pages are waiting to be reclaimed.
    let r = reading(3, 30, 4);
    assert_eq!(r.effective_free_bytes(), 34 * GB);
    assert_eq!(
        assess(&limits(), &r, GuardLevel::Ok),
        GuardLevel::Ok,
        "trimming does not continue over space already freed"
    );
}

#[test]
fn a_paused_collector_resumes_only_once_free_space_is_back_above_the_minimum() {
    assert_eq!(
        assess(&limits(), &reading(3, 0, 10), GuardLevel::Paused),
        GuardLevel::Paused,
        "above the floor is not enough"
    );
    assert_ne!(assess(&limits(), &reading(3, 0, 21), GuardLevel::Paused), GuardLevel::Paused);
}

#[test]
fn trimming_stops_only_with_a_margin_so_a_database_at_its_cap_does_not_flap() {
    // Right at the cap: still Trimming (it was trimming), not Ok.
    assert_eq!(assess(&limits(), &reading(10, 0, 200), GuardLevel::Trimming), GuardLevel::Trimming);
    // Just inside the cap but not by the margin (9.5 GB of a 10 GB cap): still Trimming.
    let nearly = StorageReading { live_bytes: 9_500_000_000, reclaimable_bytes: 0, drive_free_bytes: 200 * GB };
    assert_eq!(assess(&limits(), &nearly, GuardLevel::Trimming), GuardLevel::Trimming);
    // Well inside (90% of the cap or less, 9 GB included): Ok again.
    assert_eq!(assess(&limits(), &reading(9, 0, 200), GuardLevel::Trimming), GuardLevel::Ok);
    assert_eq!(assess(&limits(), &reading(8, 0, 200), GuardLevel::Trimming), GuardLevel::Ok);
    // And from Ok, being just under the cap stays Ok (no margin is needed to STAY ok).
    assert_eq!(assess(&limits(), &reading(9, 0, 200), GuardLevel::Ok), GuardLevel::Ok);
}

#[test]
fn trimming_for_lack_of_free_space_ends_only_with_a_margin_above_the_minimum() {
    assert_eq!(
        assess(&limits(), &reading(3, 0, 21), GuardLevel::Trimming),
        GuardLevel::Trimming,
        "21 < 22 GB (minimum + 10%)"
    );
    assert_eq!(assess(&limits(), &reading(3, 0, 22), GuardLevel::Trimming), GuardLevel::Ok);
}

// ---- purging in bounded steps

use syntrac_core::retention::purge_step;

#[test]
fn a_purge_pass_goes_no_further_than_its_span_past_the_oldest_detail() {
    let step = purge_step(at(100 * HOUR), Some(at(10 * HOUR)), Duration::from_secs(2 * 3600));
    assert_eq!((step.cutoff, step.more_remains), (at(12 * HOUR), true));
}

#[test]
fn the_last_pass_reaches_the_cutoff_and_says_nothing_more_remains() {
    let step = purge_step(at(11 * HOUR), Some(at(10 * HOUR)), Duration::from_secs(2 * 3600));
    assert_eq!((step.cutoff, step.more_remains), (at(11 * HOUR), false));
    let exact = purge_step(at(12 * HOUR), Some(at(10 * HOUR)), Duration::from_secs(2 * 3600));
    assert_eq!(
        (exact.cutoff, exact.more_remains),
        (at(12 * HOUR), false),
        "reaching the cutoff exactly is also the end"
    );
}

#[test]
fn with_no_old_detail_the_pass_still_runs_to_the_cutoff_so_old_process_records_go_too() {
    let none = purge_step(at(50 * HOUR), None, Duration::from_secs(3600));
    assert_eq!((none.cutoff, none.more_remains), (at(50 * HOUR), false));
    let newer = purge_step(at(50 * HOUR), Some(at(60 * HOUR)), Duration::from_secs(3600));
    assert_eq!(
        (newer.cutoff, newer.more_remains),
        (at(50 * HOUR), false),
        "the oldest detail is already inside the window"
    );
}
