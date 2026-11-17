//! Findings: what a detector found, how repeats and the user's expectations are handled, and what each decision does
//! (docs/ALERTS-PLAN.md, F1). All pure: the stores, the detectors and the worker live elsewhere.
//!
//! A finding is evidence, never a verdict: the tiers are `unusual` (differs from what was seen before), `suspicious` (matches a
//! pattern often seen in rogue software) and `confirmed` (matches a list the user imported). Nothing here calls anything malicious.

use std::collections::{HashMap, HashSet};

use crate::clean_text::clean_text;
use crate::time::Timestamp;

/// Longest note the user can attach to an expectation.
pub const MAX_NOTE_CHARS: usize = 200;

const DAY_MICROS: i64 = 86_400 * 1_000_000;
const MAX_SNOOZE_DAYS: u32 = 365;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Unusual,
    Suspicious,
    Confirmed,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unusual => "unusual",
            Self::Suspicious => "suspicious",
            Self::Confirmed => "confirmed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "unusual" => Some(Self::Unusual),
            "suspicious" => Some(Self::Suspicious),
            "confirmed" => Some(Self::Confirmed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingStatus {
    Open,
    /// The user said it is expected this time.
    Expected,
    /// The user said it is NOT expected: it stays visible as a concern.
    NotExpected,
    Snoozed,
    /// The condition ended.
    Resolved,
}

impl FindingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Expected => "expected",
            Self::NotExpected => "not_expected",
            Self::Snoozed => "snoozed",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "open" => Some(Self::Open),
            "expected" => Some(Self::Expected),
            "not_expected" => Some(Self::NotExpected),
            "snoozed" => Some(Self::Snoozed),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
}

/// What a detector found, before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The rule that found it (`new_program`, ...). An expectation covers one kind.
    pub kind: String,
    /// What it is about, as a stable lower-case key (a program's path key, a start entry's key). An expectation covers exactly this.
    pub subject: String,
    /// What to call the subject on screen.
    pub subject_label: String,
    pub tier: Tier,
    pub title: String,
    /// One plain sentence: why this was raised.
    pub why: String,
    /// The facts the rule used, one per line.
    pub evidence: Vec<String>,
    /// The same thing found again has the same key (kind, subject and, for things that recur, a time window).
    pub dedupe_key: String,
    pub at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: i64,
    pub kind: String,
    pub subject: String,
    pub subject_label: String,
    pub tier: Tier,
    pub title: String,
    pub why: String,
    pub evidence: Vec<String>,
    pub dedupe_key: String,
    pub status: FindingStatus,
    pub first_at: Timestamp,
    pub last_at: Timestamp,
    /// How many times it was found.
    pub hits: u32,
    /// When the user last decided about it.
    pub decided_at: Option<Timestamp>,
    pub snoozed_until: Option<Timestamp>,
}

/// Something the user said is expected: a kind of finding about exactly one subject.
#[derive(Debug, Clone, PartialEq)]
pub struct Expectation {
    pub id: i64,
    pub kind: String,
    pub subject: String,
    pub note: Option<String>,
    pub created_at: Timestamp,
    /// The finding the user was looking at when they said so.
    pub source_finding: Option<i64>,
    /// For usage rules ("raise the limit to what I just saw"): the level that is now expected.
    pub ceiling: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewExpectation {
    pub kind: String,
    pub subject: String,
    pub note: Option<String>,
    pub source_finding: Option<i64>,
    pub ceiling: Option<f64>,
}

/// What reconciling changes in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingWrite {
    Insert(Candidate),
    /// Found again while open: one more hit.
    Touch {
        id: i64,
        at: Timestamp,
    },
    /// Found again after it was snoozed (and the snooze ended) or resolved.
    Reopen {
        id: i64,
        at: Timestamp,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSettings {
    /// At most this many NEW findings per run; the rest are counted as overflow (for the digest), most serious first.
    pub max_new: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reconciled {
    pub writes: Vec<FindingWrite>,
    /// Candidates the user had said are expected.
    pub suppressed: u32,
    /// New findings left out because of `max_new`.
    pub overflow: u32,
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Decides what to do with the candidates of one run. `existing` are the stored findings (any status). A candidate the user said
/// is expected is dropped; one already stored is touched or reopened according to its status; the rest are inserted, most serious
/// first, up to the cap.
pub fn reconcile(
    candidates: Vec<Candidate>,
    existing: &[Finding],
    expectations: &[Expectation],
    now: Timestamp,
    settings: &ReconcileSettings,
) -> Reconciled {
    let by_key: HashMap<&str, &Finding> = existing.iter().map(|f| (f.dedupe_key.as_str(), f)).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut result = Reconciled::default();
    let mut inserts: Vec<Candidate> = Vec::new();

    for candidate in candidates {
        if expectations.iter().any(|e| e.kind == candidate.kind && e.subject == candidate.subject) {
            result.suppressed += 1;
            continue;
        }
        if !seen.insert(candidate.dedupe_key.clone()) {
            continue;
        }
        match by_key.get(candidate.dedupe_key.as_str()) {
            None => inserts.push(candidate),
            Some(found) => match found.status {
                FindingStatus::Open | FindingStatus::NotExpected => {
                    result.writes.push(FindingWrite::Touch { id: found.id, at: now });
                }
                FindingStatus::Expected => {}
                FindingStatus::Snoozed => {
                    if found.snoozed_until.is_none_or(|until| until <= now) {
                        result.writes.push(FindingWrite::Reopen { id: found.id, at: now });
                    }
                }
                FindingStatus::Resolved => result.writes.push(FindingWrite::Reopen { id: found.id, at: now }),
            },
        }
    }

    inserts.sort_by_key(|candidate| std::cmp::Reverse(candidate.tier));
    result.overflow = count(inserts.len().saturating_sub(settings.max_new));
    inserts.truncate(settings.max_new);
    result.writes.extend(inserts.into_iter().map(FindingWrite::Insert));
    result
}

/// What the user can do with a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    ExpectedThisTime,
    /// Expected every time, for this kind of finding about this subject.
    AlwaysExpected {
        note: Option<String>,
    },
    NotExpected,
    Snooze {
        days: u32,
    },
    /// Undo any earlier decision.
    Reopen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionEffect {
    pub status: FindingStatus,
    pub snoozed_until: Option<Timestamp>,
    pub new_expectation: Option<NewExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DecisionError(pub String);

/// What a decision does to a finding. Refuses a snooze outside one day to a year, and a note that is too long.
pub fn apply_decision(finding: &Finding, decision: &Decision, now: Timestamp) -> Result<DecisionEffect, DecisionError> {
    let plain = |status| DecisionEffect { status, snoozed_until: None, new_expectation: None };
    match decision {
        Decision::ExpectedThisTime => Ok(plain(FindingStatus::Expected)),
        Decision::NotExpected => Ok(plain(FindingStatus::NotExpected)),
        Decision::Reopen => Ok(plain(FindingStatus::Open)),
        Decision::Snooze { days } => {
            if *days == 0 || *days > MAX_SNOOZE_DAYS {
                return Err(DecisionError(format!("A snooze must be between 1 and {MAX_SNOOZE_DAYS} days.")));
            }
            let until = Timestamp::from_micros(now.as_micros().saturating_add(i64::from(*days) * DAY_MICROS));
            Ok(DecisionEffect { status: FindingStatus::Snoozed, snoozed_until: Some(until), new_expectation: None })
        }
        Decision::AlwaysExpected { note } => {
            let note = match note.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
                None => None,
                Some(text) if text.chars().count() > MAX_NOTE_CHARS => {
                    return Err(DecisionError(format!("A note may be at most {MAX_NOTE_CHARS} characters.")));
                }
                Some(text) => clean_text(text, MAX_NOTE_CHARS),
            };
            Ok(DecisionEffect {
                status: FindingStatus::Expected,
                snoozed_until: None,
                new_expectation: Some(NewExpectation {
                    kind: finding.kind.clone(),
                    subject: finding.subject.clone(),
                    note,
                    source_finding: Some(finding.id),
                    ceiling: None,
                }),
            })
        }
    }
}
