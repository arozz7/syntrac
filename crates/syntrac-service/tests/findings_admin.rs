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
        at: Timestamp::from_micros(10),
    }
}

fn admin() -> FindingsAdmin<InMemoryRepository, ManualClock> {
    let mut repo = InMemoryRepository::default();
    repo.apply_writes(&[FindingWrite::Insert(candidate("a.exe"))]).unwrap();
    FindingsAdmin::new(repo, ManualClock::new(Timestamp::from_micros(NOW)))
}

fn decide(admin: &impl FindingsControl, id: i64, decision: &str, days: Option<u32>, note: Option<&str>) -> Response {
    admin.decide(id, decision, days, note)
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

    let decided =
        handler.handle(Request::FindingDecide { id: 1, decision: "not_expected".into(), days: None, note: None });

    assert_eq!(decided, Response::Done);
    assert!(matches!(handler.handle(Request::Ping), Response::Pong { .. }));
}

#[test]
fn without_findings_the_requests_say_they_are_not_available() {
    let handler = service(None);

    let response = handler.handle(Request::ExpectationRemove { id: 1 });

    assert!(error_text(response).contains("not available"));
}
