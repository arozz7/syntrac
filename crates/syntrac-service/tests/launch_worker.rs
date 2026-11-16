//! The launch-record worker: when it sweeps, how it paces itself, and that it survives a failing source or repository and stops
//! promptly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_core::{
    Executable, InstallRecord, InstallRepository, InstallSeen, InstallWrite, LaunchSource, LauncherKind,
    LauncherObservation, LauncherRepository, LauncherSeen, LauncherWrite, RawEntry, RawInstall, RepoError, Timestamp,
};
use syntrac_service::{LaunchTick, LaunchWorker, LaunchWorkerSettings};
use syntrac_testkit::{FakeInstallSource, FakeLaunchSource, InMemoryRepository, ManualClock};

const A: &str = r"C:\Apps\A.exe";

fn entry(location: &str, command: &str) -> RawEntry {
    RawEntry {
        kind: LauncherKind::Task,
        location: location.to_owned(),
        command: command.to_owned(),
        detail: String::new(),
        profile: None,
    }
}

fn tasks(entries: Vec<RawEntry>) -> Vec<Box<dyn LaunchSource + Send>> {
    vec![Box::new(FakeLaunchSource::new("tasks", &[LauncherKind::Task], entries))]
}

fn settings() -> LaunchWorkerSettings {
    LaunchWorkerSettings { interval: Duration::from_secs(600) }
}

fn env(_: &str) -> Option<String> {
    None
}

fn exists(_: &str) -> bool {
    true
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_micros(seconds * 1_000_000)
}

fn recorded(tick: &LaunchTick) -> Option<u32> {
    match tick {
        LaunchTick::Swept(report) => Some(report.recorded),
        _ => None,
    }
}

#[test]
fn the_first_tick_sweeps_and_the_next_waits_for_the_interval() {
    let clock = ManualClock::new(at(1_000));
    let mut worker = LaunchWorker::new(
        InMemoryRepository::default(),
        tasks(vec![entry(r"\One", A)]),
        env,
        exists,
        clock.clone(),
        settings(),
    );

    let first = worker.tick();

    assert_eq!(recorded(&first), Some(1), "{first:?}");
    assert_eq!(worker.tick(), LaunchTick::NotYet);
    clock.set(at(1_599));
    assert_eq!(worker.tick(), LaunchTick::NotYet, "still inside the ten minutes");
    clock.set(at(1_601));
    assert_eq!(recorded(&worker.tick()), Some(1), "swept again");
}

#[test]
fn a_failing_source_is_part_of_the_report_and_the_sweep_goes_on() {
    let sources: Vec<Box<dyn LaunchSource + Send>> = vec![
        Box::new(FakeLaunchSource::failing("services", &[LauncherKind::ServiceAuto], "the service manager is busy")),
        Box::new(FakeLaunchSource::new("tasks", &[LauncherKind::Task], vec![entry(r"\One", A)])),
    ];
    let mut worker =
        LaunchWorker::new(InMemoryRepository::default(), sources, env, exists, ManualClock::new(at(0)), settings());

    let tick = worker.tick();

    let LaunchTick::Swept(report) = tick else { panic!("expected a sweep: {tick:?}") };
    assert_eq!((report.recorded, report.failed.len()), (1, 1));
    assert_eq!(report.failed[0].0, "services");
}

/// A repository whose first write fails, to prove one storage error does not end the worker.
struct Flaky {
    inner: InMemoryRepository,
    failures_left: u32,
}

impl LauncherRepository for Flaky {
    fn record_launchers(
        &mut self,
        rows: &[LauncherObservation],
        completed: &[LauncherKind],
        at: Timestamp,
    ) -> Result<LauncherWrite, RepoError> {
        if self.failures_left > 0 {
            self.failures_left -= 1;
            return Err(RepoError::Backend("disk full".into()));
        }
        self.inner.record_launchers(rows, completed, at)
    }
    fn launchers_of(&self, path: &str) -> Result<Vec<LauncherSeen>, RepoError> {
        self.inner.launchers_of(path)
    }
    fn all_launchers(&self) -> Result<Vec<(Executable, Vec<LauncherSeen>)>, RepoError> {
        self.inner.all_launchers()
    }
}

#[test]
fn a_storage_error_is_reported_and_the_next_sweep_still_happens() {
    let clock = ManualClock::new(at(0));
    let repo = Flaky { inner: InMemoryRepository::default(), failures_left: 1 };
    let mut worker = LaunchWorker::new(repo, tasks(vec![entry(r"\One", A)]), env, exists, clock.clone(), settings());

    let first = worker.tick();
    assert!(matches!(&first, LaunchTick::Failed(message) if message.contains("disk full")), "{first:?}");

    clock.set(at(601));
    assert_eq!(recorded(&worker.tick()), Some(1), "a bad sweep does not end the worker");
}

#[test]
fn the_loop_ends_promptly_when_asked_to_stop() {
    let worker = LaunchWorker::new(
        InMemoryRepository::default(),
        tasks(vec![]),
        env,
        exists,
        ManualClock::new(at(0)),
        settings(),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let handle = std::thread::spawn(move || worker.run(&flag, Duration::from_millis(5)));

    std::thread::sleep(Duration::from_millis(50));
    let asked = Instant::now();
    stop.store(true, Ordering::SeqCst);
    handle.join().unwrap();

    assert!(asked.elapsed() < Duration::from_secs(2), "the worker took {:?} to stop", asked.elapsed());
}

impl InstallRepository for Flaky {
    fn record_installs(&mut self, rows: &[InstallRecord], at: Timestamp) -> Result<InstallWrite, RepoError> {
        self.inner.record_installs(rows, at)
    }
    fn installs(&self) -> Result<Vec<InstallSeen>, RepoError> {
        self.inner.installs()
    }
}

fn product(name: &str) -> RawInstall {
    RawInstall {
        hive: "machine".to_owned(),
        key_name: format!("{{{name}}}"),
        display_name: name.to_owned(),
        install_location: format!(r"C:\Apps\{name}"),
        ..RawInstall::default()
    }
}

#[test]
fn the_uninstall_keys_are_read_in_the_same_sweep_when_a_source_is_given() {
    let mut worker = LaunchWorker::new(
        InMemoryRepository::default(),
        tasks(vec![entry(r"\One", A)]),
        env,
        exists,
        ManualClock::new(at(0)),
        settings(),
    )
    .with_installs(Box::new(FakeInstallSource::new(vec![product("Alpha"), product("Beta")])));

    let tick = worker.tick();

    let LaunchTick::Swept(report) = tick else { panic!("expected a sweep: {tick:?}") };
    assert_eq!(report.recorded, 1);
    let installs = report.installs.expect("install records were read");
    assert_eq!((installs.recorded, installs.new), (2, 2));
}

#[test]
fn a_failing_uninstall_read_is_reported_and_the_launch_records_are_still_kept() {
    let mut worker = LaunchWorker::new(
        InMemoryRepository::default(),
        tasks(vec![entry(r"\One", A)]),
        env,
        exists,
        ManualClock::new(at(0)),
        settings(),
    )
    .with_installs(Box::new(FakeInstallSource::failing("the registry is busy")));

    let tick = worker.tick();

    let LaunchTick::Swept(report) = tick else { panic!("expected a sweep: {tick:?}") };
    assert_eq!(report.recorded, 1);
    assert!(report.installs.is_none());
    assert_eq!(report.failed, [("install records".to_owned(), "the registry is busy".to_owned())]);
}
