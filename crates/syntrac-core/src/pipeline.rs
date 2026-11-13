//! Ingest steps: drain a source, correlate in time order, aggregate network flows, persist.

use std::time::Duration;

use crate::correlator::correlate;
use crate::event::{EventEnvelope, EventPayload};
use crate::flow::FlowAggregator;
use crate::identity::ProcessInstanceId;
use crate::metrics::MetricsSample;
use crate::ports::{
    EventRepository, MetricsRepository, MetricsSource, ProcessRepository, RepoError, SourceError, TelemetrySource,
};
use crate::time::Timestamp;

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Holds the only state the ingest path needs: the open network-flow windows.
#[derive(Debug)]
pub struct Pipeline {
    flows: FlowAggregator,
    last_closed_stale: u64,
}

impl Pipeline {
    pub fn new(flow_window: Duration) -> Self {
        Self { flows: FlowAggregator::new(flow_window), last_closed_stale: 0 }
    }

    /// Drains `source`, correlates, and stores. Raw network activity is folded into flows, which are
    /// stored later by [`Pipeline::flush_due`] / [`Pipeline::flush_all`]. Returns how many events were stored now.
    pub fn ingest<S, R>(&mut self, source: &mut S, repo: &mut R) -> Result<usize, IngestError>
    where
        S: TelemetrySource + ?Sized,
        R: ProcessRepository + EventRepository + ?Sized,
    {
        let events = source.drain()?;
        Ok(self.ingest_events(events, repo)?)
    }

    /// Correlates and stores a batch that may have been gathered from SEVERAL sources. The whole batch is
    /// ordered by timestamp first, so which source delivered an event never changes how it is attributed
    /// (for example a snapshot's process list sorts before the traffic of those processes).
    pub fn ingest_events<R>(&mut self, mut events: Vec<EventEnvelope>, repo: &mut R) -> Result<usize, RepoError>
    where
        R: ProcessRepository + EventRepository + ?Sized,
    {
        events.sort_by_key(|event| event.ts);
        self.last_closed_stale = 0;

        // A batch containing ProcessObserved events is a COMPLETE startup snapshot of running processes (only
        // the snapshot source emits them, all in one batch). Remember who is alive and when it was taken.
        let snapshot_alive: Vec<ProcessInstanceId> = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ProcessObserved { pid, created, .. } => {
                    Some(ProcessInstanceId::from_pid_and_creation(*pid, *created))
                }
                _ => None,
            })
            .collect();
        let snapshot_began = events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::ProcessObserved { .. }))
            .map(|event| event.ts)
            .min();

        let mut to_store = Vec::with_capacity(events.len());
        for event in events {
            let correlated = correlate(repo, event)?;
            if matches!(correlated.payload, EventPayload::NetworkActivity { .. }) {
                self.flows.add(&correlated);
            } else {
                to_store.push(correlated);
            }
        }

        repo.append(&to_store)?;

        // Anything recorded as running that the snapshot did not see exited while we were down.
        if let Some(began) = snapshot_began {
            self.last_closed_stale = repo.reconcile_running(&snapshot_alive, began)?;
        }
        Ok(to_store.len())
    }

    /// How many stale "running" processes the most recent startup snapshot closed (0 when none, or no snapshot).
    pub fn last_closed_stale(&self) -> u64 {
        self.last_closed_stale
    }

    /// Stores every flow window that has ended by `now`.
    pub fn flush_due<R: EventRepository + ?Sized>(&mut self, now: Timestamp, repo: &mut R) -> Result<usize, RepoError> {
        let flows = self.flows.flush_due(now);
        repo.append(&flows)?;
        Ok(flows.len())
    }

    /// Stores every open flow window. Call at shutdown.
    pub fn flush_all<R: EventRepository + ?Sized>(&mut self, repo: &mut R) -> Result<usize, RepoError> {
        let flows = self.flows.flush_all();
        repo.append(&flows)?;
        Ok(flows.len())
    }
}

/// Takes one metrics sample, resolves each process to its instance, and stores it.
pub fn ingest_metrics<S, R>(source: &mut S, now: Timestamp, repo: &mut R) -> Result<(), IngestError>
where
    S: MetricsSource + ?Sized,
    R: ProcessRepository + MetricsRepository + ?Sized,
{
    let mut sample: MetricsSample = source.sample(now)?;
    for process in &mut sample.processes {
        process.instance = repo.resolve(process.pid, sample.ts)?;
    }
    repo.append_metrics(&sample)?;
    Ok(())
}
