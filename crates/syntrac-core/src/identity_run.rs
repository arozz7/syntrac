//! Keeping executable identities up to date: which executables are due for a look, and one bounded pass over them.
//!
//! The rules are pure and the pass only talks to two ports (`IdentityRepository`, `FileInspector`), so it runs in tests with
//! fakes and never decides anything about the file itself.
//!
//! When is an executable due?
//! - never looked at: always;
//! - the last look failed: after a back-off that grows with each failure (1 h, 6 h, 1 day, then weekly);
//! - looked at successfully: only if it has RUN since that look, and the last look is older than `recheck_every`. A program
//!   that has not run cannot have been affected by what we are looking for, and this keeps old, long-gone programs quiet.
//!
//! A recheck first compares size and modification time (cheap). The whole file is read again only when they differ.

use std::cmp::Reverse;
use std::time::Duration;

use crate::exe_identity::{
    FileInspector, IdentityEntry, IdentityFailure, IdentityRecord, IdentityRepository, InspectError,
};
use crate::ports::RepoError;
use crate::time::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentitySettings {
    /// At most this many files per pass, so a backlog (the first run sees hundreds) is worked off gently.
    pub max_per_pass: usize,
    /// A checked executable is looked at again no sooner than this.
    pub recheck_every: Duration,
}

impl Default for IdentitySettings {
    fn default() -> Self {
        Self { max_per_pass: 10, recheck_every: Duration::from_secs(24 * 3600) }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IdentityPass {
    /// Files read in full and saved.
    pub inspected: usize,
    /// Files whose size and time had not changed: nothing was read.
    pub unchanged: usize,
    pub failed: usize,
    /// More executables are already due: run again soon instead of waiting for the usual interval.
    pub backlog: bool,
}

/// How long to wait before retrying after `attempts` consecutive failures.
pub fn retry_after(attempts: u32) -> Duration {
    const HOUR: u64 = 3600;
    Duration::from_secs(match attempts {
        0 => 0,
        1 => HOUR,
        2 => 6 * HOUR,
        3 => 24 * HOUR,
        _ => 7 * 24 * HOUR,
    })
}

fn micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

fn after(ts: Timestamp, wait: Duration) -> Timestamp {
    Timestamp::from_micros(ts.as_micros().saturating_add(micros(wait)))
}

pub fn is_due(entry: &IdentityEntry, now: Timestamp, settings: &IdentitySettings) -> bool {
    let Some(record) = &entry.record else { return true };
    if let Some(failure) = &record.failure {
        return after(record.last_attempt, retry_after(failure.attempts)) <= now;
    }
    let ran_since = entry.seen.is_some_and(|seen| seen.last_seen > record.last_attempt);
    ran_since && after(record.last_attempt, settings.recheck_every) <= now
}

/// Never-looked-at first (the most recently seen first: those matter most), then the rest, the longest-waiting first.
fn priority(entry: &IdentityEntry) -> (u8, Reverse<Option<Timestamp>>, Option<Timestamp>, String) {
    let last_seen = entry.seen.map(|seen| seen.last_seen);
    match &entry.record {
        None => (0, Reverse(last_seen), None, entry.exe.key.clone()),
        Some(record) => (1, Reverse(None), Some(record.last_attempt), entry.exe.key.clone()),
    }
}

/// One bounded pass: looks at the most important due executables and saves what it finds. A file that cannot be read is
/// recorded and does not stop the pass; only a storage error does.
pub fn inspect_due<R, I>(
    repo: &mut R,
    inspector: &I,
    now: Timestamp,
    settings: &IdentitySettings,
) -> Result<IdentityPass, RepoError>
where
    R: IdentityRepository + ?Sized,
    I: FileInspector + ?Sized,
{
    let mut due: Vec<IdentityEntry> =
        repo.identity_entries()?.into_iter().filter(|entry| is_due(entry, now, settings)).collect();
    due.sort_by_key(priority);

    let mut pass = IdentityPass { backlog: due.len() > settings.max_per_pass, ..IdentityPass::default() };
    for entry in due.into_iter().take(settings.max_per_pass) {
        let record = look_at(&entry, inspector, now, &mut pass);
        repo.save_identity(&record)?;
    }
    Ok(pass)
}

fn look_at<I: FileInspector + ?Sized>(
    entry: &IdentityEntry,
    inspector: &I,
    now: Timestamp,
    pass: &mut IdentityPass,
) -> IdentityRecord {
    let path = entry.exe.path.as_str();
    let previous = entry.record.as_ref();
    let known_facts = previous.and_then(|record| record.facts.as_ref());

    let outcome = match known_facts {
        Some(facts) => match inspector.stamp(path) {
            Ok(stamp) if stamp == facts.stamp => Ok(None),
            Ok(_) => inspector.inspect(path).map(Some),
            Err(error) => Err(error),
        },
        None => inspector.inspect(path).map(Some),
    };

    let attempt = |facts, inspected_at, failure| IdentityRecord {
        exe: entry.exe.clone(),
        facts,
        inspected_at,
        last_attempt: now,
        failure,
    };
    let kept_facts = known_facts.cloned();
    let kept_time = previous.and_then(|record| record.inspected_at);
    match outcome {
        Ok(Some(facts)) => {
            pass.inspected += 1;
            attempt(Some(facts), Some(now), None)
        }
        Ok(None) => {
            pass.unchanged += 1;
            attempt(kept_facts, kept_time, None)
        }
        Err(InspectError { kind, message }) => {
            pass.failed += 1;
            let earlier = previous.and_then(|record| record.failure.as_ref()).map_or(0, |failure| failure.attempts);
            attempt(kept_facts, kept_time, Some(IdentityFailure { kind, message, attempts: earlier + 1 }))
        }
    }
}
