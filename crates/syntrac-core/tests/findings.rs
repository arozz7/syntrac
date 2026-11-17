//! Findings: what a detector found, how repeats and expectations are handled, and what each decision of the user does. All pure.

use syntrac_core::findings::{
    apply_decision, reconcile, Candidate, Decision, DecisionEffect, Expectation, Finding, FindingStatus, FindingWrite,
    ReconcileSettings, Tier,
};
use syntrac_core::Timestamp;

const DAY: i64 = 86_400 * 1_000_000;

fn at(day: i64) -> Timestamp {
    Timestamp::from_micros(day * DAY)
}

fn candidate(kind: &str, subject: &str, tier: Tier) -> Candidate {
    Candidate {
        kind: kind.into(),
        subject: subject.into(),
        subject_label: subject.into(),
        tier,
        title: format!("{kind} {subject}"),
        why: "Because.".into(),
        evidence: vec!["one".into()],
        dedupe_key: format!("{kind}:{subject}"),
        at: at(10),
    }
}

fn finding(id: i64, key: &str, status: FindingStatus) -> Finding {
    Finding {
        id,
        kind: "new_program".into(),
        subject: "s".into(),
        subject_label: "s".into(),
        tier: Tier::Unusual,
        title: "t".into(),
        why: "w".into(),
        evidence: vec![],
        dedupe_key: key.into(),
        status,
        first_at: at(1),
        last_at: at(2),
        hits: 1,
        decided_at: None,
        snoozed_until: None,
    }
}

fn expectation(kind: &str, subject: &str) -> Expectation {
    Expectation {
        id: 1,
        kind: kind.into(),
        subject: subject.into(),
        note: None,
        created_at: at(3),
        source_finding: None,
        ceiling: None,
    }
}

const ROOMY: ReconcileSettings = ReconcileSettings { max_new: 100 };

fn run(
    candidates: Vec<Candidate>,
    existing: &[Finding],
    expectations: &[Expectation],
) -> syntrac_core::findings::Reconciled {
    reconcile(candidates, existing, expectations, at(10), &ROOMY)
}

// ---- reconcile

#[test]
fn a_new_candidate_is_inserted() {
    let result = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &[], &[]);

    assert_eq!(result.writes.len(), 1);
    assert!(matches!(&result.writes[0], FindingWrite::Insert(c) if c.subject == "a.exe"));
    assert_eq!((result.suppressed, result.overflow), (0, 0));
}

#[test]
fn what_the_user_said_is_expected_is_not_raised_again_and_is_counted() {
    let result =
        run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &[], &[expectation("new_program", "a.exe")]);

    assert!(result.writes.is_empty());
    assert_eq!(result.suppressed, 1);
}

#[test]
fn an_expectation_covers_only_its_own_kind_and_exactly_its_own_subject() {
    let result = run(
        vec![
            candidate("new_program", "a.exe", Tier::Unusual),
            candidate("new_start_entry", "a.exe", Tier::Unusual),
            candidate("new_program", "a.exe.evil", Tier::Unusual),
            candidate("new_program", "A.EXE", Tier::Unusual),
        ],
        &[],
        &[expectation("new_program", "a.exe")],
    );

    let subjects: Vec<String> = result
        .writes
        .iter()
        .map(|write| match write {
            FindingWrite::Insert(c) => format!("{}:{}", c.kind, c.subject),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(subjects, vec!["new_start_entry:a.exe", "new_program:a.exe.evil", "new_program:A.EXE"]);
    assert_eq!(result.suppressed, 1);
}

#[test]
fn the_same_thing_seen_again_touches_the_open_finding_instead_of_adding_another() {
    let existing = [finding(7, "new_program:a.exe", FindingStatus::Open)];

    let result = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &existing, &[]);

    assert_eq!(result.writes, vec![FindingWrite::Touch { id: 7, at: at(10) }]);
}

#[test]
fn a_finding_the_user_marked_not_expected_stays_and_is_touched() {
    let existing = [finding(7, "new_program:a.exe", FindingStatus::NotExpected)];

    let result = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &existing, &[]);

    assert_eq!(result.writes, vec![FindingWrite::Touch { id: 7, at: at(10) }]);
}

#[test]
fn a_finding_marked_expected_this_time_is_left_alone() {
    let existing = [finding(7, "new_program:a.exe", FindingStatus::Expected)];

    let result = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &existing, &[]);

    assert!(result.writes.is_empty());
}

#[test]
fn a_snoozed_finding_is_quiet_until_the_snooze_ends_and_then_comes_back() {
    let mut snoozed = finding(7, "new_program:a.exe", FindingStatus::Snoozed);
    snoozed.snoozed_until = Some(at(12));
    let quiet = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &[snoozed.clone()], &[]);
    assert!(quiet.writes.is_empty());

    snoozed.snoozed_until = Some(at(10));
    let back = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &[snoozed], &[]);
    assert_eq!(back.writes, vec![FindingWrite::Reopen { id: 7, at: at(10) }]);
}

#[test]
fn a_resolved_finding_that_happens_again_is_reopened() {
    let existing = [finding(7, "new_program:a.exe", FindingStatus::Resolved)];

    let result = run(vec![candidate("new_program", "a.exe", Tier::Unusual)], &existing, &[]);

    assert_eq!(result.writes, vec![FindingWrite::Reopen { id: 7, at: at(10) }]);
}

#[test]
fn the_same_thing_found_twice_in_one_run_is_inserted_once() {
    let result = run(
        vec![candidate("new_program", "a.exe", Tier::Unusual), candidate("new_program", "a.exe", Tier::Unusual)],
        &[],
        &[],
    );

    assert_eq!(result.writes.len(), 1);
}

#[test]
fn when_there_is_a_cap_the_most_serious_come_first_and_the_rest_are_counted_not_lost_silently() {
    let result = reconcile(
        vec![
            candidate("new_program", "u1", Tier::Unusual),
            candidate("new_program", "s1", Tier::Suspicious),
            candidate("new_program", "u2", Tier::Unusual),
        ],
        &[],
        &[],
        at(10),
        &ReconcileSettings { max_new: 2 },
    );

    let subjects: Vec<&str> = result
        .writes
        .iter()
        .filter_map(|write| match write {
            FindingWrite::Insert(c) => Some(c.subject.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(subjects, vec!["s1", "u1"]);
    assert_eq!(result.overflow, 1);
}

// ---- the user's decisions

fn effect(decision: Decision, status: FindingStatus) -> DecisionEffect {
    apply_decision(&finding(7, "k", status), &decision, at(10)).unwrap()
}

#[test]
fn expected_this_time_only_changes_the_status() {
    let result = effect(Decision::ExpectedThisTime, FindingStatus::Open);

    assert_eq!(result.status, FindingStatus::Expected);
    assert_eq!(result.snoozed_until, None);
    assert_eq!(result.new_expectation, None);
}

#[test]
fn always_expected_also_creates_an_expectation_for_the_kind_and_subject_of_the_finding() {
    let result = effect(Decision::AlwaysExpected { note: Some("my backup".into()) }, FindingStatus::Open);

    assert_eq!(result.status, FindingStatus::Expected);
    let expectation = result.new_expectation.expect("an expectation");
    assert_eq!((expectation.kind.as_str(), expectation.subject.as_str()), ("new_program", "s"));
    assert_eq!(expectation.note.as_deref(), Some("my backup"));
    assert_eq!(expectation.source_finding, Some(7));
}

#[test]
fn not_expected_keeps_the_finding_visible_as_a_concern() {
    assert_eq!(effect(Decision::NotExpected, FindingStatus::Open).status, FindingStatus::NotExpected);
}

#[test]
fn snoozing_sets_when_it_comes_back() {
    let result = effect(Decision::Snooze { days: 7 }, FindingStatus::Open);

    assert_eq!(result.status, FindingStatus::Snoozed);
    assert_eq!(result.snoozed_until, Some(at(17)));
}

#[test]
fn a_snooze_must_be_between_one_day_and_a_year() {
    for days in [0, 366] {
        assert!(apply_decision(&finding(7, "k", FindingStatus::Open), &Decision::Snooze { days }, at(10)).is_err());
    }
    assert!(apply_decision(&finding(7, "k", FindingStatus::Open), &Decision::Snooze { days: 365 }, at(10)).is_ok());
}

#[test]
fn any_decision_can_be_undone_by_reopening() {
    let result = effect(Decision::Reopen, FindingStatus::Expected);

    assert_eq!(result.status, FindingStatus::Open);
    assert_eq!(result.snoozed_until, None);
}

#[test]
fn a_note_is_cleaned_and_limited() {
    let long = "x".repeat(500);
    assert!(apply_decision(
        &finding(7, "k", FindingStatus::Open),
        &Decision::AlwaysExpected { note: Some(long) },
        at(10)
    )
    .is_err());
    let cleaned = apply_decision(
        &finding(7, "k", FindingStatus::Open),
        &Decision::AlwaysExpected { note: Some("  fine\u{0007} note ".into()) },
        at(10),
    )
    .unwrap();
    assert_eq!(cleaned.new_expectation.unwrap().note.as_deref(), Some("fine note"));
}

#[test]
fn status_and_tier_names_round_trip_and_unknown_names_are_refused() {
    for status in [
        FindingStatus::Open,
        FindingStatus::Expected,
        FindingStatus::NotExpected,
        FindingStatus::Snoozed,
        FindingStatus::Resolved,
    ] {
        assert_eq!(FindingStatus::parse(status.as_str()), Some(status));
    }
    for tier in [Tier::Unusual, Tier::Suspicious, Tier::Confirmed] {
        assert_eq!(Tier::parse(tier.as_str()), Some(tier));
    }
    assert_eq!(FindingStatus::parse("bogus"), None);
    assert_eq!(Tier::parse("malicious"), None);
}
