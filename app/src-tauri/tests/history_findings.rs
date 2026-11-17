//! The Findings tab's read model: what was found, what the user decided, what they said is expected, and whether detection is on.

use syntrac_app_lib::history_findings::{findings_view, FindingsView};
use syntrac_core::findings::{Candidate, FindingStatus, FindingWrite, NewExpectation, Tier};
use syntrac_core::{DetectionStateRepository, ExpectationRepository, FindingRepository, Timestamp};
use syntrac_testkit::InMemoryRepository;

const SECOND: i64 = 1_000_000;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * SECOND)
}

fn candidate(subject: &str, tier: Tier, when: i64) -> Candidate {
    Candidate {
        kind: "new_program".into(),
        subject: subject.into(),
        subject_label: subject.to_uppercase(),
        tier,
        title: format!("New program {subject}"),
        why: "It was first seen after detection was switched on.".into(),
        evidence: vec!["first line".into(), "second line".into()],
        dedupe_key: format!("new_program:{subject}"),
        at: at(when),
    }
}

fn repo() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    repo.apply_writes(&[
        FindingWrite::Insert(candidate("a.exe", Tier::Unusual, 100)),
        FindingWrite::Insert(candidate("b.exe", Tier::Suspicious, 200)),
        FindingWrite::Insert(candidate("c.exe", Tier::Unusual, 300)),
    ])
    .unwrap();
    repo.set_status(2, FindingStatus::Expected, at(400), None).unwrap();
    repo.set_status(3, FindingStatus::Snoozed, at(410), Some(at(9_000))).unwrap();
    repo
}

fn view(repo: &InMemoryRepository, status: Option<&str>) -> FindingsView {
    findings_view(repo, status, 100).unwrap()
}

#[test]
fn a_finding_carries_everything_the_screen_shows_in_milliseconds_and_plain_names() {
    let view = view(&repo(), Some("snoozed"));

    let found = &view.findings[0];
    assert_eq!((found.id, found.kind.as_str(), found.subject.as_str()), (3, "new_program", "c.exe"));
    assert_eq!((found.subject_label.as_str(), found.tier, found.status), ("C.EXE", "unusual", "snoozed"));
    assert_eq!(found.evidence, vec!["first line".to_owned(), "second line".to_owned()]);
    assert_eq!((found.first_at_ms, found.last_at_ms, found.hits), (300_000, 300_000, 1));
    assert_eq!((found.decided_at_ms, found.snoozed_until_ms), (Some(410_000), Some(9_000_000)));
}

#[test]
fn the_counts_cover_every_status_whatever_is_listed() {
    let view = view(&repo(), Some("open"));

    assert_eq!((view.counts.open, view.counts.expected, view.counts.snoozed), (1, 1, 1));
    assert_eq!((view.counts.not_expected, view.counts.resolved), (0, 0));
    assert_eq!(view.findings.len(), 1, "only the open one is listed");
}

#[test]
fn no_status_lists_everything_newest_first_and_a_limit_applies() {
    let all = view(&repo(), None);
    assert_eq!(all.findings.iter().map(|f| f.subject.as_str()).collect::<Vec<_>>(), vec!["c.exe", "b.exe", "a.exe"]);

    let one = findings_view(&repo(), None, 1).unwrap();
    assert_eq!(one.findings.len(), 1);
}

#[test]
fn an_unknown_status_is_refused_in_plain_words() {
    let error = findings_view(&repo(), Some("malicious"), 10).unwrap_err();

    assert!(error.to_string().contains("status"), "{error}");
}

#[test]
fn detection_is_off_until_a_run_has_switched_it_on_and_then_says_when() {
    let mut repo = repo();
    assert_eq!(view(&repo, None).detection, None);

    repo.switch_on(at(1_000)).unwrap();
    repo.record_run(at(1_300), Some("fails: no data")).unwrap();

    let detection = view(&repo, None).detection.expect("on");
    assert_eq!((detection.switched_on_ms, detection.last_run_ms), (1_000_000, Some(1_300_000)));
    assert_eq!(detection.last_error.as_deref(), Some("fails: no data"));
}

#[test]
fn what_the_user_said_is_expected_is_listed_with_its_note_and_ceiling() {
    let mut repo = repo();
    repo.add_expectation(
        &NewExpectation {
            kind: "new_program".into(),
            subject: "b.exe".into(),
            note: Some("my backup".into()),
            source_finding: Some(2),
            ceiling: Some(2.5),
        },
        at(500),
    )
    .unwrap();

    let view = view(&repo, None);

    let expectation = &view.expectations[0];
    assert_eq!((expectation.kind.as_str(), expectation.subject.as_str()), ("new_program", "b.exe"));
    assert_eq!(
        (expectation.note.as_deref(), expectation.created_ms, expectation.source_finding),
        (Some("my backup"), 500_000, Some(2))
    );
    assert_eq!(expectation.ceiling, Some(2.5));
}

#[test]
fn an_empty_database_gives_an_empty_view() {
    let view = view(&InMemoryRepository::default(), None);

    assert!(view.findings.is_empty() && view.expectations.is_empty());
    assert_eq!((view.counts.open, view.detection), (0, None));
}

#[test]
fn a_program_finding_offers_the_install_folder_as_a_scope_and_other_findings_do_not() {
    let mut repo = InMemoryRepository::default();
    let mut program = candidate(r"c:\users\someone\.cargo\bin\cargo.exe", Tier::Suspicious, 100);
    program.dedupe_key = "new_program:cargo".into();
    let mut other = candidate(r"c:\users\someone\.cargo\bin\cargo.exe", Tier::Suspicious, 200);
    other.kind = "new_start_entry".into();
    other.dedupe_key = "new_start_entry:cargo".into();
    repo.apply_writes(&[FindingWrite::Insert(program), FindingWrite::Insert(other)]).unwrap();

    let view = view(&repo, None);

    let by_kind = |kind: &str| view.findings.iter().find(|f| f.kind == kind).unwrap();
    assert_eq!(by_kind("new_program").folder_scope.as_deref(), Some(r"c:\users\someone\.cargo"));
    assert_eq!(by_kind("new_start_entry").folder_scope, None);
}
