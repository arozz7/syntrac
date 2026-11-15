//! One pass of the context worker with a fake source and the in-memory repository: what it writes, what it leaves alone, and how it
//! fails.

use std::time::Duration;

use syntrac_core::{
    observe_context, ContextFailure, ContextRepository, ContextSettings, ContextSnapshot, ContextSourceError,
    ContextTracker, ProcessContext, ServiceInfo, Timestamp,
};
use syntrac_testkit::{FakeContextSource, InMemoryRepository};

const SYSTEM: &str = "S-1-5-18";
const USER: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";
const A: &str = r"C:\Apps\A.exe";
const B: &str = r"C:\Apps\B.exe";

fn running(path: &str, sid: &str, services: &[&str]) -> ProcessContext {
    ProcessContext {
        path: path.to_owned(),
        sid: sid.to_owned(),
        account_name: None,
        services: services.iter().map(|s| ServiceInfo { name: (*s).to_owned(), display_name: None }).collect(),
    }
}

fn snapshot(processes: Vec<ProcessContext>) -> ContextSnapshot {
    ContextSnapshot { processes, unreadable: 0 }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_micros(seconds * 1_000_000)
}

fn settings(max_per_pass: usize) -> ContextSettings {
    ContextSettings { max_per_pass, refresh_every: Duration::from_secs(600) }
}

fn tracker() -> ContextTracker {
    ContextTracker::new(Duration::from_secs(600))
}

#[test]
fn the_first_pass_stores_every_program_and_says_what_was_new() {
    let source = FakeContextSource::new(snapshot(vec![running(A, USER, &[]), running(B, SYSTEM, &["acmeserv"])]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());

    let pass = observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap();

    assert_eq!((pass.seen, pass.written, pass.new_accounts, pass.new_services), (2, 2, 2, 1));
    assert!(!pass.truncated);
    assert_eq!(repo.all_contexts().unwrap().len(), 2);
}

#[test]
fn a_second_pass_a_moment_later_writes_nothing_because_nothing_changed() {
    let source = FakeContextSource::new(snapshot(vec![running(A, USER, &[])]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());
    observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap();

    let again = observe_context(&source, &mut repo, &mut tracker, at(30), &settings(100)).unwrap();

    assert_eq!((again.seen, again.written), (1, 0));
    assert_eq!(repo.context_of(A).unwrap().accounts[0].last_seen, at(0), "last seen was not rewritten");
}

#[test]
fn after_the_refresh_time_last_seen_moves() {
    let source = FakeContextSource::new(snapshot(vec![running(A, USER, &[])]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());
    observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap();

    let later = observe_context(&source, &mut repo, &mut tracker, at(600), &settings(100)).unwrap();

    assert_eq!((later.written, later.new_accounts), (1, 0));
    assert_eq!(repo.context_of(A).unwrap().accounts[0].last_seen, at(600));
}

#[test]
fn a_new_service_is_written_at_once() {
    let source = FakeContextSource::new(snapshot(vec![running(A, SYSTEM, &["one"])]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());
    observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap();
    source.set(snapshot(vec![running(A, SYSTEM, &["one", "two"])]));

    let pass = observe_context(&source, &mut repo, &mut tracker, at(30), &settings(100)).unwrap();

    assert_eq!((pass.written, pass.new_services), (1, 1));
}

#[test]
fn a_pass_writes_at_most_its_limit_and_the_rest_follow_at_the_next_pass() {
    let source = FakeContextSource::new(snapshot(vec![
        running(r"C:\Apps\1.exe", USER, &[]),
        running(r"C:\Apps\2.exe", USER, &[]),
        running(r"C:\Apps\3.exe", USER, &[]),
        running(r"C:\Apps\4.exe", USER, &[]),
        running(r"C:\Apps\5.exe", USER, &[]),
    ]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());

    let first = observe_context(&source, &mut repo, &mut tracker, at(0), &settings(2)).unwrap();
    let second = observe_context(&source, &mut repo, &mut tracker, at(30), &settings(2)).unwrap();
    let third = observe_context(&source, &mut repo, &mut tracker, at(60), &settings(2)).unwrap();
    let fourth = observe_context(&source, &mut repo, &mut tracker, at(90), &settings(2)).unwrap();

    assert_eq!((first.written, first.truncated), (2, true));
    assert_eq!((second.written, second.truncated), (2, true));
    assert_eq!((third.written, third.truncated), (1, false));
    assert_eq!(fourth.written, 0);
    assert_eq!(repo.all_contexts().unwrap().len(), 5);
}

#[test]
fn processes_with_no_path_or_that_could_not_be_opened_are_counted_not_stored() {
    let source = FakeContextSource::new(ContextSnapshot {
        processes: vec![running("", USER, &[]), running(A, USER, &[])],
        unreadable: 7,
    });
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());

    let pass = observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap();

    assert_eq!((pass.seen, pass.written, pass.skipped, pass.unreadable), (1, 1, 1, 7));
}

#[test]
fn a_source_that_fails_stores_nothing_and_is_reported() {
    let source = FakeContextSource::failing("the process list could not be read");
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());

    let failure = observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap_err();

    assert!(matches!(failure, ContextFailure::Source(ContextSourceError(ref text)) if text.contains("process list")));
    assert!(repo.all_contexts().unwrap().is_empty());
}

#[test]
fn after_a_failed_pass_everything_is_still_due() {
    let source = FakeContextSource::new(snapshot(vec![running(A, USER, &[])]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());
    source.fail_next("one bad pass");
    assert!(observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).is_err());

    let pass = observe_context(&source, &mut repo, &mut tracker, at(30), &settings(100)).unwrap();

    assert_eq!(pass.written, 1);
}

#[test]
fn a_program_that_stopped_running_and_comes_back_is_written_again() {
    let source = FakeContextSource::new(snapshot(vec![running(A, USER, &[])]));
    let (mut repo, mut tracker) = (InMemoryRepository::default(), tracker());
    observe_context(&source, &mut repo, &mut tracker, at(0), &settings(100)).unwrap();
    source.set(snapshot(vec![]));
    observe_context(&source, &mut repo, &mut tracker, at(30), &settings(100)).unwrap();
    assert!(tracker.is_empty(), "the tracker let go of a program that is not running");

    source.set(snapshot(vec![running(A, USER, &[])]));
    let back = observe_context(&source, &mut repo, &mut tracker, at(60), &settings(100)).unwrap();

    assert_eq!((back.written, back.new_accounts), (1, 0), "written again, but it is not a new account");
    assert_eq!(repo.context_of(A).unwrap().accounts[0].last_seen, at(60));
}
