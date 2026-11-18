//! Contract suite for findings, expectations and the detection state: every implementation (in-memory, SQLite, ...) must pass
//! these, so the two cannot drift apart. The last tests run whole detection runs through the repositories.

use syntrac_core::findings::{
    apply_decision, Candidate, Decision, ExpectationScope, FindingStatus, FindingWrite, NewExpectation,
    ReconcileSettings, Tier,
};
use syntrac_core::{
    run_detection, DetectError, DetectionStateRepository, Detector, ExpectationRepository, FindingRepository,
    Retention, Timestamp,
};

pub trait FindingsSubject: FindingRepository + ExpectationRepository + DetectionStateRepository + Retention {}
impl<T> FindingsSubject for T where T: FindingRepository + ExpectationRepository + DetectionStateRepository + Retention {}

pub fn findings_contract<R: FindingsSubject>(make: impl Fn() -> R) {
    an_empty_database_has_nothing(make());
    an_inserted_finding_reads_back_whole(make());
    a_measured_level_is_kept_and_a_missing_one_stays_missing(make());
    touching_and_reopening_count_hits_and_keep_the_rest(make());
    findings_are_found_by_their_keys(make());
    a_decision_is_stored_with_its_time_and_unknown_findings_are_reported(make());
    findings_come_newest_first_and_can_be_filtered_and_counted(make());
    expectations_are_unique_per_kind_and_subject_and_can_be_removed(make());
    the_detection_state_remembers_the_first_switch_on_and_the_last_run(make());
    findings_expectations_and_state_survive_the_purge_of_detail(make());
    a_run_stores_then_touches_and_expectations_silence_it(make());
    one_detector_failing_does_not_stop_the_others_and_is_recorded(make());
}

pub fn at(micros: i64) -> Timestamp {
    Timestamp::from_micros(micros)
}

pub fn candidate(subject: &str, tier: Tier, when: i64) -> Candidate {
    Candidate {
        kind: "new_program".into(),
        subject: subject.into(),
        subject_label: format!("{subject} (label)"),
        tier,
        title: format!("New program {subject}"),
        why: "It was first seen after detection was switched on.".into(),
        evidence: vec!["first line".into(), "second line".into()],
        dedupe_key: format!("new_program:{subject}"),
        level: None,
        at: at(when),
    }
}

fn insert<R: FindingsSubject>(repo: &mut R, subject: &str, tier: Tier, when: i64) -> i64 {
    repo.apply_writes(&[FindingWrite::Insert(candidate(subject, tier, when))]).unwrap();
    repo.findings_for_keys(&[format!("new_program:{subject}")]).unwrap()[0].id
}

fn an_empty_database_has_nothing<R: FindingsSubject>(repo: R) {
    assert!(repo.findings(None, 10).unwrap().is_empty());
    assert!(repo.status_counts().unwrap().is_empty());
    assert!(repo.expectations().unwrap().is_empty());
    assert_eq!(repo.detection_state().unwrap(), None);
    assert_eq!(repo.finding(1).unwrap(), None);
}

fn an_inserted_finding_reads_back_whole<R: FindingsSubject>(mut repo: R) {
    let id = insert(&mut repo, "a.exe", Tier::Suspicious, 100);

    let found = repo.finding(id).unwrap().expect("stored");

    assert_eq!((found.kind.as_str(), found.subject.as_str()), ("new_program", "a.exe"));
    assert_eq!(found.subject_label, "a.exe (label)");
    assert_eq!(found.tier, Tier::Suspicious);
    assert_eq!(found.title, "New program a.exe");
    assert_eq!(found.why, "It was first seen after detection was switched on.");
    assert_eq!(found.evidence, vec!["first line".to_owned(), "second line".to_owned()], "lines keep their order");
    assert_eq!(found.dedupe_key, "new_program:a.exe");
    assert_eq!(found.status, FindingStatus::Open);
    assert_eq!((found.first_at, found.last_at, found.hits), (at(100), at(100), 1));
    assert_eq!((found.decided_at, found.snoozed_until), (None, None));
}

fn a_measured_level_is_kept_and_a_missing_one_stays_missing<R: FindingsSubject>(mut repo: R) {
    let mut usage = candidate("p.exe|cpu", Tier::Unusual, 100);
    usage.kind = "usage".into();
    usage.dedupe_key = "usage:p.exe|cpu:1".into();
    usage.level = Some(37.25);
    repo.apply_writes(&[FindingWrite::Insert(usage), FindingWrite::Insert(candidate("a.exe", Tier::Unusual, 100))])
        .unwrap();

    let measured = repo.findings_for_keys(&["usage:p.exe|cpu:1".into()]).unwrap().remove(0);
    let plain = repo.findings_for_keys(&["new_program:a.exe".into()]).unwrap().remove(0);

    assert_eq!(measured.level, Some(37.25));
    assert_eq!(plain.level, None);
}

fn touching_and_reopening_count_hits_and_keep_the_rest<R: FindingsSubject>(mut repo: R) {
    let id = insert(&mut repo, "a.exe", Tier::Unusual, 100);

    repo.apply_writes(&[FindingWrite::Touch { id, at: at(200) }]).unwrap();
    let touched = repo.finding(id).unwrap().unwrap();
    assert_eq!(
        (touched.hits, touched.first_at, touched.last_at, touched.status),
        (2, at(100), at(200), FindingStatus::Open)
    );

    repo.set_status(id, FindingStatus::Snoozed, at(250), Some(at(900))).unwrap();
    repo.apply_writes(&[FindingWrite::Reopen { id, at: at(1000) }]).unwrap();
    let reopened = repo.finding(id).unwrap().unwrap();
    assert_eq!((reopened.status, reopened.hits, reopened.last_at), (FindingStatus::Open, 3, at(1000)));
    assert_eq!(reopened.snoozed_until, None, "a reopened finding is no longer snoozed");
}

fn findings_are_found_by_their_keys<R: FindingsSubject>(mut repo: R) {
    insert(&mut repo, "a.exe", Tier::Unusual, 100);
    insert(&mut repo, "b.exe", Tier::Unusual, 110);

    let found = repo.findings_for_keys(&["new_program:b.exe".into(), "new_program:none.exe".into()]).unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].subject, "b.exe");
    assert!(repo.findings_for_keys(&[]).unwrap().is_empty());
}

fn a_decision_is_stored_with_its_time_and_unknown_findings_are_reported<R: FindingsSubject>(mut repo: R) {
    let id = insert(&mut repo, "a.exe", Tier::Unusual, 100);

    assert!(repo.set_status(id, FindingStatus::NotExpected, at(300), None).unwrap());
    let found = repo.finding(id).unwrap().unwrap();
    assert_eq!(
        (found.status, found.decided_at, found.snoozed_until),
        (FindingStatus::NotExpected, Some(at(300)), None)
    );

    assert!(repo.set_status(id, FindingStatus::Snoozed, at(400), Some(at(5000))).unwrap());
    assert_eq!(repo.finding(id).unwrap().unwrap().snoozed_until, Some(at(5000)));

    assert!(!repo.set_status(9999, FindingStatus::Expected, at(1), None).unwrap());
}

fn findings_come_newest_first_and_can_be_filtered_and_counted<R: FindingsSubject>(mut repo: R) {
    let a = insert(&mut repo, "a.exe", Tier::Unusual, 100);
    let b = insert(&mut repo, "b.exe", Tier::Unusual, 200);
    insert(&mut repo, "c.exe", Tier::Unusual, 300);
    repo.apply_writes(&[FindingWrite::Touch { id: a, at: at(400) }]).unwrap();
    repo.set_status(b, FindingStatus::Expected, at(500), None).unwrap();

    let all: Vec<String> = repo.findings(None, 10).unwrap().into_iter().map(|f| f.subject).collect();
    assert_eq!(all, vec!["a.exe", "c.exe", "b.exe"], "by last activity, newest first");

    let open: Vec<String> =
        repo.findings(Some(FindingStatus::Open), 10).unwrap().into_iter().map(|f| f.subject).collect();
    assert_eq!(open, vec!["a.exe", "c.exe"]);
    assert_eq!(repo.findings(None, 1).unwrap().len(), 1);

    let mut counts = repo.status_counts().unwrap();
    counts.sort_by_key(|(status, _)| status.as_str());
    assert_eq!(counts, vec![(FindingStatus::Expected, 1), (FindingStatus::Open, 2)]);
}

fn new_expectation(kind: &str, subject: &str, note: Option<&str>) -> NewExpectation {
    NewExpectation {
        kind: kind.into(),
        subject: subject.into(),
        note: note.map(str::to_owned),
        source_finding: None,
        ceiling: None,
    }
}

fn expectations_are_unique_per_kind_and_subject_and_can_be_removed<R: FindingsSubject>(mut repo: R) {
    let first = repo.add_expectation(&new_expectation("new_program", "a.exe", Some("mine")), at(10)).unwrap();
    let other_kind = repo.add_expectation(&new_expectation("new_start_entry", "a.exe", None), at(20)).unwrap();
    let again = repo.add_expectation(&new_expectation("new_program", "a.exe", Some("renamed")), at(30)).unwrap();

    assert_eq!(first, again, "the same kind and subject is one expectation");
    assert_ne!(first, other_kind);
    let list = repo.expectations().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].id, list[0].note.as_deref(), list[0].created_at), (first, Some("renamed"), at(10)));
    assert_eq!(list[1].kind, "new_start_entry");

    assert!(repo.remove_expectation(first).unwrap());
    assert!(!repo.remove_expectation(first).unwrap());
    assert_eq!(repo.expectations().unwrap().len(), 1);
}

fn the_detection_state_remembers_the_first_switch_on_and_the_last_run<R: FindingsSubject>(mut repo: R) {
    assert_eq!(repo.switch_on(at(100)).unwrap(), at(100));
    assert_eq!(repo.switch_on(at(500)).unwrap(), at(100), "the first switch-on wins");

    repo.record_run(at(600), Some("one detector failed")).unwrap();
    let state = repo.detection_state().unwrap().unwrap();
    assert_eq!(
        (state.switched_on_at, state.last_run_at, state.last_error.as_deref()),
        (at(100), Some(at(600)), Some("one detector failed"))
    );

    repo.record_run(at(700), None).unwrap();
    let state = repo.detection_state().unwrap().unwrap();
    assert_eq!((state.last_run_at, state.last_error), (Some(at(700)), None));
}

fn findings_expectations_and_state_survive_the_purge_of_detail<R: FindingsSubject>(mut repo: R) {
    insert(&mut repo, "a.exe", Tier::Unusual, 100);
    repo.add_expectation(&new_expectation("new_program", "b.exe", None), at(10)).unwrap();
    repo.switch_on(at(5)).unwrap();

    repo.purge_before(at(1_000_000_000_000_000)).unwrap();

    assert_eq!(repo.findings(None, 10).unwrap().len(), 1);
    assert_eq!(repo.expectations().unwrap().len(), 1);
    assert!(repo.detection_state().unwrap().is_some());
}

/// A detector that returns the same candidates every time.
struct Fixed(Vec<Candidate>);
impl Detector for Fixed {
    fn name(&self) -> &'static str {
        "fixed"
    }
    fn detect(&self, _since: Timestamp, _now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        Ok(self.0.clone())
    }
}

struct Broken;
impl Detector for Broken {
    fn name(&self) -> &'static str {
        "broken"
    }
    fn detect(&self, _since: Timestamp, _now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        Err(DetectError("it could not read its data".into()))
    }
}

const SETTINGS: ReconcileSettings = ReconcileSettings { max_new: 50 };

fn a_run_stores_then_touches_and_expectations_silence_it<R: FindingsSubject>(mut repo: R) {
    let detector = Fixed(vec![candidate("a.exe", Tier::Unusual, 0), candidate("b.exe", Tier::Suspicious, 0)]);

    let first = run_detection(&[&detector], &mut repo, at(1000), &SETTINGS).unwrap();
    assert_eq!((first.candidates, first.inserted, first.touched), (2, 2, 0));
    assert_eq!(repo.detection_state().unwrap().unwrap().switched_on_at, at(1000));

    let second = run_detection(&[&detector], &mut repo, at(2000), &SETTINGS).unwrap();
    assert_eq!((second.inserted, second.touched), (0, 2));
    assert_eq!(repo.findings(None, 10).unwrap().iter().map(|f| f.hits).collect::<Vec<_>>(), vec![2, 2]);
    assert_eq!(repo.detection_state().unwrap().unwrap().switched_on_at, at(1000), "still the first switch-on");

    // The user says a.exe is always expected: it is marked and an expectation is stored, and the run stops raising it.
    let a = repo.findings_for_keys(&["new_program:a.exe".into()]).unwrap().remove(0);
    let effect =
        apply_decision(&a, &Decision::AlwaysExpected { note: None, scope: ExpectationScope::ThisItem }, at(2500), "c:")
            .unwrap();
    repo.set_status(a.id, effect.status, at(2500), effect.snoozed_until).unwrap();
    repo.add_expectation(&effect.new_expectation.unwrap(), at(2500)).unwrap();

    let third = run_detection(&[&detector], &mut repo, at(3000), &SETTINGS).unwrap();
    assert_eq!((third.suppressed, third.touched, third.inserted), (1, 1, 0));
    assert_eq!(
        repo.findings_for_keys(&["new_program:a.exe".into()]).unwrap()[0].hits,
        2,
        "a.exe was not touched again"
    );
}

fn one_detector_failing_does_not_stop_the_others_and_is_recorded<R: FindingsSubject>(mut repo: R) {
    let good = Fixed(vec![candidate("a.exe", Tier::Unusual, 0)]);

    let report = run_detection(&[&Broken, &good], &mut repo, at(1000), &SETTINGS).unwrap();

    assert_eq!(report.inserted, 1);
    assert_eq!(report.failed, vec![("broken".to_owned(), "it could not read its data".to_owned())]);
    let state = repo.detection_state().unwrap().unwrap();
    assert_eq!(state.last_error.as_deref(), Some("broken: it could not read its data"));
    assert_eq!(state.last_run_at, Some(at(1000)));
}
