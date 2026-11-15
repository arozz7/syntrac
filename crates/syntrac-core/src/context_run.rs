//! One pass of the context worker: look at what is running, work out what is worth writing, write it (docs/RUNAS-PLAN.md, I4a).
//!
//! Pure scheduling around two ports (the source and the repository). The Windows part is the source's business; the SQL is the
//! repository's. A pass costs one snapshot, and writes only what is new, changed, or has not been refreshed for a while.

use std::fmt;
use std::time::Duration;

use crate::exe_context::{merge_snapshot, ContextRepository, ContextSourceError, ContextTracker, ProcessContextSource};
use crate::ports::RepoError;
use crate::time::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextSettings {
    /// At most this many observations are written in one pass; the rest stay due for the next (a bound on one transaction).
    pub max_per_pass: usize,
    /// A program that keeps running has its "last seen" moved this often, not every pass.
    pub refresh_every: Duration,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self { max_per_pass: 2_000, refresh_every: Duration::from_secs(600) }
    }
}

/// What one pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ContextPass {
    /// Programs seen running as an account, after merging instances.
    pub seen: usize,
    /// Observations written (the rest were already stored recently).
    pub written: usize,
    /// Of those, accounts and services never stored before.
    pub new_accounts: usize,
    pub new_services: usize,
    /// Processes dropped for having no path or no SID.
    pub skipped: usize,
    /// Processes the adapter could not open.
    pub unreadable: usize,
    /// More were due than the pass limit allows; the rest follow at the next pass.
    pub truncated: bool,
}

#[derive(Debug)]
pub enum ContextFailure {
    Source(ContextSourceError),
    Repo(RepoError),
}

impl fmt::Display for ContextFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "could not look at the running programs: {error}"),
            Self::Repo(error) => write!(f, "could not store the context: {error}"),
        }
    }
}

impl std::error::Error for ContextFailure {}

/// Looks at what is running and stores what is worth storing. A failure of the source or the repository leaves the tracker
/// untouched, so everything is still due at the next pass.
pub fn observe_context<S, R>(
    source: &S,
    repo: &mut R,
    tracker: &mut ContextTracker,
    now: Timestamp,
    settings: &ContextSettings,
) -> Result<ContextPass, ContextFailure>
where
    S: ProcessContextSource + ?Sized,
    R: ContextRepository + ?Sized,
{
    let merged = merge_snapshot(source.observe().map_err(ContextFailure::Source)?);
    tracker.forget_unseen(&merged.observations);

    let mut due = tracker.due(&merged.observations, now);
    let truncated = due.len() > settings.max_per_pass;
    due.truncate(settings.max_per_pass);

    let mut pass = ContextPass {
        seen: merged.observations.len(),
        skipped: merged.skipped,
        unreadable: merged.unreadable,
        truncated,
        ..ContextPass::default()
    };
    if !due.is_empty() {
        let write = repo.record_context(&due, now).map_err(ContextFailure::Repo)?;
        tracker.mark_written(&due, now);
        pass.written = due.len();
        pass.new_accounts = write.new_accounts;
        pass.new_services = write.new_services;
    }
    Ok(pass)
}
