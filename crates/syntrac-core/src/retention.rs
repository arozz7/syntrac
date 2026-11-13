//! The retention and storage-guard rules, as pure functions: when detail may be deleted, how full is too full, and how far one
//! trimming step may go. No I/O here; the loop that applies them lives in `storage_guard`.
//!
//! The one rule that never bends: detail is deleted only after it has been rolled up (summarised). Every cutoff returned here
//! is bounded by the rollup watermark, whatever the disk looks like.

use std::time::Duration;

use crate::rollup::HOUR_MICROS;
use crate::time::Timestamp;

/// The cutoff for the normal purge: older than the retention window AND already rolled up. `None` when nothing has been rolled
/// up yet (then nothing may be deleted).
pub fn purge_cutoff(now: Timestamp, retention: Duration, rolled_up_before: Option<Timestamp>) -> Option<Timestamp> {
    let rolled_up = rolled_up_before?;
    let window = i64::try_from(retention.as_micros()).unwrap_or(i64::MAX);
    let by_age = Timestamp::from_micros(now.as_micros().saturating_sub(window));
    Some(by_age.min(rolled_up))
}

/// The cutoff for one early-trimming step: one hour past the oldest detail, never past the rollup watermark. `None` when there
/// is nothing eligible (no detail, or all remaining detail is not rolled up yet).
pub fn trim_cutoff(oldest_detail: Option<Timestamp>, rolled_up_before: Option<Timestamp>) -> Option<Timestamp> {
    let (oldest, rolled_up) = (oldest_detail?, rolled_up_before?);
    if oldest >= rolled_up {
        return None;
    }
    let step = Timestamp::from_micros(oldest.as_micros().saturating_add(HOUR_MICROS));
    Some(step.min(rolled_up))
}

/// How the collector is coping with the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuardLevel {
    /// Within every limit.
    #[default]
    Ok,
    /// Over the size cap or short of free space: the oldest rolled-up detail is being trimmed early.
    Trimming,
    /// Below the hard floor of free space: detail collection is paused (rollups, purging and status continue).
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardLimits {
    /// The most live data the database may hold.
    pub max_db_bytes: u64,
    /// Free space on the data drive below which trimming starts.
    pub min_free_bytes: u64,
    /// Free space below which detail collection pauses. Must be below `min_free_bytes`.
    pub hard_floor_bytes: u64,
}

/// What the guard measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageReading {
    /// Bytes of live data in the database (pages in use).
    pub live_bytes: u64,
    /// Bytes the database holds as free pages: deleted data that has not been handed back to the drive yet.
    pub reclaimable_bytes: u64,
    /// Free space on the drive that holds the database.
    pub drive_free_bytes: u64,
}

impl StorageReading {
    /// Free space as it will be once the database's free pages are reclaimed. Measuring this (not the drive alone) stops the
    /// guard from deleting more data while space it already freed has not been returned yet.
    pub fn effective_free_bytes(&self) -> u64 {
        self.drive_free_bytes.saturating_add(self.reclaimable_bytes)
    }
}

/// Leaving a state needs a margin beyond the limit that entered it (10%), so a database sitting at its cap does not flip
/// between states every check.
fn with_margin_below(limit: u64) -> u64 {
    limit - limit / 10
}

fn with_margin_above(limit: u64) -> u64 {
    limit.saturating_add(limit / 10)
}

/// The level for a reading, given the previous level (for the margins).
pub fn assess(limits: &GuardLimits, reading: &StorageReading, previous: GuardLevel) -> GuardLevel {
    let free = reading.effective_free_bytes();
    if free < limits.hard_floor_bytes {
        return GuardLevel::Paused;
    }
    if previous == GuardLevel::Paused && free < limits.min_free_bytes {
        return GuardLevel::Paused; // collection resumes only once free space is back above the minimum
    }
    let over_cap = reading.live_bytes > limits.max_db_bytes;
    let short_of_space = free < limits.min_free_bytes;
    if over_cap || short_of_space {
        return GuardLevel::Trimming;
    }
    if previous != GuardLevel::Ok {
        // Coming back from Trimming (or Paused): only once there is a margin below the cap and above the minimum.
        let well_inside = reading.live_bytes <= with_margin_below(limits.max_db_bytes)
            && free >= with_margin_above(limits.min_free_bytes);
        if !well_inside {
            return GuardLevel::Trimming;
        }
    }
    GuardLevel::Ok
}

/// One bounded purge pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeStep {
    /// Delete detail older than this.
    pub cutoff: Timestamp,
    /// The full cutoff is further on: run another pass soon.
    pub more_remains: bool,
}

/// Deleting costs about a second per hour of detail, so a long backlog is removed in passes of at most `max_span` rather than in
/// one go that would hold up the collector's loop (and let ETW's buffers overflow). With no detail older than the cutoff the
/// pass simply runs to the cutoff: that is cheap, and it still removes old process records.
pub fn purge_step(cutoff: Timestamp, oldest_detail: Option<Timestamp>, max_span: Duration) -> PurgeStep {
    let span = i64::try_from(max_span.as_micros()).unwrap_or(i64::MAX);
    match oldest_detail {
        Some(oldest) if oldest < cutoff => {
            let limit = Timestamp::from_micros(oldest.as_micros().saturating_add(span));
            if limit < cutoff {
                PurgeStep { cutoff: limit, more_remains: true }
            } else {
                PurgeStep { cutoff, more_remains: false }
            }
        }
        _ => PurgeStep { cutoff, more_remains: false },
    }
}
