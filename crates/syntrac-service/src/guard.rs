//! The collector's side of the storage guard: when to check, what the last check found, and the log lines when the level
//! changes. The rules and the trimming loop are in `syntrac_core::{retention, storage_guard}`, proven against a fake volume;
//! this only schedules them.

use std::time::Duration;

use syntrac_core::retention::{GuardLevel, GuardLimits};
use syntrac_core::storage_guard::{check_storage, DatabaseUsage, DetailStore, FreeSpace};
use syntrac_core::Timestamp;
use tracing::{error, info, warn};

/// What the last guard check found, for the status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageSnapshot {
    pub level: GuardLevel,
    pub usage: DatabaseUsage,
    pub drive_free_bytes: u64,
    /// One-hour steps of detail trimmed early since the service started.
    pub trimmed_steps_total: u64,
}

pub(crate) struct GuardRuntime {
    limits: GuardLimits,
    volume: Box<dyn FreeSpace>,
    interval: Duration,
    level: GuardLevel,
    next_check_at: Timestamp,
    snapshot: Option<StorageSnapshot>,
    trimmed_steps_total: u64,
}

impl GuardRuntime {
    pub(crate) fn new(limits: GuardLimits, volume: Box<dyn FreeSpace>, interval: Duration, now: Timestamp) -> Self {
        Self {
            limits,
            volume,
            interval,
            level: GuardLevel::Ok,
            next_check_at: now,
            snapshot: None,
            trimmed_steps_total: 0,
        }
    }

    pub(crate) fn level(&self) -> GuardLevel {
        self.level
    }

    pub(crate) fn snapshot(&self) -> Option<StorageSnapshot> {
        self.snapshot
    }

    /// Runs a check when one is due. Returns how many one-hour steps it trimmed. A failed measurement keeps the previous level
    /// (and is logged): guessing "all clear" or "full" from nothing would be worse.
    pub(crate) fn check_if_due<S: DetailStore + ?Sized>(&mut self, store: &mut S, now: Timestamp) -> u32 {
        if now < self.next_check_at {
            return 0;
        }
        let micros = i64::try_from(self.interval.as_micros()).unwrap_or(i64::MAX);
        self.next_check_at = Timestamp::from_micros(now.as_micros().saturating_add(micros));

        match check_storage(store, self.volume.as_ref(), &self.limits, self.level) {
            Ok(check) => {
                self.trimmed_steps_total += u64::from(check.trimmed_steps);
                if check.trimmed_steps > 0 {
                    warn!(hours = check.trimmed_steps, "Storage guard trimmed the oldest rolled-up detail early");
                }
                if check.level != self.level {
                    match check.level {
                        GuardLevel::Ok => info!("Storage guard: back within its limits"),
                        GuardLevel::Trimming => warn!(
                            live_mb = check.usage.live_bytes / 1_000_000,
                            free_mb = check.drive_free_bytes / 1_000_000,
                            "Storage guard: over its size cap or short of free space, trimming the oldest detail"
                        ),
                        GuardLevel::Paused => error!(
                            free_mb = check.drive_free_bytes / 1_000_000,
                            "Storage guard: free space is below the hard floor, detail collection is PAUSED (rollups and status continue)"
                        ),
                    }
                    self.level = check.level;
                }
                self.snapshot = Some(StorageSnapshot {
                    level: check.level,
                    usage: check.usage,
                    drive_free_bytes: check.drive_free_bytes,
                    trimmed_steps_total: self.trimmed_steps_total,
                });
                check.trimmed_steps
            }
            Err(error) => {
                error!(%error, "Storage guard could not measure the database or the drive; keeping the previous level");
                0
            }
        }
    }
}
