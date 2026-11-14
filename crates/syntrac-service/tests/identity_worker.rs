//! The identity worker: when it runs a pass, how it paces itself, and that it survives a storage error and stops promptly.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_core::{
    FileFacts, FileStamp, IdentityEntry, IdentityRecord, IdentityRepository, IdentitySettings, RepoError, Timestamp,
};
use syntrac_service::{IdentityWorker, IdentityWorkerSettings, WorkerTick};
use syntrac_testkit::identity_contract::seed_executables;
use syntrac_testkit::{FakeInspector, InMemoryRepository, ManualClock};

const A: &str = r"C:\Program Files\Acme\a.exe";
const B: &str = r"C:\Program Files\Acme\b.exe";
const C: &str = r"C:\Program Files\Acme\c.exe";

fn facts() -> FileFacts {
    FileFacts { stamp: FileStamp { size: 10, modified: None }, ..FileFacts::default() }
}

fn settings(per_pass: usize) -> IdentityWorkerSettings {
    IdentityWorkerSettings {
        interval: Duration::from_secs(300),
        backlog_pause: Duration::from_secs(30),
        pass: IdentitySettings { max_per_pass: per_pass, recheck_every: Duration::from_secs(24 * 3600) },
    }
}

fn inspector_for(paths: &[&str]) -> FakeInspector {
    let inspector = FakeInspector::default();
    for path in paths {
        inspector.set(path, Ok(facts()));
    }
    inspector
}

fn advance(clock: &ManualClock, now: Timestamp, seconds: i64) -> Timestamp {
    let later = Timestamp::from_micros(now.as_micros() + seconds * 1_000_000);
    clock.set(later);
    later
}

#[test]
fn the_first_tick_runs_a_pass_and_the_next_waits_for_the_interval() {
    let mut repo = InMemoryRepository::default();
    let now = seed_executables(&mut repo, &[A, B]);
    let clock = ManualClock::new(now);
    let mut worker = IdentityWorker::new(repo, inspector_for(&[A, B]), clock.clone(), settings(10));

    let first = worker.tick();
    let again = worker.tick();

    assert!(matches!(first, WorkerTick::Passed(pass) if pass.inspected == 2 && !pass.backlog), "{first:?}");
    assert_eq!(again, WorkerTick::NotYet);

    let later = advance(&clock, now, 299);
    assert_eq!(worker.tick(), WorkerTick::NotYet, "still inside the interval at {later:?}");
    advance(&clock, now, 301);
    assert!(matches!(worker.tick(), WorkerTick::Passed(_)), "the interval has passed");
}

#[test]
fn a_backlog_is_worked_off_on_the_short_pause_not_the_long_interval() {
    let mut repo = InMemoryRepository::default();
    let now = seed_executables(&mut repo, &[A, B, C]);
    let clock = ManualClock::new(now);
    let mut worker = IdentityWorker::new(repo, inspector_for(&[A, B, C]), clock.clone(), settings(2));

    let first = worker.tick();
    assert!(matches!(first, WorkerTick::Passed(pass) if pass.inspected == 2 && pass.backlog), "{first:?}");

    advance(&clock, now, 29);
    assert_eq!(worker.tick(), WorkerTick::NotYet);
    advance(&clock, now, 31);
    let second = worker.tick();
    assert!(matches!(second, WorkerTick::Passed(pass) if pass.inspected == 1 && !pass.backlog), "{second:?}");

    advance(&clock, now, 200);
    assert_eq!(worker.tick(), WorkerTick::NotYet, "no backlog left: back to the long interval");
}

/// A repository whose first reads fail, to prove one storage error does not end the worker.
struct Flaky {
    inner: InMemoryRepository,
    failures_left: Cell<u32>,
}

impl IdentityRepository for Flaky {
    fn identity_entries(&self) -> Result<Vec<IdentityEntry>, RepoError> {
        if self.failures_left.get() > 0 {
            self.failures_left.set(self.failures_left.get() - 1);
            return Err(RepoError::Backend("disk full".into()));
        }
        self.inner.identity_entries()
    }
    fn save_identity(&mut self, record: &IdentityRecord) -> Result<(), RepoError> {
        self.inner.save_identity(record)
    }
    fn identity(&self, path: &str) -> Result<Option<IdentityRecord>, RepoError> {
        self.inner.identity(path)
    }
}

#[test]
fn a_storage_error_is_reported_and_the_next_pass_still_happens() {
    let mut inner = InMemoryRepository::default();
    let now = seed_executables(&mut inner, &[A]);
    let clock = ManualClock::new(now);
    let repo = Flaky { inner, failures_left: Cell::new(1) };
    let mut worker = IdentityWorker::new(repo, inspector_for(&[A]), clock.clone(), settings(10));

    let first = worker.tick();
    assert!(matches!(first, WorkerTick::StorageError(_)), "{first:?}");

    advance(&clock, now, 301);
    let second = worker.tick();
    assert!(matches!(second, WorkerTick::Passed(pass) if pass.inspected == 1), "{second:?}");
}

#[test]
fn the_loop_ends_promptly_when_asked_to_stop() {
    let mut repo = InMemoryRepository::default();
    let now = seed_executables(&mut repo, &[A]);
    let worker = IdentityWorker::new(repo, inspector_for(&[A]), ManualClock::new(now), settings(10));
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let handle = std::thread::spawn(move || worker.run(&flag, Duration::from_millis(5)));

    std::thread::sleep(Duration::from_millis(50));
    let asked = Instant::now();
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();

    assert!(asked.elapsed() < Duration::from_secs(2), "the worker took {:?} to stop", asked.elapsed());
}
