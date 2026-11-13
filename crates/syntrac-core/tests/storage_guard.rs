//! The storage guard, proven against a fake store and a fake volume (no disk involved).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use syntrac_core::retention::{GuardLevel, GuardLimits};
use syntrac_core::storage_guard::{check_storage, DatabaseUsage, DetailStore, FreeSpace, MAX_TRIM_STEPS_PER_CHECK};
use syntrac_core::{RepoError, Timestamp};

const MB: u64 = 1_000_000;
const GB: u64 = 1_000 * MB;
const HOUR: i64 = 3_600_000_000;

fn at_hour(h: i64) -> Timestamp {
    Timestamp::from_micros(h * HOUR)
}

/// The drive, shared between the store (which gives space back to it) and the guard (which reads it).
#[derive(Clone)]
struct Volume(Rc<Cell<u64>>);

impl FreeSpace for Volume {
    fn free_bytes(&self) -> Result<u64, String> {
        Ok(self.0.get())
    }
}

struct BrokenVolume;
impl FreeSpace for BrokenVolume {
    fn free_bytes(&self) -> Result<u64, String> {
        Err("the drive is gone".into())
    }
}

/// One hour of detail is `hour_bytes`. Deleted bytes sit in `pending` (free pages) until `reclaim_space` returns them to the
/// drive, like the real database.
struct FakeStore {
    hours: RefCell<Vec<i64>>,
    hour_bytes: u64,
    watermark_hour: Option<i64>,
    pending: Cell<u64>,
    drive: Rc<Cell<u64>>,
    /// A store whose deletes free nothing (something else is filling the drive).
    frees_nothing: bool,
    purges: Cell<u32>,
}

impl FakeStore {
    fn new(
        hours: std::ops::Range<i64>,
        hour_bytes: u64,
        watermark_hour: Option<i64>,
        drive_free: u64,
    ) -> (Self, Volume) {
        let drive = Rc::new(Cell::new(drive_free));
        let store = Self {
            hours: RefCell::new(hours.collect()),
            hour_bytes,
            watermark_hour,
            pending: Cell::new(0),
            drive: Rc::clone(&drive),
            frees_nothing: false,
            purges: Cell::new(0),
        };
        (store, Volume(drive))
    }
    fn live(&self) -> u64 {
        self.hours.borrow().len() as u64 * self.hour_bytes
    }
}

impl DetailStore for FakeStore {
    fn usage(&self) -> Result<DatabaseUsage, RepoError> {
        let oldest = self.hours.borrow().first().copied().map(at_hour);
        Ok(DatabaseUsage {
            file_bytes: self.live() + self.pending.get(),
            live_bytes: self.live(),
            reclaimable_bytes: self.pending.get(),
            oldest_detail: oldest,
            oldest_rollup: None,
        })
    }
    fn rolled_up_before(&self) -> Result<Option<Timestamp>, RepoError> {
        Ok(self.watermark_hour.map(at_hour))
    }
    fn purge_detail_before(&mut self, cutoff: Timestamp) -> Result<(), RepoError> {
        self.purges.set(self.purges.get() + 1);
        assert!(
            self.watermark_hour.is_some_and(|w| cutoff <= at_hour(w)),
            "the guard must never delete past the rollup watermark"
        );
        if self.frees_nothing {
            return Ok(());
        }
        let mut hours = self.hours.borrow_mut();
        let before = hours.len() as u64;
        hours.retain(|h| at_hour(*h) >= cutoff);
        self.pending.set(self.pending.get() + (before - hours.len() as u64) * self.hour_bytes);
        Ok(())
    }
    fn reclaim_space(&mut self) -> Result<(), RepoError> {
        self.drive.set(self.drive.get() + self.pending.get());
        self.pending.set(0);
        Ok(())
    }
}

/// The same limits but with a size cap that is not in play, for the tests about free space.
fn roomy_limits() -> GuardLimits {
    GuardLimits { max_db_bytes: 1_000 * GB, ..limits() }
}

fn limits() -> GuardLimits {
    GuardLimits { max_db_bytes: 10 * GB, min_free_bytes: 20 * GB, hard_floor_bytes: 5 * GB }
}

#[test]
fn within_every_limit_nothing_is_touched() {
    let (mut store, volume) = FakeStore::new(0..100, 10 * MB, Some(100), 200 * GB);
    let check = check_storage(&mut store, &volume, &limits(), GuardLevel::Ok).unwrap();
    assert_eq!((check.level, check.trimmed_steps, store.purges.get()), (GuardLevel::Ok, 0, 0));
    assert_eq!(check.usage.live_bytes, 1_000 * MB);
}

#[test]
fn over_the_cap_a_check_trims_the_oldest_hours_but_only_a_bounded_amount() {
    // 1,200 hours of 10 MB = 12 GB against a 10 GB cap.
    let (mut store, volume) = FakeStore::new(0..1_200, 10 * MB, Some(1_200), 200 * GB);
    let check = check_storage(&mut store, &volume, &limits(), GuardLevel::Ok).unwrap();
    assert_eq!(check.trimmed_steps, MAX_TRIM_STEPS_PER_CHECK, "a check stays short: it trims an hour at most");
    assert_eq!(check.level, GuardLevel::Trimming, "still over the cap, so the next check continues");
    assert_eq!(
        *store.hours.borrow().first().unwrap(),
        i64::from(MAX_TRIM_STEPS_PER_CHECK),
        "the OLDEST hours were removed"
    );
}

#[test]
fn repeated_checks_finish_the_job_and_then_leave_the_data_alone() {
    let (mut store, volume) = FakeStore::new(0..1_200, 10 * MB, Some(1_200), 200 * GB);
    let mut level = GuardLevel::Ok;
    for _ in 0..400 {
        level = check_storage(&mut store, &volume, &limits(), level).unwrap().level;
    }
    assert_eq!(level, GuardLevel::Ok);
    let kept = store.hours.borrow().len();
    let again = check_storage(&mut store, &volume, &limits(), level).unwrap();
    assert_eq!((again.level, again.trimmed_steps, store.hours.borrow().len()), (GuardLevel::Ok, 0, kept));
    assert!(store.live() <= 9 * GB);
}

#[test]
fn detail_that_is_not_rolled_up_is_never_deleted_whatever_the_disk_says() {
    // 12 GB live against a 10 GB cap, but only the first 10 hours are rolled up.
    let (mut store, volume) = FakeStore::new(0..1_200, 10 * MB, Some(10), 200 * GB);
    let (level, _) = settle(&mut store, &volume, &limits(), GuardLevel::Ok);
    assert_eq!(level, GuardLevel::Trimming, "still too big, and nothing more is eligible");
    assert_eq!(*store.hours.borrow().first().unwrap(), 10, "hours 0-9 went, hour 10 onwards (not rolled up) stayed");
}

#[test]
fn no_watermark_yet_means_nothing_can_be_trimmed() {
    let (mut store, volume) = FakeStore::new(0..1_200, 10 * MB, None, 200 * GB);
    let check = check_storage(&mut store, &volume, &limits(), GuardLevel::Ok).unwrap();
    assert_eq!((check.level, check.trimmed_steps, store.purges.get()), (GuardLevel::Trimming, 0, 0));
}

#[test]
fn short_of_free_space_trims_and_stops_once_the_freed_space_is_back_with_a_margin() {
    // 100 hours of 1 GB, a drive with 19 GB free (minimum 20): each trimmed hour returns 1 GB. Needs 22 GB (minimum + 10%).
    let (mut store, volume) = FakeStore::new(0..100, GB, Some(100), 19 * GB);
    let (level, steps) = settle(&mut store, &volume, &roomy_limits(), GuardLevel::Ok);
    assert_eq!(level, GuardLevel::Ok);
    assert_eq!(steps, 3, "19 -> 22 GB free: exactly three hours, not one more");
    assert_eq!(store.hours.borrow().len(), 97);
}

#[test]
fn freed_but_not_yet_reclaimed_space_is_counted_so_the_guard_does_not_over_delete() {
    // The store has already deleted 30 GB that has not been handed back: the drive shows 4 GB, effective free is 34 GB.
    let (mut store, volume) = FakeStore::new(0..100, GB, Some(100), 4 * GB);
    store.pending.set(30 * GB);
    let check = check_storage(&mut store, &volume, &roomy_limits(), GuardLevel::Paused).unwrap();
    assert_eq!(check.level, GuardLevel::Ok);
    assert_eq!((check.trimmed_steps, store.purges.get()), (0, 0));
}

#[test]
fn below_the_hard_floor_the_guard_reports_paused_and_still_frees_what_it_can() {
    let (mut store, volume) = FakeStore::new(0..10, GB, Some(10), 4 * GB);
    // Ten 1 GB hours bring free space from 4 GB to 14 GB: above the floor but still short of the 20 GB minimum.
    let (level, _) = settle(&mut store, &volume, &roomy_limits(), GuardLevel::Ok);
    assert_eq!(level, GuardLevel::Paused, "collection stays paused until free space is back above the minimum");
    assert_eq!(store.hours.borrow().len(), 0);
}

#[test]
fn a_step_that_frees_nothing_ends_the_check_instead_of_looping() {
    let (mut store, volume) = FakeStore::new(0..1_200, 10 * MB, Some(1_200), 200 * GB);
    store.frees_nothing = true;
    let check = check_storage(&mut store, &volume, &limits(), GuardLevel::Ok).unwrap();
    assert_eq!((check.level, check.trimmed_steps, store.purges.get()), (GuardLevel::Trimming, 1, 1));
}

#[test]
fn a_volume_that_cannot_be_read_is_an_error_not_a_guess() {
    let (mut store, _) = FakeStore::new(0..10, MB, Some(10), 200 * GB);
    assert!(check_storage(&mut store, &BrokenVolume, &limits(), GuardLevel::Ok).is_err());
}

/// Runs checks (each does a couple of hours at most) until one has nothing left to do. Returns the final check and the steps taken.
fn settle(store: &mut FakeStore, volume: &Volume, limits: &GuardLimits, start: GuardLevel) -> (GuardLevel, u32) {
    let (mut level, mut total) = (start, 0);
    for _ in 0..500 {
        let check = check_storage(store, volume, limits, level).unwrap();
        level = check.level;
        total += check.trimmed_steps;
        if check.trimmed_steps == 0 {
            break;
        }
    }
    (level, total)
}
