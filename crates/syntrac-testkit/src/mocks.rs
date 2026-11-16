//! Mock implementations of the tool-layer interfaces.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;

use syntrac_core::{Clock, EventEnvelope, MetricsSample, MetricsSource, SourceError, TelemetrySource, Timestamp};

/// Replays a fixed list of events once, then reports nothing new.
#[derive(Debug)]
pub struct MockTelemetrySource {
    pending: Vec<EventEnvelope>,
}

impl MockTelemetrySource {
    pub fn new(events: Vec<EventEnvelope>) -> Self {
        Self { pending: events }
    }
}

impl TelemetrySource for MockTelemetrySource {
    fn name(&self) -> &str {
        "mock"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Ok(std::mem::take(&mut self.pending))
    }
}

/// Delivers one prepared batch per drain, then nothing. Lets a test model events that ARRIVE LATE
/// (stamped earlier than the previous batch) the way ETW's buffered delivery does.
#[derive(Debug)]
pub struct ScriptedTelemetrySource {
    batches: VecDeque<Vec<EventEnvelope>>,
}

impl ScriptedTelemetrySource {
    pub fn new(batches: Vec<Vec<EventEnvelope>>) -> Self {
        Self { batches: batches.into() }
    }
}

impl TelemetrySource for ScriptedTelemetrySource {
    fn name(&self) -> &str {
        "scripted"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Ok(self.batches.pop_front().unwrap_or_default())
    }
}

/// A source that always fails, to prove one broken source cannot stop the others.
#[derive(Debug, Default)]
pub struct FailingTelemetrySource;

impl TelemetrySource for FailingTelemetrySource {
    fn name(&self) -> &str {
        "failing"
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Err(SourceError::Failed("simulated failure".into()))
    }
}

/// Hands out prepared samples one per call, then fails (to exercise error handling).
#[derive(Debug)]
pub struct MockMetricsSource {
    samples: VecDeque<MetricsSample>,
}

impl MockMetricsSource {
    pub fn new(samples: Vec<MetricsSample>) -> Self {
        Self { samples: samples.into() }
    }
}

impl MetricsSource for MockMetricsSource {
    fn name(&self) -> &str {
        "mock-metrics"
    }

    fn sample(&mut self, _now: Timestamp) -> Result<MetricsSample, SourceError> {
        self.samples.pop_front().ok_or_else(|| SourceError::Failed("no more prepared samples".into()))
    }
}

/// A clock that only moves when told to. Clones share the same time, so a test can keep one
/// handle while handing another to the code under test.
#[derive(Debug, Clone)]
pub struct ManualClock {
    micros: Arc<AtomicI64>,
}

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        Self { micros: Arc::new(AtomicI64::new(start.as_micros())) }
    }

    pub fn set(&self, now: Timestamp) {
        self.micros.store(now.as_micros(), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_micros(self.micros.load(Ordering::SeqCst))
    }
}

/// How many times a `RestartableTelemetrySource` was asked to restart. Shared, so a test can read it after the
/// source has been handed to the collector.
#[derive(Debug, Clone, Default)]
pub struct RestartCounter(Arc<AtomicU32>);

impl RestartCounter {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A source with a name of the test's choosing that supports `restart`, for exercising the silence watchdog.
#[derive(Debug)]
pub struct RestartableTelemetrySource {
    name: String,
    restarts: RestartCounter,
    fail: bool,
}

impl RestartableTelemetrySource {
    pub fn new(name: &str) -> (Self, RestartCounter) {
        let restarts = RestartCounter::default();
        (Self { name: name.to_owned(), restarts: restarts.clone(), fail: false }, restarts)
    }

    /// Makes every restart fail, after counting it.
    pub fn fail_restarts(&mut self) {
        self.fail = true;
    }
}

impl TelemetrySource for RestartableTelemetrySource {
    fn name(&self) -> &str {
        &self.name
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        Ok(Vec::new())
    }

    fn restart(&mut self) -> Result<(), SourceError> {
        self.restarts.0.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(SourceError::Failed("simulated restart failure".into()))
        } else {
            Ok(())
        }
    }
}

/// A `FileInspector` over scripted files, for tests: it answers from a table and remembers what it was asked, so a test can
/// prove a file was NOT read again. Paths are matched without regard to case, like Windows.
#[derive(Debug, Default)]
pub struct FakeInspector {
    files: std::cell::RefCell<
        std::collections::HashMap<String, Result<syntrac_core::FileFacts, syntrac_core::InspectError>>,
    >,
    stats: std::cell::Cell<usize>,
    inspections: std::cell::RefCell<Vec<String>>,
}

impl FakeInspector {
    /// Sets (or replaces) what this path will answer.
    pub fn set(&self, path: &str, answer: Result<syntrac_core::FileFacts, syntrac_core::InspectError>) {
        self.files.borrow_mut().insert(path.to_lowercase(), answer);
    }

    /// How many cheap size-and-time checks were made.
    pub fn stat_count(&self) -> usize {
        self.stats.get()
    }

    /// The paths that were read in full, in order.
    pub fn full_inspections(&self) -> Vec<String> {
        self.inspections.borrow().clone()
    }

    fn answer(&self, path: &str) -> Result<syntrac_core::FileFacts, syntrac_core::InspectError> {
        self.files.borrow().get(&path.to_lowercase()).cloned().unwrap_or_else(|| {
            Err(syntrac_core::InspectError {
                kind: syntrac_core::InspectErrorKind::NotFound,
                message: "no such file".into(),
            })
        })
    }
}

impl syntrac_core::FileInspector for FakeInspector {
    fn stamp(&self, path: &str) -> Result<syntrac_core::FileStamp, syntrac_core::InspectError> {
        self.stats.set(self.stats.get() + 1);
        self.answer(path).map(|facts| facts.stamp)
    }

    fn inspect(&self, path: &str) -> Result<syntrac_core::FileFacts, syntrac_core::InspectError> {
        self.inspections.borrow_mut().push(path.to_owned());
        self.answer(path)
    }
}

/// A `ProcessContextSource` that answers what it was told: a snapshot, or a failure (always, or just for the next pass).
#[derive(Debug)]
pub struct FakeContextSource {
    answer: std::cell::RefCell<Result<syntrac_core::ContextSnapshot, String>>,
    fail_once: std::cell::RefCell<Option<String>>,
}

impl FakeContextSource {
    pub fn new(snapshot: syntrac_core::ContextSnapshot) -> Self {
        Self { answer: std::cell::RefCell::new(Ok(snapshot)), fail_once: std::cell::RefCell::new(None) }
    }

    /// A source whose every look fails with this message.
    pub fn failing(message: &str) -> Self {
        Self { answer: std::cell::RefCell::new(Err(message.to_owned())), fail_once: std::cell::RefCell::new(None) }
    }

    /// From now on, answers this snapshot.
    pub fn set(&self, snapshot: syntrac_core::ContextSnapshot) {
        *self.answer.borrow_mut() = Ok(snapshot);
    }

    /// The next look fails with this message; the ones after it answer normally again.
    pub fn fail_next(&self, message: &str) {
        *self.fail_once.borrow_mut() = Some(message.to_owned());
    }
}

impl syntrac_core::ProcessContextSource for FakeContextSource {
    fn observe(&self) -> Result<syntrac_core::ContextSnapshot, syntrac_core::ContextSourceError> {
        if let Some(message) = self.fail_once.borrow_mut().take() {
            return Err(syntrac_core::ContextSourceError(message));
        }
        self.answer.borrow().clone().map_err(syntrac_core::ContextSourceError)
    }
}

/// A `DatasetFetcher` that answers what it was told (a file, or a failure) and counts how often it was asked. Safe to share
/// between threads, so the service's background refresh can use it.
#[derive(Debug)]
pub struct FakeDatasetFetcher {
    answer: std::sync::Mutex<Result<syntrac_core::FetchedFile, String>>,
    calls: std::sync::atomic::AtomicUsize,
    /// How long a fetch takes, so a test can look at the "refreshing" state.
    delay: std::time::Duration,
}

impl FakeDatasetFetcher {
    pub fn new(text: &str) -> Self {
        Self {
            answer: std::sync::Mutex::new(Ok(syntrac_core::FetchedFile {
                text: text.to_owned(),
                source: "iptoasn.com (test)".to_owned(),
                sha256: "00".repeat(32),
            })),
            calls: std::sync::atomic::AtomicUsize::new(0),
            delay: std::time::Duration::ZERO,
        }
    }

    pub fn failing(message: &str) -> Self {
        let fetcher = Self::new("");
        *fetcher.answer.lock().unwrap() = Err(message.to_owned());
        fetcher
    }

    pub fn slow(mut self, delay: std::time::Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl syntrac_core::DatasetFetcher for FakeDatasetFetcher {
    fn fetch(&self) -> Result<syntrac_core::FetchedFile, syntrac_core::FetchError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(self.delay);
        self.answer.lock().unwrap().clone().map_err(syntrac_core::FetchError)
    }
}

/// A `LaunchSource` that answers what it was told: the entries it found, or a failure.
#[derive(Debug)]
pub struct FakeLaunchSource {
    name: &'static str,
    kinds: &'static [syntrac_core::LauncherKind],
    answer: Result<syntrac_core::SourceRead, String>,
}

impl FakeLaunchSource {
    pub fn new(
        name: &'static str,
        kinds: &'static [syntrac_core::LauncherKind],
        entries: Vec<syntrac_core::RawEntry>,
    ) -> Self {
        Self::with_read(name, kinds, syntrac_core::SourceRead { entries, unreadable: 0 })
    }

    pub fn with_read(
        name: &'static str,
        kinds: &'static [syntrac_core::LauncherKind],
        read: syntrac_core::SourceRead,
    ) -> Self {
        Self { name, kinds, answer: Ok(read) }
    }

    pub fn failing(name: &'static str, kinds: &'static [syntrac_core::LauncherKind], message: &str) -> Self {
        Self { name, kinds, answer: Err(message.to_owned()) }
    }
}

impl syntrac_core::LaunchSource for FakeLaunchSource {
    fn name(&self) -> &'static str {
        self.name
    }

    fn kinds(&self) -> &'static [syntrac_core::LauncherKind] {
        self.kinds
    }

    fn read(&self) -> Result<syntrac_core::SourceRead, syntrac_core::LaunchSourceError> {
        self.answer.clone().map_err(syntrac_core::LaunchSourceError)
    }
}
