//! View models for the Findings tab (docs/ALERTS-PLAN.md, F1): what the detection rules found, what the user decided, what they said
//! is expected, and whether detection is on. Same conventions as the other history views: camelCase on the wire, times in
//! milliseconds since the Unix epoch.
//!
//! The app only reads. A decision or removing an expectation goes to the service over the pipe, which checks it.

use serde::Serialize;
use syntrac_core::findings::{folder_scope_of, Expectation, Finding, FindingStatus};
use syntrac_core::{DetectionStateRepository, ExpectationRepository, FindingRepository, RepoError};

use crate::history::ms;

/// How many findings are listed at most.
pub const MAX_FINDING_ROWS: usize = 200;
/// How many are listed when the screen does not say.
pub const DEFAULT_FINDING_ROWS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingView {
    pub id: i64,
    pub kind: String,
    pub subject: String,
    pub subject_label: String,
    pub tier: &'static str,
    pub title: String,
    pub why: String,
    pub evidence: Vec<String>,
    pub status: &'static str,
    pub first_at_ms: i64,
    pub last_at_ms: i64,
    pub hits: u32,
    pub decided_at_ms: Option<i64>,
    pub snoozed_until_ms: Option<i64>,
    /// The install folder "always expected" could cover as a whole (a program finding only); the service works it out again itself.
    pub folder_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectationView {
    pub id: i64,
    pub kind: String,
    pub subject: String,
    pub note: Option<String>,
    pub created_ms: i64,
    pub source_finding: Option<i64>,
    pub ceiling: Option<f64>,
}

/// How many findings have each status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounts {
    pub open: u32,
    pub not_expected: u32,
    pub snoozed: u32,
    pub expected: u32,
    pub resolved: u32,
}

/// Detection as the database knows it. Absent until the detect worker has run once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionView {
    /// When detection was first switched on: "new" counts only what came after.
    pub switched_on_ms: i64,
    pub last_run_ms: Option<i64>,
    /// What went wrong in the last run, in plain words.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingsView {
    /// `None`: detection has never run (it is off, or has not had its first run yet).
    pub detection: Option<DetectionView>,
    pub counts: StatusCounts,
    pub findings: Vec<FindingView>,
    pub expectations: Vec<ExpectationView>,
}

fn finding_view(finding: Finding) -> FindingView {
    let folder_scope = folder_scope_of(&finding.kind, &finding.subject);
    FindingView {
        folder_scope,
        id: finding.id,
        kind: finding.kind,
        subject: finding.subject,
        subject_label: finding.subject_label,
        tier: finding.tier.as_str(),
        title: finding.title,
        why: finding.why,
        evidence: finding.evidence,
        status: finding.status.as_str(),
        first_at_ms: ms(finding.first_at),
        last_at_ms: ms(finding.last_at),
        hits: finding.hits,
        decided_at_ms: finding.decided_at.map(ms),
        snoozed_until_ms: finding.snoozed_until.map(ms),
    }
}

fn expectation_view(expectation: Expectation) -> ExpectationView {
    ExpectationView {
        id: expectation.id,
        kind: expectation.kind,
        subject: expectation.subject,
        note: expectation.note,
        created_ms: ms(expectation.created_at),
        source_finding: expectation.source_finding,
        ceiling: expectation.ceiling,
    }
}

/// The findings with `status` (`None`: every status), newest activity first, with the counts of every status, what the user said is
/// expected, and the detection state. An unknown status name is refused.
pub fn findings_view<R>(repo: &R, status: Option<&str>, limit: usize) -> Result<FindingsView, RepoError>
where
    R: FindingRepository + ExpectationRepository + DetectionStateRepository,
{
    let wanted = match status {
        None => None,
        Some(name) => {
            Some(FindingStatus::parse(name).ok_or_else(|| RepoError::Backend(format!("unknown status: {name}")))?)
        }
    };
    let mut counts = StatusCounts::default();
    for (status, count) in repo.status_counts()? {
        match status {
            FindingStatus::Open => counts.open = count,
            FindingStatus::NotExpected => counts.not_expected = count,
            FindingStatus::Snoozed => counts.snoozed = count,
            FindingStatus::Expected => counts.expected = count,
            FindingStatus::Resolved => counts.resolved = count,
        }
    }
    Ok(FindingsView {
        detection: repo.detection_state()?.map(|state| DetectionView {
            switched_on_ms: ms(state.switched_on_at),
            last_run_ms: state.last_run_at.map(ms),
            last_error: state.last_error,
        }),
        counts,
        findings: repo.findings(wanted, limit.clamp(1, MAX_FINDING_ROWS))?.into_iter().map(finding_view).collect(),
        expectations: repo.expectations()?.into_iter().map(expectation_view).collect(),
    })
}
