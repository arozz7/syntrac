//! The storage guard: measures the database and the drive, and trims the oldest rolled-up detail when either is too full.
//!
//! The rules (levels, margins, cutoffs) are the pure functions in [`crate::retention`]; this module only applies them to a
//! store and a volume that are passed in, so the whole behaviour is proven offline against a fake volume.

use crate::ports::{RepoError, Retention};
use crate::retention::{assess, trim_cutoff, GuardLevel, GuardLimits, StorageReading};
use crate::rollup_run::RollupRepository;
use crate::time::Timestamp;

/// What the database holds and how big it is. Both rollup and detail ages are reported so growth is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DatabaseUsage {
    /// The size of the database file.
    pub file_bytes: u64,
    /// Bytes in use by live data.
    pub live_bytes: u64,
    /// Bytes held as free pages, waiting to be handed back to the drive.
    pub reclaimable_bytes: u64,
    pub oldest_detail: Option<Timestamp>,
    pub oldest_rollup: Option<Timestamp>,
}

/// Size accounting and space recovery for a store.
pub trait StorageStats {
    fn usage(&self) -> Result<DatabaseUsage, RepoError>;

    /// Hands free pages back to the drive. Must be safe to call at any time; may do nothing.
    fn reclaim_space(&mut self) -> Result<(), RepoError>;
}

/// Free space on the drive that holds the database.
pub trait FreeSpace {
    fn free_bytes(&self) -> Result<u64, String>;
}

/// The narrow view the guard needs. Anything that can purge, report usage and report the rollup watermark is one.
pub trait DetailStore {
    fn usage(&self) -> Result<DatabaseUsage, RepoError>;
    fn rolled_up_before(&self) -> Result<Option<Timestamp>, RepoError>;
    fn purge_detail_before(&mut self, cutoff: Timestamp) -> Result<(), RepoError>;
    fn reclaim_space(&mut self) -> Result<(), RepoError>;
}

impl<T: Retention + StorageStats + RollupRepository> DetailStore for T {
    fn usage(&self) -> Result<DatabaseUsage, RepoError> {
        StorageStats::usage(self)
    }
    fn rolled_up_before(&self) -> Result<Option<Timestamp>, RepoError> {
        RollupRepository::rolled_up_before(self)
    }
    fn purge_detail_before(&mut self, cutoff: Timestamp) -> Result<(), RepoError> {
        self.purge_before(cutoff).map(|_| ())
    }
    fn reclaim_space(&mut self) -> Result<(), RepoError> {
        StorageStats::reclaim_space(self)
    }
}

/// At most this many one-hour trimming steps in one check (about a second each on this database), so a check never holds up the
/// collector's loop; the guard checks every minute, so a few hours a minute is plenty.
pub const MAX_TRIM_STEPS_PER_CHECK: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardCheck {
    pub level: GuardLevel,
    pub usage: DatabaseUsage,
    pub drive_free_bytes: u64,
    /// One-hour steps of detail trimmed by this check.
    pub trimmed_steps: u32,
}

fn measure<S: DetailStore + ?Sized>(
    store: &S,
    volume: &dyn FreeSpace,
) -> Result<(DatabaseUsage, StorageReading), RepoError> {
    let usage = store.usage()?;
    let drive_free_bytes = volume.free_bytes().map_err(RepoError::Backend)?;
    let reading =
        StorageReading { live_bytes: usage.live_bytes, reclaimable_bytes: usage.reclaimable_bytes, drive_free_bytes };
    Ok((usage, reading))
}

/// One guard check: measure, and while the level is not `Ok` trim one hour of the oldest ROLLED-UP detail at a time, hand the
/// freed pages back, and measure again. Stops when the level is `Ok`, when nothing eligible is left (everything remaining is not
/// rolled up yet), when a step frees nothing measurable (something else is filling the drive: more deleting would not help), or
/// after [`MAX_TRIM_STEPS_PER_CHECK`] steps.
pub fn check_storage<S: DetailStore + ?Sized>(
    store: &mut S,
    volume: &dyn FreeSpace,
    limits: &GuardLimits,
    previous: GuardLevel,
) -> Result<GuardCheck, RepoError> {
    let (mut usage, mut reading) = measure(store, volume)?;
    let mut level = assess(limits, &reading, previous);
    let mut steps = 0;

    while level != GuardLevel::Ok && steps < MAX_TRIM_STEPS_PER_CHECK {
        let Some(cutoff) = trim_cutoff(usage.oldest_detail, store.rolled_up_before()?) else { break };
        store.purge_detail_before(cutoff)?;
        store.reclaim_space()?;
        steps += 1;

        let before = usage.live_bytes;
        (usage, reading) = measure(store, volume)?;
        level = assess(limits, &reading, level);
        if usage.live_bytes >= before {
            break; // nothing was freed: stop rather than loop
        }
    }
    Ok(GuardCheck { level, usage, drive_free_bytes: reading.drive_free_bytes, trimmed_steps: steps })
}
