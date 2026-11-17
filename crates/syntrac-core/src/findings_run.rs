//! One detection run: ask every detector what it found, clean it, reconcile it with what is stored and what the user said is
//! expected, and write the result (docs/ALERTS-PLAN.md, F1). The detectors and the stores are ports, so the whole loop runs in
//! memory with fakes.

use crate::clean_text::clean_text;
use crate::findings::{
    reconcile, Candidate, Expectation, Finding, FindingStatus, FindingWrite, NewExpectation, ReconcileSettings,
};
use crate::ports::RepoError;
use crate::time::Timestamp;

const MAX_TITLE_CHARS: usize = 120;
const MAX_TEXT_CHARS: usize = 300;
const MAX_EVIDENCE_LINES: usize = 12;
const MAX_KEY_CHARS: usize = 600;

/// Where findings are kept.
pub trait FindingRepository {
    /// Newest activity first; `None` for every status.
    fn findings(&self, status: Option<FindingStatus>, limit: usize) -> Result<Vec<Finding>, RepoError>;

    fn finding(&self, id: i64) -> Result<Option<Finding>, RepoError>;

    /// The stored findings (any status) with one of these dedupe keys.
    fn findings_for_keys(&self, keys: &[String]) -> Result<Vec<Finding>, RepoError>;

    /// Applies the writes of one run in one transaction.
    fn apply_writes(&mut self, writes: &[FindingWrite]) -> Result<(), RepoError>;

    /// Sets the status the user chose. `false` when there is no such finding.
    fn set_status(
        &mut self,
        id: i64,
        status: FindingStatus,
        decided_at: Timestamp,
        snoozed_until: Option<Timestamp>,
    ) -> Result<bool, RepoError>;

    fn status_counts(&self) -> Result<Vec<(FindingStatus, u32)>, RepoError>;
}

/// Where the user's expectations are kept. Permanent.
pub trait ExpectationRepository {
    /// Oldest first.
    fn expectations(&self) -> Result<Vec<Expectation>, RepoError>;

    /// Adds the expectation, or (when this kind and subject are already expected) updates its note and ceiling. Returns its id.
    fn add_expectation(&mut self, expectation: &NewExpectation, at: Timestamp) -> Result<i64, RepoError>;

    /// `false` when there was no such expectation.
    fn remove_expectation(&mut self, id: i64) -> Result<bool, RepoError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionState {
    /// When detection was first switched on. "New" rules only look at things first seen after this.
    pub switched_on_at: Timestamp,
    pub last_run_at: Option<Timestamp>,
    /// What went wrong in the last run, in plain words.
    pub last_error: Option<String>,
}

pub trait DetectionStateRepository {
    fn detection_state(&self) -> Result<Option<DetectionState>, RepoError>;

    /// Records that detection is on and returns when it was FIRST switched on (an earlier call wins).
    fn switch_on(&mut self, now: Timestamp) -> Result<Timestamp, RepoError>;

    fn record_run(&mut self, at: Timestamp, error: Option<&str>) -> Result<(), RepoError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DetectError(pub String);

/// One rule (or group of rules). It owns its own read ports; the run only asks it what it found.
pub trait Detector {
    fn name(&self) -> &'static str;

    /// `since` is when detection was first switched on; "new" rules count only what was first seen after it.
    fn detect(&self, since: Timestamp, now: Timestamp) -> Result<Vec<Candidate>, DetectError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunReport {
    pub candidates: u32,
    pub inserted: u32,
    pub touched: u32,
    pub reopened: u32,
    pub suppressed: u32,
    pub overflow: u32,
    /// Detectors that failed, with why. A failed detector never stops the others.
    pub failed: Vec<(String, String)>,
}

/// A candidate with every text cleaned and capped; `None` when it has nothing to show or no usable key.
pub fn clean_candidate(candidate: Candidate) -> Option<Candidate> {
    let title = clean_text(&candidate.title, MAX_TITLE_CHARS)?;
    let why = clean_text(&candidate.why, MAX_TEXT_CHARS)?;
    let subject = clean_text(&candidate.subject, MAX_KEY_CHARS)?;
    let dedupe_key = clean_text(&candidate.dedupe_key, MAX_KEY_CHARS)?;
    let subject_label = clean_text(&candidate.subject_label, MAX_TITLE_CHARS).unwrap_or_else(|| subject.clone());
    let evidence = candidate
        .evidence
        .iter()
        .filter_map(|line| clean_text(line, MAX_TEXT_CHARS))
        .take(MAX_EVIDENCE_LINES)
        .collect();
    Some(Candidate { title, why, subject, dedupe_key, subject_label, evidence, ..candidate })
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Runs every detector once and stores what they found. The first run switches detection on (the moment "new" counts from).
pub fn run_detection<R>(
    detectors: &[&dyn Detector],
    repo: &mut R,
    now: Timestamp,
    settings: &ReconcileSettings,
) -> Result<RunReport, RepoError>
where
    R: FindingRepository + ExpectationRepository + DetectionStateRepository,
{
    let since = repo.switch_on(now)?;
    let mut report = RunReport::default();
    let mut candidates = Vec::new();
    for detector in detectors {
        match detector.detect(since, now) {
            Ok(found) => candidates.extend(found.into_iter().filter_map(clean_candidate)),
            Err(error) => report.failed.push((detector.name().to_owned(), error.0)),
        }
    }
    report.candidates = count(candidates.len());

    let keys: Vec<String> = candidates.iter().map(|candidate| candidate.dedupe_key.clone()).collect();
    let existing = repo.findings_for_keys(&keys)?;
    let expectations = repo.expectations()?;
    let reconciled = reconcile(candidates, &existing, &expectations, now, settings);

    for write in &reconciled.writes {
        match write {
            FindingWrite::Insert(_) => report.inserted += 1,
            FindingWrite::Touch { .. } => report.touched += 1,
            FindingWrite::Reopen { .. } => report.reopened += 1,
        }
    }
    report.suppressed = reconciled.suppressed;
    report.overflow = reconciled.overflow;
    repo.apply_writes(&reconciled.writes)?;

    let error = if report.failed.is_empty() {
        None
    } else {
        Some(report.failed.iter().map(|(name, why)| format!("{name}: {why}")).collect::<Vec<_>>().join("; "))
    };
    repo.record_run(now, error.as_deref())?;
    Ok(report)
}
