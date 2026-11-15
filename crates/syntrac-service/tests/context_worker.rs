//! The context worker: when it runs a pass, how it paces itself, and that it survives a failing source or repository and stops
//! promptly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_core::{
    ContextObservation, ContextPass, ContextRepository, ContextSettings, ContextSnapshot, ContextWrite,
    ExecutableContext, ProcessContext, RepoError, Timestamp,
};
use syntrac_service::{ContextTick, ContextWorker, ContextWorkerSettings};
use syntrac_testkit::{FakeContextSource, InMemoryRepository, ManualClock};

const A: &str = r"C:\Apps\A.exe";
const USER: &str = "S-1-5-21-1004336348-1177238915-682003330-1001";

fn running(path: &str) -> ProcessContext {
    ProcessContext { path: path.to_owned(), sid: USER.to_owned(), account_name: None, services: Vec::new() }
}

fn source_with(paths: &[&str]) -> FakeContextSource {
    FakeContextSource::new(ContextSnapshot { processes: paths.iter().map(|p| running(p)).collect(), unreadable: 0 })
}

fn settings() -> ContextWorkerSettings {
    ContextWorkerSettings {
        interval: Duration::from_secs(30),
        pass: ContextSettings { max_per_pass: 100, refresh_every: Duration::from_secs(600) },
    }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_micros(seconds * 1_000_000)
}

fn advance(clock: &ManualClock, seconds: i64) {
    clock.set(at(seconds));
}

fn written(tick: &ContextTick) -> Option<usize> {
    match tick {
        ContextTick::Passed(ContextPass { written, .. }) => Some(*written),
        _ => None,
    }
}

#[test]
fn the_first_tick_runs_a_pass_and_the_next_waits_for_the_interval() {
    let clock = ManualClock::new(at(1_000));
    let mut worker = ContextWorker::new(InMemoryRepository::default(), source_with(&[A]), clock.clone(), settings());

    let first = worker.tick();
    let again = worker.tick();

    assert_eq!(written(&first), Some(1), "{first:?}");
    assert_eq!(again, ContextTick::NotYet);

    advance(&clock, 1_029);
    assert_eq!(worker.tick(), ContextTick::NotYet, "still inside the 30 seconds");
    advance(&clock, 1_031);
    let next = worker.tick();
    assert_eq!(written(&next), Some(0), "the interval has passed; nothing changed so nothing is written: {next:?}");
}

#[test]
fn a_program_that_keeps_running_is_written_again_only_after_the_refresh_time() {
    let clock = ManualClock::new(at(0));
    let mut worker = ContextWorker::new(InMemoryRepository::default(), source_with(&[A]), clock.clone(), settings());
    assert_eq!(written(&worker.tick()), Some(1));

    advance(&clock, 31);
    assert_eq!(written(&worker.tick()), Some(0));
    advance(&clock, 601);
    assert_eq!(written(&worker.tick()), Some(1), "ten minutes: last seen is moved");
}

#[test]
fn a_failing_source_is_reported_and_the_next_pass_still_happens() {
    let clock = ManualClock::new(at(0));
    let source = source_with(&[A]);
    source.fail_next("the process list could not be read");
    let mut worker = ContextWorker::new(InMemoryRepository::default(), source, clock.clone(), settings());

    let first = worker.tick();
    assert!(matches!(&first, ContextTick::Failed(message) if message.contains("process list")), "{first:?}");

    advance(&clock, 31);
    assert_eq!(written(&worker.tick()), Some(1), "a bad pass does not end the worker");
}

/// A repository whose first writes fail, to prove one storage error does not end the worker.
struct Flaky {
    inner: InMemoryRepository,
    failures_left: u32,
}

impl ContextRepository for Flaky {
    fn record_context(
        &mut self,
        observations: &[ContextObservation],
        at: Timestamp,
    ) -> Result<ContextWrite, RepoError> {
        if self.failures_left > 0 {
            self.failures_left -= 1;
            return Err(RepoError::Backend("disk full".into()));
        }
        self.inner.record_context(observations, at)
    }
    fn context_of(&self, path: &str) -> Result<ExecutableContext, RepoError> {
        self.inner.context_of(path)
    }
    fn all_contexts(&self) -> Result<Vec<(syntrac_core::Executable, ExecutableContext)>, RepoError> {
        self.inner.all_contexts()
    }
}

#[test]
fn a_storage_error_is_reported_and_what_failed_to_write_is_retried() {
    let clock = ManualClock::new(at(0));
    let repo = Flaky { inner: InMemoryRepository::default(), failures_left: 1 };
    let mut worker = ContextWorker::new(repo, source_with(&[A]), clock.clone(), settings());

    let first = worker.tick();
    assert!(matches!(&first, ContextTick::Failed(message) if message.contains("disk full")), "{first:?}");

    advance(&clock, 31);
    assert_eq!(written(&worker.tick()), Some(1), "it was never marked written, so it is written now");
}

#[test]
fn the_loop_ends_promptly_when_asked_to_stop() {
    let worker =
        ContextWorker::new(InMemoryRepository::default(), source_with(&[A]), ManualClock::new(at(0)), settings());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let handle = std::thread::spawn(move || worker.run(&flag, Duration::from_millis(5)));

    std::thread::sleep(Duration::from_millis(50));
    let asked = Instant::now();
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();

    assert!(asked.elapsed() < Duration::from_secs(2), "the worker took {:?} to stop", asked.elapsed());
}
