//! The collector run loop: drain sources, correlate and store, flush finished flow windows,
//! sample metrics and purge old data on their schedules.
//!
//! Everything it touches is injected (sources, repository, clock), so it is proven offline in
//! `tests/collector_loop.rs`. Nothing here may panic or stop on a single failure: a broken source,
//! a failed sample or a storage error is counted and logged, and the loop carries on.

use std::time::Duration;

use syntrac_core::{
    ingest_metrics, Clock, EventRepository, IngestError, MetricsRepository, MetricsSource, Pipeline, ProcessRepository,
    PurgeCounts, Retention, TelemetrySource, Timestamp,
};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy)]
pub struct CollectorSettings {
    pub flow_window: Duration,
    /// A flow window is stored only this long AFTER it ends, so events that arrive late (ETW delivers in
    /// buffered batches) still join it instead of creating a duplicate row.
    pub flow_flush_grace: Duration,
    pub metrics_interval: Duration,
    pub retention: Duration,
    pub purge_interval: Duration,
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
    R: ProcessRepository + EventRepository + MetricsRepository + Retention,
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
            totals: Totals::default(),
        }
    }

    pub fn repository(&self) -> &R {
        &self.repo
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// One pass of the loop. Call it every few hundred milliseconds.
    pub fn tick(&mut self) -> TickReport {
        let now = self.clock.now();
        let mut report = TickReport::default();

        self.drain_sources(&mut report);
        self.flush_finished_flows(now, &mut report);
        self.sample_metrics_if_due(now, &mut report);
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

        match self.pipeline.ingest_events(batch, &mut self.repo) {
            Ok(stored) => report.events_stored += stored,
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
        let Some(source) = self.metrics.as_mut() else { return };
        if now < self.next_metrics_at {
            return;
        }
        // Schedule first, so a failing sampler cannot turn into a hot loop.
        self.next_metrics_at = after(now, self.settings.metrics_interval);

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

    fn purge_if_due(&mut self, now: Timestamp, report: &mut TickReport) {
        if now < self.next_purge_at {
            return;
        }
        self.next_purge_at = after(now, self.settings.purge_interval);

        match self.repo.purge_before(before(now, self.settings.retention)) {
            Ok(counts) => {
                info!(
                    events = counts.events,
                    processes = counts.processes,
                    metric_samples = counts.metric_samples,
                    "Old data purged"
                );
                report.purged = Some(counts);
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
