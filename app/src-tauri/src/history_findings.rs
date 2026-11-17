//! View models for the Findings tab (docs/ALERTS-PLAN.md, F1): what the detection rules found, what the user decided, what they said
//! is expected, and whether detection is on. Same conventions as the other history views: camelCase on the wire, times in
//! milliseconds since the Unix epoch.
//!
//! The app only reads. A decision or removing an expectation goes to the service over the pipe, which checks it.

use serde::Serialize;
use std::collections::HashSet;
use std::net::IpAddr;
use syntrac_core::findings::{folder_scope_of, Expectation, Finding, FindingStatus};

use syntrac_core::rollup::HOUR_MICROS;
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{
    classify_addr, new_destination_cutoff, review_groups, DestinationTrendRepository, DetectionStateRepository,
    ExpectationRepository, FindingRepository, IdentityRepository, KnownProductRepository, OwnerRepository, RepoError,
    Timestamp,
};

use crate::history::ms;

/// How many review groups are listed at most.
pub const MAX_REVIEW_GROUPS: usize = 100;
/// How many are listed when the screen does not say.
pub const DEFAULT_REVIEW_GROUPS: usize = 60;

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

/// What appeared in the last day, counted and not raised: new internet addresses, and networks never reached before. A digest, not
/// an alert (there are thousands of destinations and new ones appear daily).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestView {
    pub window_hours: u32,
    /// Internet addresses first seen in the window (never the first day of history, which is the baseline).
    pub new_destinations: u32,
    /// Networks (by owner) among them that no earlier address belonged to. Zero without the owner data.
    pub new_owners: u32,
    /// The owner data is on this machine; without it networks cannot be counted.
    pub owner_data: bool,
}

pub const DIGEST_WINDOW_HOURS: u32 = 24;

/// Counts what appeared in the last day.
pub fn digest_view<R>(repo: &R, now: Timestamp) -> Result<DigestView, RepoError>
where
    R: DestinationTrendRepository + OwnerRepository + StorageStats,
{
    let window_start = Timestamp::from_micros(now.as_micros() - i64::from(DIGEST_WINDOW_HOURS) * HOUR_MICROS);
    let cutoff = new_destination_cutoff(StorageStats::usage(repo)?.oldest_rollup, window_start);
    let fresh: Vec<IpAddr> = repo
        .new_destinations(cutoff)?
        .into_iter()
        .map(|row| row.addr)
        .filter(|addr| classify_addr(*addr).is_internet())
        .collect();
    let owner_data = repo.dataset_info()?.is_some();
    let mut new_owners = 0;
    if owner_data && !fresh.is_empty() {
        let mut fresh_owners: HashSet<u32> = HashSet::new();
        for addr in &fresh {
            if let Some(owner) = repo.lookup_owner(*addr)? {
                fresh_owners.insert(owner.asn);
            }
        }
        // Every network an address from before the window belonged to.
        for row in repo.destination_totals(Timestamp::from_micros(0), cutoff)? {
            if fresh_owners.is_empty() {
                break;
            }
            if classify_addr(row.addr).is_internet() {
                if let Some(owner) = repo.lookup_owner(row.addr)? {
                    fresh_owners.remove(&owner.asn);
                }
            }
        }
        new_owners = u32::try_from(fresh_owners.len()).unwrap_or(u32::MAX);
    }
    Ok(DigestView {
        window_hours: DIGEST_WINDOW_HOURS,
        new_destinations: u32::try_from(fresh.len()).unwrap_or(u32::MAX),
        new_owners,
        owner_data,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingsView {
    /// The last day's new addresses and networks, counted.
    pub digest: DigestView,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewProgramView {
    pub path: String,
    pub name: String,
}

/// A group of programs that were already here when detection was switched on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewGroupView {
    /// The install folder, lower case: what "mark as expected" covers (the service checks it again).
    pub root: String,
    pub label: String,
    pub publisher: Option<String>,
    pub count: usize,
    pub programs: Vec<ReviewProgramView>,
    /// Why it deserves a closer look, as phrases; empty when it does not.
    pub concerns: Vec<String>,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    /// When detection was first switched on; none before that (then every program seen running is offered).
    pub switched_on_ms: Option<i64>,
    pub total_groups: usize,
    pub groups: Vec<ReviewGroupView>,
}

/// "Review what is here today": the groups of programs seen before the switch-on that no known product and no expectation covers.
pub fn review_view<R>(repo: &R, limit: usize) -> Result<ReviewView, RepoError>
where
    R: IdentityRepository + KnownProductRepository + ExpectationRepository + DetectionStateRepository,
{
    let switched_on = repo.detection_state()?.map(|state| state.switched_on_at);
    let groups = review_groups(&repo.identity_entries()?, &repo.known_products()?, &repo.expectations()?, switched_on);
    let total_groups = groups.len();
    Ok(ReviewView {
        switched_on_ms: switched_on.map(ms),
        total_groups,
        groups: groups
            .into_iter()
            .take(limit.clamp(1, MAX_REVIEW_GROUPS))
            .map(|group| ReviewGroupView {
                root: group.root,
                label: group.label,
                publisher: group.publisher,
                count: group.count,
                programs: group
                    .programs
                    .into_iter()
                    .map(|program| ReviewProgramView { path: program.path, name: program.name })
                    .collect(),
                concerns: group.concerns,
                location: group.location.to_owned(),
            })
            .collect(),
    })
}

/// The findings with `status` (`None`: every status), newest activity first, with the counts of every status, what the user said is
/// expected, and the detection state. An unknown status name is refused.
pub fn findings_view<R>(repo: &R, status: Option<&str>, limit: usize, now: Timestamp) -> Result<FindingsView, RepoError>
where
    R: FindingRepository
        + ExpectationRepository
        + DetectionStateRepository
        + DestinationTrendRepository
        + OwnerRepository
        + StorageStats,
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
        digest: digest_view(repo, now)?,
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
