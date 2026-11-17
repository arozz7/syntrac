//! Contract suite for executable identity: every `IdentityRepository` (in-memory, SQLite, ...) must pass these, and the
//! inspection pass (`inspect_due`) is exercised through them, so the pass and the stores are held to one behaviour.

use std::time::Duration;

use syntrac_core::{
    inspect_due, roll_up_due, EventRepository, FileFacts, FileStamp, IdentityRepository, IdentitySettings,
    InspectError, InspectErrorKind, MetricsRepository, ProcessRepository, RepoError, Retention, RollupRepository,
    Signature, SignatureStatus, Timestamp,
};

use crate::fixtures::{metrics_sample, process_metric};
use crate::mocks::FakeInspector;
use crate::rollup_contract::{secs, settings, start_process, t};

pub trait IdentitySubject:
    IdentityRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}
impl<T> IdentitySubject for T where
    T: IdentityRepository + RollupRepository + EventRepository + ProcessRepository + MetricsRepository + Retention
{
}

const A: &str = r"C:\Program Files\Acme\a.exe";
const B: &str = r"C:\Program Files\Acme\b.exe";
const C: &str = r"C:\Users\x\AppData\Local\Temp\c.exe";

pub fn identity_contract<R: IdentitySubject>(make: impl Fn() -> R) {
    an_empty_database_has_nothing_to_inspect(make());
    every_rolled_up_executable_is_listed_without_an_identity(make());
    a_new_executable_is_inspected_and_saved(make());
    the_pass_is_capped_and_the_most_recently_seen_come_first(make());
    an_unchanged_file_is_not_read_again(make());
    a_program_that_has_not_run_since_is_not_checked_again(make());
    a_changed_file_is_inspected_again(make());
    a_failure_is_recorded_and_retried_after_a_back_off(make());
    a_later_failure_keeps_what_was_learnt_and_a_success_clears_it(make());
    one_failing_file_does_not_stop_the_others(make());
    saving_for_an_unknown_executable_is_refused(make());
    identities_survive_the_purge_of_detail(make());
    paths_are_matched_without_regard_to_case(make());
    every_field_survives_storage(make());
    a_look_cut_short_by_a_stop_is_not_saved_and_ends_the_pass(make());
}

fn run_in_hour<R: IdentitySubject>(repo: &mut R, pid: u32, exe: &str, hour: i64) {
    let process = start_process(repo, pid, t(hour, 0), exe);
    repo.append_metrics(&metrics_sample(
        secs(hour, 10),
        10.0,
        vec![process_metric(pid, Some(process.id), 1.0)],
        0.0,
        0,
    ))
    .unwrap();
}

fn roll_through<R: IdentitySubject>(repo: &mut R, hour: i64) {
    roll_up_due(repo, t(hour, 0), &settings(500)).unwrap();
}

fn facts(size: u64, sha: &str) -> FileFacts {
    FileFacts {
        stamp: FileStamp { size, modified: Some(t(0, 0)) },
        sha256: Some(sha.to_owned()),
        signature: Signature { status: SignatureStatus::Valid, signer: Some("Acme".into()), ..Signature::default() },
        ..FileFacts::default()
    }
}

fn denied() -> InspectError {
    InspectError { kind: InspectErrorKind::AccessDenied, message: "access is denied".into() }
}

fn settings_of(max_per_pass: usize) -> IdentitySettings {
    IdentitySettings { max_per_pass, recheck_every: Duration::from_secs(24 * 3600) }
}

fn pass<R: IdentitySubject>(
    repo: &mut R,
    inspector: &FakeInspector,
    now: Timestamp,
    max: usize,
) -> syntrac_core::IdentityPass {
    inspect_due(repo, inspector, now, &settings_of(max)).unwrap()
}

fn an_empty_database_has_nothing_to_inspect<R: IdentitySubject>(mut repo: R) {
    assert!(repo.identity_entries().unwrap().is_empty());

    let result = pass(&mut repo, &FakeInspector::default(), t(5, 0), 10);

    assert_eq!((result.inspected, result.unchanged, result.failed, result.backlog), (0, 0, 0, false));
}

fn every_rolled_up_executable_is_listed_without_an_identity<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    run_in_hour(&mut repo, 11, B, 1);
    // Detail without a process: the unattributed row must never be offered for inspection.
    repo.append_metrics(&metrics_sample(secs(1, 20), 5.0, vec![process_metric(99, None, 3.0)], 0.0, 0)).unwrap();
    roll_through(&mut repo, 4);

    let entries = repo.identity_entries().unwrap();

    let mut paths: Vec<_> = entries.iter().map(|e| e.exe.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, vec![A.to_owned(), B.to_owned()]);
    assert!(entries.iter().all(|e| e.record.is_none() && e.seen.is_some()));
}

fn a_new_executable_is_inspected_and_saved<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));

    let result = pass(&mut repo, &inspector, t(5, 0), 10);

    assert_eq!((result.inspected, result.unchanged, result.failed, result.backlog), (1, 0, 0, false));
    let record = repo.identity(A).unwrap().expect("saved");
    assert_eq!(record.facts, Some(facts(100, "aa")));
    assert_eq!((record.inspected_at, record.last_attempt, record.failure), (Some(t(5, 0)), t(5, 0), None));
    assert_eq!(inspector.full_inspections(), vec![A.to_owned()]);
    assert_eq!(inspector.stat_count(), 0, "a file never seen before needs no cheap check first");

    let second = pass(&mut repo, &inspector, t(5, 30), 10);
    assert_eq!(second.inspected + second.unchanged + second.failed, 0, "nothing is due again so soon");
}

fn the_pass_is_capped_and_the_most_recently_seen_come_first<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    run_in_hour(&mut repo, 11, B, 1);
    run_in_hour(&mut repo, 12, C, 2);
    roll_through(&mut repo, 6);
    let inspector = FakeInspector::default();
    for path in [A, B, C] {
        inspector.set(path, Ok(facts(1, "x")));
    }

    let first = pass(&mut repo, &inspector, t(8, 0), 2);

    assert_eq!((first.inspected, first.backlog), (2, true));
    assert_eq!(inspector.full_inspections(), vec![C.to_owned(), B.to_owned()], "newest sighting first");
    let second = pass(&mut repo, &inspector, t(8, 1), 2);
    assert_eq!((second.inspected, second.backlog), (1, false));
}

fn an_unchanged_file_is_not_read_again<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(5, 0), 10);
    // The program runs again a day later.
    run_in_hour(&mut repo, 11, A, 30);
    roll_through(&mut repo, 33);

    let result = pass(&mut repo, &inspector, t(34, 0), 10);

    assert_eq!((result.inspected, result.unchanged, result.failed), (0, 1, 0));
    assert_eq!(inspector.stat_count(), 1);
    assert_eq!(inspector.full_inspections().len(), 1, "only the first look read the whole file");
    let record = repo.identity(A).unwrap().unwrap();
    assert_eq!((record.last_attempt, record.inspected_at), (t(34, 0), Some(t(5, 0))), "checked now, measured then");
}

fn a_program_that_has_not_run_since_is_not_checked_again<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(5, 0), 10);

    let result = pass(&mut repo, &inspector, t(200, 0), 10);

    assert_eq!(result.inspected + result.unchanged + result.failed, 0);
    assert_eq!(inspector.stat_count(), 0);
}

fn a_changed_file_is_inspected_again<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(5, 0), 10);
    run_in_hour(&mut repo, 11, A, 30);
    roll_through(&mut repo, 33);
    inspector.set(A, Ok(facts(250, "bb")));

    let result = pass(&mut repo, &inspector, t(34, 0), 10);

    assert_eq!((result.inspected, result.unchanged), (1, 0));
    let record = repo.identity(A).unwrap().unwrap();
    assert_eq!(record.facts, Some(facts(250, "bb")));
    assert_eq!(record.inspected_at, Some(t(34, 0)));
}

fn a_failure_is_recorded_and_retried_after_a_back_off<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Err(denied()));

    let first = pass(&mut repo, &inspector, t(5, 0), 10);

    assert_eq!((first.inspected, first.failed), (0, 1));
    let record = repo.identity(A).unwrap().unwrap();
    assert_eq!(record.facts, None);
    let failure = record.failure.expect("failure recorded");
    assert_eq!((failure.kind, failure.attempts), (InspectErrorKind::AccessDenied, 1));
    assert_eq!(failure.message, "access is denied");

    let too_soon = pass(&mut repo, &inspector, t(5, 30), 10);
    assert_eq!(too_soon.failed, 0, "not retried within the back-off");
    let later = pass(&mut repo, &inspector, t(7, 0), 10);
    assert_eq!(later.failed, 1);
    assert_eq!(repo.identity(A).unwrap().unwrap().failure.unwrap().attempts, 2, "the count grows");
}

fn a_later_failure_keeps_what_was_learnt_and_a_success_clears_it<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(5, 0), 10);
    run_in_hour(&mut repo, 11, A, 30);
    roll_through(&mut repo, 33);
    inspector.set(A, Err(InspectError { kind: InspectErrorKind::NotFound, message: "gone".into() }));

    pass(&mut repo, &inspector, t(34, 0), 10);

    let record = repo.identity(A).unwrap().unwrap();
    assert_eq!(record.facts, Some(facts(100, "aa")), "an uninstalled program keeps what we knew about it");
    assert_eq!(record.inspected_at, Some(t(5, 0)));
    assert_eq!(record.failure.as_ref().unwrap().kind, InspectErrorKind::NotFound);

    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(60, 0), 10);
    assert_eq!(repo.identity(A).unwrap().unwrap().failure, None, "the file is back: the failure is cleared");
}

fn one_failing_file_does_not_stop_the_others<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    run_in_hour(&mut repo, 11, B, 1);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Err(denied()));
    inspector.set(B, Ok(facts(7, "bb")));

    let result = pass(&mut repo, &inspector, t(5, 0), 10);

    assert_eq!((result.inspected, result.failed), (1, 1));
    assert!(repo.identity(B).unwrap().unwrap().facts.is_some());
}

fn saving_for_an_unknown_executable_is_refused<R: IdentitySubject>(mut repo: R) {
    let record = syntrac_core::IdentityRecord {
        exe: syntrac_core::Executable::from_path(r"C:\never\seen.exe").unwrap(),
        facts: None,
        inspected_at: None,
        last_attempt: t(0, 0),
        failure: None,
    };

    assert!(matches!(repo.save_identity(&record), Err(RepoError::NotFound)));
    assert_eq!(repo.identity(r"C:\never\seen.exe").unwrap(), None);
}

fn identities_survive_the_purge_of_detail<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(5, 0), 10);
    let before = repo.identity(A).unwrap();

    repo.purge_before(t(1000, 0)).unwrap();

    assert_eq!(repo.identity(A).unwrap(), before);
}

fn paths_are_matched_without_regard_to_case<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let inspector = FakeInspector::default();
    inspector.set(A, Ok(facts(100, "aa")));
    pass(&mut repo, &inspector, t(5, 0), 10);

    let found = repo.identity(&A.to_uppercase()).unwrap().expect("found by another spelling");

    assert_eq!(found.exe.path, A, "the stored spelling is the one first seen");
}

fn every_field_survives_storage<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    roll_through(&mut repo, 4);
    let full = FileFacts {
        stamp: FileStamp { size: u64::from(u32::MAX) + 5, modified: Some(t(3, 7)) },
        sha256: None,
        signature: Signature {
            status: SignatureStatus::Untrusted,
            signer: Some("CN=Acme, O=Acme Ltd".into()),
            thumbprint: Some("AB12CD".into()),
            kind: Some(syntrac_core::SignatureKind::Catalog),
        },
        version: syntrac_core::VersionInfo {
            company: Some("Acme".into()),
            product: Some("Acme Suite".into()),
            description: Some("Acme helper".into()),
            original_filename: Some("helper.exe".into()),
            file_version: Some("1.2.3.4".into()),
        },
        zone_id: Some(3),
    };
    let record = syntrac_core::IdentityRecord {
        exe: syntrac_core::Executable::from_path(A).unwrap(),
        facts: Some(full.clone()),
        inspected_at: Some(t(5, 0)),
        last_attempt: t(6, 0),
        failure: Some(syntrac_core::IdentityFailure {
            kind: InspectErrorKind::TimedOut,
            message: "took too long".into(),
            attempts: 2,
        }),
    };

    repo.save_identity(&record).unwrap();

    assert_eq!(repo.identity(A).unwrap(), Some(record));
    assert!(full.downloaded());
    let listed = repo.identity_entries().unwrap();
    assert_eq!(listed[0].record.as_ref().unwrap().facts, Some(full), "the listing reads the same fields");
}

/// Gives a repository these executables, each seen running once and rolled up, and returns an instant after all of it (the
/// "now" for a test that inspects them). Used by tests of code that works through `IdentityRepository`.
pub fn seed_executables<R: IdentitySubject>(repo: &mut R, paths: &[&str]) -> Timestamp {
    for (index, path) in paths.iter().enumerate() {
        run_in_hour(repo, 10 + index as u32, path, index as i64);
    }
    let last_hour = paths.len() as i64 + 2;
    roll_through(repo, last_hour);
    t(last_hour + 1, 0)
}

fn a_look_cut_short_by_a_stop_is_not_saved_and_ends_the_pass<R: IdentitySubject>(mut repo: R) {
    run_in_hour(&mut repo, 10, A, 0);
    run_in_hour(&mut repo, 11, B, 1);
    run_in_hour(&mut repo, 12, C, 2);
    roll_through(&mut repo, 6);
    let inspector = FakeInspector::default();
    inspector.set(A, Err(denied()));
    inspector.set(B, Err(denied()));
    inspector.set(C, Ok(facts(1, "c")));
    pass(&mut repo, &inspector, t(8, 0), 10);
    let log_before = inspector.full_inspections().len();

    // The service is stopped while the next pass is on A (the first failure due again, then B).
    inspector.set(A, Err(InspectError { kind: InspectErrorKind::Cancelled, message: "stopping".into() }));
    inspector.set(B, Ok(facts(2, "b")));
    let result = pass(&mut repo, &inspector, t(30, 0), 10);

    assert!(result.stopped, "the pass says it was cut short");
    assert_eq!((result.inspected, result.failed), (0, 0), "a stop is not counted as anything");
    let a = repo.identity(A).unwrap().unwrap();
    assert_eq!(a.failure.unwrap().attempts, 1, "a stop is not a failure of the file");
    assert_eq!(a.last_attempt, t(8, 0), "the cut-short look was not recorded at all");
    assert_eq!(
        inspector.full_inspections().split_off(log_before),
        vec![A.to_owned()],
        "nothing after the stop was read"
    );
    assert!(repo.identity(B).unwrap().unwrap().failure.is_some(), "B was not reached");

    inspector.set(A, Ok(facts(1, "a")));
    let later = pass(&mut repo, &inspector, t(31, 0), 10);
    assert!(!later.stopped);
    assert_eq!(later.inspected, 2, "the next pass looks again at both");
    assert_eq!(repo.identity(A).unwrap().unwrap().failure, None);
}

// ---- the changes of a file's content (docs/ALERTS-PLAN.md, F2)

use syntrac_core::{ChangeFacts, Executable, IdentityChangeRepository, IdentityRecord};

pub trait IdentityChangeSubject: IdentitySubject + IdentityChangeRepository {}
impl<T> IdentityChangeSubject for T where T: IdentitySubject + IdentityChangeRepository {}

pub fn identity_change_contract<R: IdentityChangeSubject>(make: impl Fn() -> R) {
    nothing_has_changed_in_an_empty_database(make());
    only_new_content_is_a_change_not_the_first_reading_or_the_same_content(make());
    a_change_keeps_both_sides_and_when_the_new_content_was_read(make());
    a_failed_look_or_content_that_could_not_be_hashed_is_not_a_change(make());
    changes_survive_the_purge_of_detail(make());
}

fn record(path: &str, facts: Option<FileFacts>, at: Timestamp) -> IdentityRecord {
    IdentityRecord {
        exe: Executable::from_path(path).unwrap(),
        facts,
        inspected_at: Some(at),
        last_attempt: at,
        failure: None,
    }
}

fn signed(sha: Option<&str>, signer: &str, status: SignatureStatus) -> FileFacts {
    FileFacts {
        stamp: FileStamp { size: 10, modified: Some(t(0, 0)) },
        sha256: sha.map(str::to_owned),
        signature: Signature { status, signer: Some(signer.to_owned()), ..Signature::default() },
        ..FileFacts::default()
    }
}

fn known_program<R: IdentityChangeSubject>() -> impl Fn(&mut R) {
    |repo| {
        run_in_hour(repo, 10, A, 0);
        roll_through(repo, 5);
    }
}

fn nothing_has_changed_in_an_empty_database<R: IdentityChangeSubject>(repo: R) {
    assert!(repo.identity_changes_since(t(0, 0)).unwrap().is_empty());
}

fn only_new_content_is_a_change_not_the_first_reading_or_the_same_content<R: IdentityChangeSubject>(mut repo: R) {
    known_program()(&mut repo);

    repo.save_identity(&record(A, Some(signed(Some("aa"), "Acme", SignatureStatus::Valid)), t(1, 0))).unwrap();
    assert!(repo.identity_changes_since(t(0, 0)).unwrap().is_empty(), "the first reading is not a change");

    repo.save_identity(&record(A, Some(signed(Some("aa"), "Acme", SignatureStatus::Valid)), t(2, 0))).unwrap();
    assert!(repo.identity_changes_since(t(0, 0)).unwrap().is_empty(), "the same content read again is not a change");

    repo.save_identity(&record(A, Some(signed(Some("bb"), "Acme", SignatureStatus::Valid)), t(3, 0))).unwrap();
    assert_eq!(repo.identity_changes_since(t(0, 0)).unwrap().len(), 1);
}

fn a_change_keeps_both_sides_and_when_the_new_content_was_read<R: IdentityChangeSubject>(mut repo: R) {
    known_program()(&mut repo);
    repo.save_identity(&record(A, Some(signed(Some("aa"), "Acme", SignatureStatus::Valid)), t(1, 0))).unwrap();
    repo.save_identity(&record(A, Some(signed(Some("bb"), "Someone", SignatureStatus::Untrusted)), t(3, 0))).unwrap();
    repo.save_identity(&record(A, Some(signed(Some("cc"), "Someone", SignatureStatus::Altered)), t(4, 0))).unwrap();

    let changes = repo.identity_changes_since(t(0, 0)).unwrap();

    assert_eq!(changes.len(), 2, "oldest first");
    let first = &changes[0];
    assert_eq!(first.exe.path, A);
    assert_eq!(
        first.old,
        ChangeFacts { sha256: Some("aa".into()), signer: Some("Acme".into()), status: Some(SignatureStatus::Valid) }
    );
    assert_eq!(
        first.new,
        ChangeFacts {
            sha256: Some("bb".into()),
            signer: Some("Someone".into()),
            status: Some(SignatureStatus::Untrusted)
        }
    );
    assert_eq!(first.at, t(3, 0));
    assert_eq!(changes[1].old.sha256.as_deref(), Some("bb"));
    assert_eq!(changes[1].new.status, Some(SignatureStatus::Altered));

    let later = repo.identity_changes_since(t(4, 0)).unwrap();
    assert_eq!(later.len(), 1, "only the changes read at or after the time asked for");
    assert_eq!(later[0].new.sha256.as_deref(), Some("cc"));
}

fn a_failed_look_or_content_that_could_not_be_hashed_is_not_a_change<R: IdentityChangeSubject>(mut repo: R) {
    known_program()(&mut repo);
    repo.save_identity(&record(A, Some(signed(Some("aa"), "Acme", SignatureStatus::Valid)), t(1, 0))).unwrap();

    // A look that failed has no facts; a file too large to hash has no content to compare.
    repo.save_identity(&record(A, None, t(2, 0))).unwrap();
    repo.save_identity(&record(A, Some(signed(None, "Acme", SignatureStatus::Valid)), t(3, 0))).unwrap();

    assert!(repo.identity_changes_since(t(0, 0)).unwrap().is_empty());
}

fn changes_survive_the_purge_of_detail<R: IdentityChangeSubject>(mut repo: R) {
    known_program()(&mut repo);
    repo.save_identity(&record(A, Some(signed(Some("aa"), "Acme", SignatureStatus::Valid)), t(1, 0))).unwrap();
    repo.save_identity(&record(A, Some(signed(Some("bb"), "Acme", SignatureStatus::Valid)), t(3, 0))).unwrap();

    repo.purge_before(Timestamp::from_micros(1_000_000_000_000_000)).unwrap();

    assert_eq!(repo.identity_changes_since(t(0, 0)).unwrap().len(), 1);
}
