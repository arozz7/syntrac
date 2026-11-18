//! Findings: what a detector found, how repeats and expectations are handled, and what each decision of the user does. All pure.

use syntrac_core::findings::{
    apply_decision, build_scope_of, folder_scope_of, reconcile, Candidate, Decision, DecisionEffect, Expectation,
    ExpectationScope, Finding, FindingStatus, FindingWrite, ReconcileSettings, Reconciled, Tier,
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
        level: None,
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
        level: None,
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
    apply_decision(&finding(7, "k", status), &decision, at(10), "c:").unwrap()
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
    let result = effect(
        Decision::AlwaysExpected { note: Some("my backup".into()), scope: ExpectationScope::ThisItem },
        FindingStatus::Open,
    );

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
        assert!(
            apply_decision(&finding(7, "k", FindingStatus::Open), &Decision::Snooze { days }, at(10), "c:").is_err()
        );
    }
    assert!(
        apply_decision(&finding(7, "k", FindingStatus::Open), &Decision::Snooze { days: 365 }, at(10), "c:").is_ok()
    );
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
        &Decision::AlwaysExpected { note: Some(long), scope: ExpectationScope::ThisItem },
        at(10),
        "c:",
    )
    .is_err());
    let cleaned = apply_decision(
        &finding(7, "k", FindingStatus::Open),
        &Decision::AlwaysExpected { note: Some("  fine\u{0007} note ".into()), scope: ExpectationScope::ThisItem },
        at(10),
        "c:",
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

// ---- a folder scope (everything under a program's install folder; docs/ALERTS-PLAN.md, F6)

fn program_finding(kind: &str, subject: &str) -> Finding {
    Finding { kind: kind.into(), subject: subject.into(), ..finding(7, "k", FindingStatus::Open) }
}

fn folder_expectation(root: &str) -> Expectation {
    expectation("new_program", &format!("folder:{root}"))
}

fn build_expectation(root: &str) -> Expectation {
    expectation("new_program", &format!("build:{root}"))
}

fn raised(result: &Reconciled) -> Vec<String> {
    result
        .writes
        .iter()
        .filter_map(|write| match write {
            FindingWrite::Insert(c) => Some(format!("{}:{}", c.kind, c.subject)),
            _ => None,
        })
        .collect()
}

fn always(scope: ExpectationScope) -> Decision {
    Decision::AlwaysExpected { note: None, scope }
}

#[test]
fn a_folder_expectation_covers_programs_below_that_folder_of_its_own_kind_and_nothing_else() {
    let result = run(
        vec![
            candidate("new_program", r"c:\program files\acme\bin\acme.exe", Tier::Unusual),
            candidate("new_program", r"c:\program files\acme\deep\er\x.exe", Tier::Unusual),
            candidate("new_program", r"c:\program files\acme2\bin\y.exe", Tier::Unusual),
            candidate("new_program", r"c:\program files\other\z.exe", Tier::Unusual),
            candidate("new_start_entry", r"c:\program files\acme\bin\acme.exe", Tier::Unusual),
        ],
        &[],
        &[folder_expectation(r"c:\program files\acme")],
    );

    assert_eq!(
        raised(&result),
        vec![
            r"new_program:c:\program files\acme2\bin\y.exe",
            r"new_program:c:\program files\other\z.exe",
            r"new_start_entry:c:\program files\acme\bin\acme.exe",
        ],
        "a folder named like the start of another is not below it, and another kind is not covered"
    );
    assert_eq!(result.suppressed, 2);
}

#[test]
fn an_older_folder_expectation_for_a_writable_folder_is_still_honoured() {
    // Kept on purpose (nothing is switched off by an upgrade); the Findings tab flags it.
    let result = run(
        vec![candidate("new_program", r"c:\users\someone\.cargo\bin\cargo.exe", Tier::Unusual)],
        &[],
        &[folder_expectation(r"c:\users\someone\.cargo")],
    );
    assert!(raised(&result).is_empty());
    assert_eq!(result.suppressed, 1);
}

#[test]
fn a_folder_expectation_for_a_protected_folder_does_not_cover_a_writable_folder_inside_it() {
    let result = run(
        vec![
            candidate("new_program", r"c:\windows\system32\new.exe", Tier::Unusual),
            candidate("new_program", r"c:\windows\temp\dropped.exe", Tier::Unusual),
            candidate("new_program", r"c:\windows\tasks\dropped.exe", Tier::Unusual),
            candidate("new_program", r"c:\windows\system32\tasks\dropped.exe", Tier::Unusual),
            candidate("new_program", r"c:\windows\system32\spool\drivers\color\dropped.exe", Tier::Unusual),
        ],
        &[],
        &[folder_expectation(r"c:\windows")],
    );
    let mut found = raised(&result);
    found.sort();
    let mut expected = vec![
        r"new_program:c:\windows\temp\dropped.exe".to_owned(),
        r"new_program:c:\windows\tasks\dropped.exe".to_owned(),
        r"new_program:c:\windows\system32\tasks\dropped.exe".to_owned(),
        r"new_program:c:\windows\system32\spool\drivers\color\dropped.exe".to_owned(),
    ];
    expected.sort();
    assert_eq!(found, expected);
    assert_eq!(result.suppressed, 1);
}

#[test]
fn a_build_folder_covers_new_programs_below_it_and_no_other_kind() {
    let result = run(
        vec![
            candidate("new_program", r"d:\src\app\target\debug\app.exe", Tier::Suspicious),
            candidate("new_program", r"d:\src2\app.exe", Tier::Unusual),
            candidate("new_start_entry", r"d:\src\app\target\debug\app.exe", Tier::Unusual),
            candidate("changed_file", r"d:\src\app\target\debug\app.exe", Tier::Unusual),
        ],
        &[],
        &[build_expectation(r"d:\src")],
    );
    let mut found = raised(&result);
    found.sort();
    assert_eq!(
        found,
        vec![
            r"changed_file:d:\src\app\target\debug\app.exe".to_owned(),
            r"new_program:d:\src2\app.exe".to_owned(),
            r"new_start_entry:d:\src\app\target\debug\app.exe".to_owned(),
        ],
        "a sibling folder and the other kinds are still raised"
    );
    assert_eq!(result.suppressed, 1);
}

#[test]
fn always_expected_for_the_folder_stores_the_install_folder_of_a_protected_program() {
    let finding = program_finding("new_program", r"c:\program files\acme\bin\acme.exe");

    let effect = apply_decision(&finding, &always(ExpectationScope::ThisFolder), at(10), "c:").unwrap();

    let expectation = effect.new_expectation.expect("an expectation");
    assert_eq!(
        (expectation.kind.as_str(), expectation.subject.as_str()),
        ("new_program", r"folder:c:\program files\acme")
    );
    assert_eq!(effect.status, FindingStatus::Expected);
}

#[test]
fn a_whole_folder_is_refused_where_a_program_could_simply_be_copied_in() {
    let folder = always(ExpectationScope::ThisFolder);
    for (path, why) in [
        (r"c:\users\someone\.cargo\bin\cargo.exe", "a user's tool folder"),
        (r"c:\users\someone\downloads\x\setup.exe", "downloads"),
        (r"c:\windows\temp\x.exe", "windows temp"),
        (r"c:\windows\tasks\x.exe", "a writable folder inside windows"),
        (r"d:\program files\acme\a.exe", "another drive's program files"),
        (r"c:\programdata\acme\a.exe", "programdata"),
    ] {
        let error = apply_decision(&program_finding("new_program", path), &folder, at(10), "c:").unwrap_err();
        assert!(error.0.contains("written"), "{why}: {error}");
    }
}

#[test]
fn a_folder_scope_is_only_for_programs_and_never_for_a_drive_or_a_path_with_no_folder() {
    let folder = always(ExpectationScope::ThisFolder);
    assert!(apply_decision(&program_finding("new_start_entry", r"c:\a\b.exe"), &folder, at(10), "c:").is_err());
    assert!(
        apply_decision(&program_finding("new_program", r"c:\tool.exe"), &folder, at(10), "c:").is_err(),
        "a drive root"
    );
    assert!(apply_decision(&program_finding("new_program", "tool.exe"), &folder, at(10), "c:").is_err(), "no folder");
}

#[test]
fn the_folder_a_finding_could_be_expected_for_is_offered_only_where_it_makes_sense() {
    assert_eq!(
        folder_scope_of("new_program", r"c:\program files\vendor\app\tool.exe", "c:").as_deref(),
        Some(r"c:\program files\vendor")
    );
    assert_eq!(folder_scope_of("new_program", r"c:\tool.exe", "c:"), None);
    assert_eq!(folder_scope_of("changed_file", r"c:\a\b\c.exe", "c:"), None);
    assert_eq!(folder_scope_of("new_program", r"c:\users\someone\.cargo\bin\cargo.exe", "c:"), None, "writable");
    assert_eq!(folder_scope_of("new_program", r"d:\program files\acme\a.exe", "c:"), None, "not the windows drive");
    assert_eq!(folder_scope_of("new_program", r"c:\windows\system32\spool\drivers\color\x.exe", "c:"), None);
}

#[test]
fn a_build_folder_is_chosen_from_a_new_program_in_a_writable_tool_or_project_folder() {
    let effect = apply_decision(
        &program_finding("new_program", r"c:\users\someone\.cargo\bin\cargo.exe"),
        &always(ExpectationScope::BuildFolder),
        at(10),
        "c:",
    )
    .unwrap();
    let expectation = effect.new_expectation.expect("an expectation");
    assert_eq!(
        (expectation.kind.as_str(), expectation.subject.as_str()),
        ("new_program", r"build:c:\users\someone\.cargo")
    );
    assert_eq!(effect.status, FindingStatus::Expected);

    assert_eq!(build_scope_of("new_program", r"d:\src\app\target\debug\app.exe", "c:").as_deref(), Some(r"d:\src"));
}

#[test]
fn a_build_folder_is_refused_for_drop_places_protected_folders_and_other_kinds() {
    let decision = always(ExpectationScope::BuildFolder);
    for (kind, path) in [
        ("new_program", r"c:\users\someone\appdata\local\temp\toolx\x.exe"),
        ("new_program", r"c:\users\someone\downloads\x.exe"),
        ("new_program", r"c:\users\someone\desktop\x.exe"),
        ("new_program", r"c:\users\someone\x.exe"),
        ("new_program", r"c:\users\someone\appdata\local\x.exe"),
        // The Startup folder and the shared Microsoft folders: where things are dropped and made to look legitimate.
        ("new_program", r"c:\users\someone\appdata\roaming\microsoft\windows\start menu\programs\startup\x.exe"),
        ("new_program", r"c:\users\someone\appdata\local\microsoft\x\y.exe"),
        ("new_program", r"c:\programdata\microsoft\windows\x.exe"),
        ("new_program", r"c:\windows\tasks\x.exe"),
        ("new_program", r"c:\program files\acme\a.exe"),
        ("new_program", r"c:\tool.exe"),
        ("new_program", "tool.exe"),
        ("new_start_entry", r"d:\src\a.exe"),
        ("changed_file", r"d:\src\a.exe"),
    ] {
        assert!(apply_decision(&program_finding(kind, path), &decision, at(10), "c:").is_err(), "{kind} {path}");
        assert_eq!(build_scope_of(kind, path, "c:"), None, "{kind} {path}");
    }
}

// ---- a ceiling ("raise the limit to what I just saw")

fn usage_candidate(subject: &str, level: f64) -> Candidate {
    Candidate { level: Some(level), ..candidate("usage", subject, Tier::Unusual) }
}

fn ceiling_expectation(subject: &str, ceiling: Option<f64>) -> Expectation {
    Expectation { ceiling, ..expectation("usage", subject) }
}

#[test]
fn an_expectation_with_a_ceiling_covers_levels_up_to_it_and_raises_the_finding_again_above_it() {
    let expected = [ceiling_expectation("p.exe|cpu", Some(40.0))];

    let at_the_ceiling = run(vec![usage_candidate("p.exe|cpu", 40.0)], &[], &expected);
    let above = run(vec![usage_candidate("p.exe|cpu", 40.1)], &[], &expected);

    assert!(at_the_ceiling.writes.is_empty());
    assert_eq!(at_the_ceiling.suppressed, 1);
    assert_eq!(above.writes.len(), 1, "more than the level the user accepted is raised again");
    assert_eq!(above.suppressed, 0);
}

#[test]
fn an_expectation_without_a_ceiling_covers_every_level() {
    let result = run(vec![usage_candidate("p.exe|cpu", 99.0)], &[], &[ceiling_expectation("p.exe|cpu", None)]);

    assert!(result.writes.is_empty());
}

#[test]
fn raising_the_limit_stores_the_level_the_finding_measured_as_the_ceiling() {
    let finding = Finding { level: Some(55.5), ..program_finding("usage", "p.exe|cpu") };

    let effect = apply_decision(&finding, &Decision::RaiseLimit, at(10), "c:").unwrap();

    assert_eq!(effect.status, FindingStatus::Expected);
    let expectation = effect.new_expectation.expect("an expectation");
    assert_eq!((expectation.kind.as_str(), expectation.subject.as_str()), ("usage", "p.exe|cpu"));
    assert_eq!(expectation.ceiling, Some(55.5));
}

#[test]
fn only_a_usage_finding_that_measured_a_level_can_have_its_limit_raised() {
    assert!(apply_decision(&program_finding("new_program", "a.exe"), &Decision::RaiseLimit, at(10), "c:").is_err());
    assert!(
        apply_decision(&program_finding("usage", "p.exe|cpu"), &Decision::RaiseLimit, at(10), "c:").is_err(),
        "no level"
    );
}
