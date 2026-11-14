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
