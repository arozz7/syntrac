//! The collector run loop: drain sources, correlate and store, flush finished flow windows,
//! sample metrics and purge old data on their schedules.
//!
//! Everything it touches is injected (sources, repository, clock), so it is proven offline in
//! `tests/collector_loop.rs`. Nothing here may panic or stop on a single failure: a broken source,
//! a failed sample or a storage error is counted and logged, and the loop carries on.

use std::time::Duration;

use syntrac_core::retention::{purge_cutoff, purge_step, GuardLevel, GuardLimits};
use syntrac_core::storage_guard::{FreeSpace, StorageStats};
use syntrac_core::EventCategory;
use syntrac_core::{
    backfill_names, ingest_metrics, roll_up_due, Clock, EventRepository, IngestError, MetricsRepository, MetricsSource,
    Pipeline, ProcessRepository, PurgeCounts, Retention, RollupRepository, RollupSettings, SourceError,
    TelemetrySource, Timestamp,
};
use tracing::{error, info, warn};

use crate::guard::{GuardRuntime, StorageSnapshot};

/// Between two rollup passes while more hours are already due: long enough for the loop to keep draining ETW in between.
const BACKLOG_PAUSE: Duration = Duration::from_secs(1);

/// One purge pass removes at most this much detail (about a second of work), so a long backlog never holds up the loop.
const PURGE_SPAN: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy)]
pub struct CollectorSettings {
    pub flow_window: Duration,
    /// A flow window is stored only this long AFTER it ends, so events that arrive late (ETW delivers in
    /// buffered batches) still join it instead of creating a duplicate row.
    pub flow_flush_grace: Duration,
    pub metrics_interval: Duration,
    pub retention: Duration,
    pub purge_interval: Duration,
    /// How often completed hours are rolled up into the permanent summaries.
    pub rollup_interval: Duration,
    pub rollup: RollupSettings,
    /// How often the storage guard measures the database and the drive (when a guard is installed).
    pub guard_interval: Duration,
}

/// What one tick did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    pub events_stored: usize,
    pub flows_stored: usize,
    pub metrics_sampled: bool,
    pub metrics_errors: usize,
    pub source_errors: usize,
    pub storage_errors: usize,
    pub purged: Option<PurgeCounts>,
    /// Hours rolled up by this tick.
    pub rollup_hours: usize,
    /// The storage guard's level after this tick (`Ok` when no guard is installed).
    pub guard_level: GuardLevel,
    /// One-hour steps of detail the guard trimmed early in this tick.
    pub guard_trimmed_steps: u32,
    /// Events thrown away because collection is paused (process events are kept).
    pub events_discarded: usize,
}

/// Running totals since start.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub ticks: u64,
    pub events_stored: u64,
    pub flows_stored: u64,
    pub metrics_samples: u64,
    pub source_errors: u64,
    pub metrics_errors: u64,
    pub storage_errors: u64,
}

pub struct Collector<R, C> {
    settings: CollectorSettings,
    pipeline: Pipeline,
    sources: Vec<Box<dyn TelemetrySource>>,
    metrics: Option<Box<dyn MetricsSource>>,
    repo: R,
    clock: C,
    next_metrics_at: Timestamp,
    next_purge_at: Timestamp,
    next_rollup_at: Timestamp,
    guard: Option<GuardRuntime>,
    totals: Totals,
}

fn after(base: Timestamp, interval: Duration) -> Timestamp {
    let micros = i64::try_from(interval.as_micros()).unwrap_or(i64::MAX);
    Timestamp::from_micros(base.as_micros().saturating_add(micros))
}

fn before(base: Timestamp, span: Duration) -> Timestamp {
    let micros = i64::try_from(span.as_micros()).unwrap_or(i64::MAX);
    Timestamp::from_micros(base.as_micros().saturating_sub(micros))
}

impl<R, C> Collector<R, C>
where
    R: ProcessRepository + EventRepository + MetricsRepository + Retention + RollupRepository + StorageStats,
    C: Clock,
{
    pub fn new(
        settings: CollectorSettings,
        sources: Vec<Box<dyn TelemetrySource>>,
        metrics: Option<Box<dyn MetricsSource>>,
        repo: R,
        clock: C,
    ) -> Self {
        let now = clock.now();
        Self {
            pipeline: Pipeline::new(settings.flow_window),
            settings,
            sources,
            metrics,
            repo,
            clock,
            // CPU usage is a delta between two readings, so a sample taken right at start would be
            // meaningless (all zeros). The first sample waits one interval.
            next_metrics_at: after(now, settings.metrics_interval),
            next_purge_at: now,
            next_rollup_at: now,
            guard: None,
            totals: Totals::default(),
        }
    }

    /// Installs the storage guard: from now on the collector measures the database and the drive on its interval, trims the
    /// oldest rolled-up detail early when either is too full, and pauses detail collection below the hard floor.
    pub fn with_storage_guard(mut self, limits: GuardLimits, volume: Box<dyn FreeSpace>) -> Self {
        self.guard = Some(GuardRuntime::new(limits, volume, self.settings.guard_interval, self.clock.now()));
        self
    }

    /// What the last guard check found. `None` without a guard, or before its first check.
    pub fn storage(&self) -> Option<StorageSnapshot> {
        self.guard.as_ref().and_then(GuardRuntime::snapshot)
    }

    fn is_paused(&self) -> bool {
        self.guard.as_ref().is_some_and(|guard| guard.level() == GuardLevel::Paused)
    }

    pub fn repository(&self) -> &R {
        &self.repo
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Restarts the source with this name, for example an ETW trace that has gone silent.
    pub fn restart_source(&mut self, name: &str) -> Result<(), SourceError> {
        match self.sources.iter_mut().find(|source| source.name() == name) {
            Some(source) => source.restart(),
            None => Err(SourceError::Failed(format!("there is no source named {name}"))),
        }
    }

    /// One pass of the loop. Call it every few hundred milliseconds.
    pub fn tick(&mut self) -> TickReport {
        let now = self.clock.now();
        let mut report = TickReport::default();

        // The guard first, so a pause takes effect before this tick drains anything.
        if let Some(guard) = self.guard.as_mut() {
            report.guard_trimmed_steps = guard.check_if_due(&mut self.repo, now);
        }
        report.guard_level = self.guard.as_ref().map_or(GuardLevel::Ok, GuardRuntime::level);

        self.drain_sources(&mut report);
        self.flush_finished_flows(now, &mut report);
        self.sample_metrics_if_due(now, &mut report);
        self.roll_up_if_due(now, &mut report);
        self.purge_if_due(now, &mut report);

        self.totals.ticks += 1;
        self.add_to_totals(&report);
        report
    }

    /// Final pass: take whatever is still waiting and store every open flow window, so a stop loses nothing.
    pub fn shutdown(&mut self) -> TickReport {
        let mut report = TickReport::default();
        // Stop first so nothing new arrives and buffered events are released, then drain what is left.
        for source in &mut self.sources {
            info!(source = source.name(), "Stopping telemetry source");
            source.stop();
        }
        self.drain_sources(&mut report);
        match self.pipeline.flush_all(&mut self.repo) {
            Ok(flows) => report.flows_stored += flows,
            Err(error) => {
                error!(%error, "Could not store open flow windows at shutdown");
                report.storage_errors += 1;
            }
        }
        self.add_to_totals(&report);
        report
    }

    /// Gathers every source into ONE batch before correlating, so the order the sources are listed in
    /// cannot change attribution. A failing source is counted and skipped; the others still deliver.
    fn drain_sources(&mut self, report: &mut TickReport) {
        let mut batch = Vec::new();
        for source in &mut self.sources {
            match source.drain() {
                Ok(mut events) => batch.append(&mut events),
                Err(error) => {
                    warn!(source = source.name(), %error, "Telemetry source failed");
                    report.source_errors += 1;
                }
            }
        }

        if self.is_paused() {
            // The disk is nearly full: keep only process starts and exits (small, and they keep traffic after the pause
            // attributed to the right program); drop everything else.
            let total = batch.len();
            batch.retain(|event| event.payload.category() == EventCategory::Process);
            report.events_discarded += total - batch.len();
        }

        match self.pipeline.ingest_events(batch, &mut self.repo) {
            Ok(stored) => {
                report.events_stored += stored;
                let closed = self.pipeline.last_closed_stale();
                if closed > 0 {
                    info!(closed, "Closed processes that exited while the collector was not running");
                }
            }
            Err(error) => {
                error!(%error, "Could not store telemetry");
                report.storage_errors += 1;
            }
        }
    }

    fn flush_finished_flows(&mut self, now: Timestamp, report: &mut TickReport) {
        let settled_before = before(now, self.settings.flow_flush_grace);
        match self.pipeline.flush_due(settled_before, &mut self.repo) {
            Ok(flows) => report.flows_stored += flows,
            Err(error) => {
                error!(%error, "Could not store finished flow windows");
                report.storage_errors += 1;
            }
        }
    }

    fn sample_metrics_if_due(&mut self, now: Timestamp, report: &mut TickReport) {
        let paused = self.is_paused();
        let Some(source) = self.metrics.as_mut() else { return };
        if now < self.next_metrics_at {
            return;
        }
        // Schedule first, so a failing sampler cannot turn into a hot loop.
        self.next_metrics_at = after(now, self.settings.metrics_interval);
        if paused {
            return; // no samples while detail collection is paused
        }

        match ingest_metrics(source.as_mut(), now, &mut self.repo) {
            Ok(()) => report.metrics_sampled = true,
            Err(IngestError::Source(error)) => {
                warn!(source = source.name(), %error, "Metrics sample failed");
                report.metrics_errors += 1;
            }
            Err(IngestError::Repo(error)) => {
                error!(%error, "Could not store a metrics sample");
                report.storage_errors += 1;
            }
        }
    }

    /// Rolls completed hours up into the permanent summaries. Done BEFORE the purge, so detail is summarised before it can
    /// age out. A pass does a few hours at most; while more are waiting the next pass follows almost at once, otherwise it
    /// waits for the usual interval. A failure is counted and retried at the usual interval (never a hot loop).
    fn roll_up_if_due(&mut self, now: Timestamp, report: &mut TickReport) {
        if now < self.next_rollup_at {
            return;
        }
        self.next_rollup_at = after(now, self.settings.rollup_interval);

        // Before any hour is rolled: the names of lookups already in the detail (hours rolled before names were kept cannot be
        // rolled again). Done once; a failure is counted and retried at the next pass.
        match backfill_names(&mut self.repo, now) {
            Ok(done) if done.ran => info!(names = done.names, "Kept the names of the retained DNS lookups"),
            Ok(_) => {}
            Err(error) => {
                error!(%error, "Could not keep the names of the retained DNS lookups");
                report.storage_errors += 1;
                return;
            }
        }

        match roll_up_due(&mut self.repo, now, &self.settings.rollup) {
            Ok(pass) => {
                report.rollup_hours = pass.hours_rolled;
                if pass.hours_rolled > 0 {
                    info!(hours = pass.hours_rolled, backlog = pass.backlog, "Rolled up completed hours");
                }
                if pass.backlog {
                    self.next_rollup_at = after(now, BACKLOG_PAUSE);
                }
            }
            Err(error) => {
                error!(%error, "Could not roll up completed hours");
                report.storage_errors += 1;
            }
        }
    }

    fn purge_if_due(&mut self, now: Timestamp, report: &mut TickReport) {
        if now < self.next_purge_at {
            return;
        }
        self.next_purge_at = after(now, self.settings.purge_interval);

        // Detail goes only when it is older than the retention window AND already summarised. Before the first rollup nothing
        // may be deleted at all.
        let rolled_up = match self.repo.rolled_up_before() {
            Ok(rolled_up) => rolled_up,
            Err(error) => {
                error!(%error, "Could not read the rollup watermark; not purging");
                report.storage_errors += 1;
                return;
            }
        };
        let Some(cutoff) = purge_cutoff(now, self.settings.retention, rolled_up) else {
            return;
        };

        // A long backlog goes in short passes (a second of work each) with a pause between them. If the oldest detail cannot be
        // read, the pass runs to the cutoff in one go, as before.
        let oldest = match StorageStats::usage(&self.repo) {
            Ok(usage) => usage.oldest_detail,
            Err(error) => {
                warn!(%error, "Could not read the oldest detail; purging in one pass");
                None
            }
        };
        let step = purge_step(cutoff, oldest, PURGE_SPAN);
        if step.more_remains {
            self.next_purge_at = after(now, BACKLOG_PAUSE);
        }

        match self.repo.purge_before(step.cutoff) {
            Ok(counts) => {
                info!(
                    events = counts.events,
                    processes = counts.processes,
                    metric_samples = counts.metric_samples,
                    "Old data purged"
                );
                report.purged = Some(counts);
                // Hand the freed pages back to the drive. Not fatal: the next purge tries again.
                if let Err(error) = StorageStats::reclaim_space(&mut self.repo) {
                    warn!(%error, "Could not return freed database space to the drive");
                }
            }
            Err(error) => {
                error!(%error, "Could not purge old data");
                report.storage_errors += 1;
            }
        }
    }

    fn add_to_totals(&mut self, report: &TickReport) {
        self.totals.events_stored += report.events_stored as u64;
        self.totals.flows_stored += report.flows_stored as u64;
        self.totals.metrics_samples += u64::from(report.metrics_sampled);
        self.totals.source_errors += report.source_errors as u64;
        self.totals.metrics_errors += report.metrics_errors as u64;
        self.totals.storage_errors += report.storage_errors as u64;
    }
}
