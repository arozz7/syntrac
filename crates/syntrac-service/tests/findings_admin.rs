//! The service's answers to the pipe's findings commands: what it accepts, what it refuses, and what each decision stores.

use std::sync::Arc;

use syntrac_core::findings::{Candidate, FindingStatus, FindingWrite, Tier};
use syntrac_core::{ExpectationRepository, FindingRepository, Timestamp};
use syntrac_ipc::protocol::{Request, Response};
use syntrac_ipc::serve::Handler;
use syntrac_service::{FindingsAdmin, FindingsControl, KnownProductsAdmin, ServiceHandler};
use syntrac_testkit::{InMemoryRepository, ManualClock};

const NOW: i64 = 5_000_000_000;

fn candidate(subject: &str) -> Candidate {
    Candidate {
        kind: "new_program".into(),
        subject: subject.into(),
        subject_label: subject.into(),
        tier: Tier::Unusual,
        title: format!("New program {subject}"),
        why: "Because.".into(),
        evidence: vec![],
        dedupe_key: format!("new_program:{subject}"),
        level: None,
        at: Timestamp::from_micros(10),
    }
}

fn admin() -> FindingsAdmin<InMemoryRepository, ManualClock> {
    let mut repo = InMemoryRepository::default();
    repo.apply_writes(&[FindingWrite::Insert(candidate("a.exe"))]).unwrap();
    FindingsAdmin::new(repo, ManualClock::new(Timestamp::from_micros(NOW)))
}

fn decide(admin: &impl FindingsControl, id: i64, decision: &str, days: Option<u32>, note: Option<&str>) -> Response {
    admin.decide(id, decision, days, note, None)
}

fn error_text(response: Response) -> String {
    match response {
        Response::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn the_finding(admin: &FindingsAdmin<InMemoryRepository, ManualClock>) -> syntrac_core::findings::Finding {
    admin.with_repository(|repo| repo.finding(1).unwrap().unwrap())
}

#[test]
fn expected_this_time_marks_the_finding_with_the_time_and_creates_no_expectation() {
    let admin = admin();

    assert_eq!(decide(&admin, 1, "expected_this_time", None, None), Response::Done);

    let found = the_finding(&admin);
    assert_eq!((found.status, found.decided_at), (FindingStatus::Expected, Some(Timestamp::from_micros(NOW))));
    assert!(admin.with_repository(|repo| repo.expectations().unwrap()).is_empty());
}

#[test]
fn always_expected_marks_the_finding_and_stores_an_expectation_for_exactly_its_kind_and_subject() {
    let admin = admin();

    assert_eq!(decide(&admin, 1, "always_expected", None, Some("my backup")), Response::Done);

    assert_eq!(the_finding(&admin).status, FindingStatus::Expected);
    let expectations = admin.with_repository(|repo| repo.expectations().unwrap());
    assert_eq!(expectations.len(), 1);
    assert_eq!((expectations[0].kind.as_str(), expectations[0].subject.as_str()), ("new_program", "a.exe"));
    assert_eq!((expectations[0].note.as_deref(), expectations[0].source_finding), (Some("my backup"), Some(1)));
    assert_eq!(expectations[0].created_at, Timestamp::from_micros(NOW));
}

#[test]
fn not_expected_and_reopen_change_only_the_status() {
    let admin = admin();

    decide(&admin, 1, "not_expected", None, None);
    assert_eq!(the_finding(&admin).status, FindingStatus::NotExpected);
    decide(&admin, 1, "reopen", None, None);
    assert_eq!(the_finding(&admin).status, FindingStatus::Open);
}

#[test]
fn a_snooze_stores_when_it_ends_and_must_be_between_one_day_and_a_year() {
    let admin = admin();

    assert_eq!(decide(&admin, 1, "snooze", Some(7), None), Response::Done);
    let found = the_finding(&admin);
    assert_eq!(found.status, FindingStatus::Snoozed);
    assert_eq!(found.snoozed_until, Some(Timestamp::from_micros(NOW + 7 * 86_400 * 1_000_000)));

    assert!(error_text(decide(&admin, 1, "snooze", Some(0), None)).contains("between 1 and 365"));
    assert!(error_text(decide(&admin, 1, "snooze", None, None)).contains("how many days"));
}

#[test]
fn an_unknown_decision_an_unknown_finding_and_a_long_note_are_refused_in_plain_words() {
    let admin = admin();

    assert!(error_text(decide(&admin, 1, "delete_everything", None, None)).contains("Unknown decision"));
    assert!(error_text(decide(&admin, 99, "reopen", None, None)).contains("no such finding"));
    assert!(error_text(decide(&admin, 1, "always_expected", None, Some(&"x".repeat(500)))).contains("at most"));
    assert_eq!(the_finding(&admin).status, FindingStatus::Open, "a refused decision changes nothing");
    assert!(admin.with_repository(|repo| repo.expectations().unwrap()).is_empty());
}

#[test]
fn an_expectation_can_be_removed_once() {
    let admin = admin();
    decide(&admin, 1, "always_expected", None, None);
    let id = admin.with_repository(|repo| repo.expectations().unwrap())[0].id;

    assert_eq!(admin.remove_expectation(id), Response::Done);
    assert!(error_text(admin.remove_expectation(id)).contains("no such expectation"));
    assert!(admin.with_repository(|repo| repo.expectations().unwrap()).is_empty());
}

struct StatusStub;
impl Handler for StatusStub {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "stub".into() },
            other => Response::Error { message: format!("status stub: {other:?}") },
        }
    }
}

fn service(findings: Option<Arc<dyn FindingsControl>>) -> impl Handler {
    let known = KnownProductsAdmin::new(InMemoryRepository::default(), ManualClock::new(Timestamp::from_micros(NOW)));
    let handler = ServiceHandler::new(StatusStub, known);
    match findings {
        Some(findings) => handler.with_findings(findings),
        None => handler,
    }
}

#[test]
fn the_pipe_handler_carries_findings_requests_to_the_admin_and_keeps_the_old_commands() {
    let handler = service(Some(Arc::new(admin())));

    let decided = handler.handle(Request::FindingDecide {
        id: 1,
        decision: "not_expected".into(),
        days: None,
        note: None,
        scope: None,
    });

    assert_eq!(decided, Response::Done);
    assert!(matches!(handler.handle(Request::Ping), Response::Pong { .. }));
}

#[test]
fn without_findings_the_requests_say_they_are_not_available() {
    let handler = service(None);

    let response = handler.handle(Request::ExpectationRemove { id: 1 });

    assert!(error_text(response).contains("not available"));
}

fn program_admin(path: &str) -> FindingsAdmin<InMemoryRepository, ManualClock> {
    let mut repo = InMemoryRepository::default();
    repo.apply_writes(&[FindingWrite::Insert(candidate(path))]).unwrap();
    FindingsAdmin::new(repo, ManualClock::new(Timestamp::from_micros(NOW)))
}

#[test]
fn always_expected_for_the_folder_stores_the_install_folder_the_service_works_out_itself() {
    let admin = program_admin(r"c:\users\someone\.cargo\bin\cargo.exe");

    let response = admin.decide(1, "always_expected", None, Some("build tools"), Some("this_folder"));

    assert_eq!(response, Response::Done);
    let expectations = admin.with_repository(|repo| repo.expectations().unwrap());
    assert_eq!(expectations[0].subject, r"folder:c:\users\someone\.cargo");
    assert_eq!(expectations[0].kind, "new_program");
}

#[test]
fn the_scope_defaults_to_this_item_and_an_unknown_or_unsuitable_scope_is_refused() {
    let admin = program_admin(r"c:\users\someone\.cargo\bin\cargo.exe");
    assert!(
        error_text(admin.decide(1, "always_expected", None, None, Some("the_whole_disk"))).contains("Unknown scope")
    );
    assert!(admin.with_repository(|repo| repo.expectations().unwrap()).is_empty());

    assert_eq!(admin.decide(1, "always_expected", None, None, None), Response::Done);
    assert_eq!(
        admin.with_repository(|repo| repo.expectations().unwrap())[0].subject,
        r"c:\users\someone\.cargo\bin\cargo.exe"
    );

    // A finding with no install folder cannot be expected as a folder.
    let drive = program_admin(r"c:\tool.exe");
    assert!(
        error_text(drive.decide(1, "always_expected", None, None, Some("this_folder"))).contains("no install folder")
    );
}

fn usage_admin(level: Option<f64>) -> FindingsAdmin<InMemoryRepository, ManualClock> {
    let mut repo = InMemoryRepository::default();
    let mut usage = candidate("p.exe|cpu");
    usage.kind = "usage".into();
    usage.dedupe_key = "usage:p.exe|cpu:1".into();
    usage.level = level;
    repo.apply_writes(&[FindingWrite::Insert(usage)]).unwrap();
    FindingsAdmin::new(repo, ManualClock::new(Timestamp::from_micros(NOW)))
}

#[test]
fn raising_the_limit_stores_what_the_finding_measured_as_the_ceiling_and_marks_it_expected() {
    let admin = usage_admin(Some(61.5));

    assert_eq!(decide(&admin, 1, "raise_limit", None, None), Response::Done);

    assert_eq!(the_finding(&admin).status, FindingStatus::Expected);
    let expectations = admin.with_repository(|repo| repo.expectations().unwrap());
    assert_eq!((expectations[0].kind.as_str(), expectations[0].subject.as_str()), ("usage", "p.exe|cpu"));
    assert_eq!(expectations[0].ceiling, Some(61.5));
}

#[test]
fn the_limit_of_a_finding_that_measured_nothing_cannot_be_raised() {
    let admin = usage_admin(None);

    assert!(error_text(decide(&admin, 1, "raise_limit", None, None)).contains("usage"));
    assert!(admin.with_repository(|repo| repo.expectations().unwrap()).is_empty());
}
