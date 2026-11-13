//! Notices a telemetry source that is running but has stopped delivering, and decides what to do about it.
//!
//! Pure logic: it is told a running count and the time, never reads a clock itself. The count is whatever the
//! source increments per event; "silent" means the count has not changed for a while.

use crate::time::Timestamp;

const MICROS_PER_SEC: i64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SilencePolicy {
    /// How long the count may stand still before the source is restarted.
    pub restart_after_secs: u64,
    /// How many restarts in a row are tried before giving up and leaving the problem visible.
    pub max_restarts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilenceAction {
    None,
    /// Restart the source. `attempt` counts from 1 within the current stall.
    Restart {
        attempt: u32,
    },
    /// Restarting did not help and the budget is spent. Reported once; the silence itself keeps being reported.
    GiveUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SilenceVerdict {
    /// Seconds since the count last changed. A restart never shortens this: it is what the health display shows.
    pub silent_secs: u64,
    pub action: SilenceAction,
}

#[derive(Debug)]
pub struct SilenceWatchdog {
    policy: SilencePolicy,
    last_count: u64,
    /// When the count last changed (or the watchdog started). The TRUE silence; a restart does not reset it.
    quiet_since: Timestamp,
    /// When the source was last restarted; only paces the next restart.
    last_restart: Option<Timestamp>,
    restarts_in_this_stall: u32,
    gave_up: bool,
}

impl SilenceWatchdog {
    pub fn new(policy: SilencePolicy, now: Timestamp) -> Self {
        Self { policy, last_count: 0, quiet_since: now, last_restart: None, restarts_in_this_stall: 0, gave_up: false }
    }

    pub fn observe(&mut self, count: u64, now: Timestamp) -> SilenceVerdict {
        if count != self.last_count {
            // Any change is activity, including a counter that restarted from zero with a new session.
            self.last_count = count;
            self.quiet_since = now;
            self.last_restart = None;
            self.restarts_in_this_stall = 0;
            self.gave_up = false;
        }
        SilenceVerdict { silent_secs: secs_between(self.quiet_since, now), action: self.decide(now) }
    }

    fn decide(&mut self, now: Timestamp) -> SilenceAction {
        // The wait for the next restart counts from the last restart if there was one, else from the last event.
        let waited = secs_between(self.last_restart.unwrap_or(self.quiet_since), now);
        if waited < self.policy.restart_after_secs || self.gave_up {
            return SilenceAction::None;
        }
        if self.restarts_in_this_stall >= self.policy.max_restarts {
            self.gave_up = true;
            return SilenceAction::GiveUp;
        }
        self.restarts_in_this_stall += 1;
        // The new session gets a full threshold to prove itself before the next restart.
        self.last_restart = Some(now);
        SilenceAction::Restart { attempt: self.restarts_in_this_stall }
    }
}

fn secs_between(from: Timestamp, to: Timestamp) -> u64 {
    u64::try_from(to.as_micros().saturating_sub(from.as_micros()).max(0)).unwrap_or(0) / MICROS_PER_SEC as u64
}
