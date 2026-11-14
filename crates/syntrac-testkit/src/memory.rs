//! In-memory repository: lets the whole pipeline run offline with no database.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use syntrac_core::{
    CpuBreakdown, CpuContribution, Destination, EventEnvelope, EventPayload, EventQuery, EventRepository, FlowQueries,
    FlowRow, MetricsRepository, MetricsSample, ProcessFilter, ProcessInstanceId, ProcessQueries, ProcessRecord,
    ProcessRepository, PurgeCounts, RepoError, Retention, SystemPoint, Timestamp,
};

#[derive(Debug, Default)]
pub struct InMemoryRepository {
    pub(crate) processes: HashMap<ProcessInstanceId, ProcessRecord>,
    pub(crate) events: Vec<EventEnvelope>,
    pub(crate) metrics: Vec<MetricsSample>,
    pub(crate) rollups: crate::memory_rollup::RollupState,
    pub(crate) known: Vec<syntrac_core::KnownProduct>,
    /// How many ids have been handed out: an id that was removed is never issued again.
    pub(crate) known_issued: i64,
}

impl ProcessRepository for InMemoryRepository {
    fn record_start(&mut self, mut record: ProcessRecord) -> Result<(), RepoError> {
        if let Some(existing) = self.processes.get(&record.id) {
            // Never clear a recorded exit: a late startup snapshot must not resurrect the process.
            if record.ended.is_none() {
                record.ended = existing.ended;
                record.ended_approximate = existing.ended_approximate;
            }
            record.exit_code = record.exit_code.or(existing.exit_code);
            record.parent_id = record.parent_id.or(existing.parent_id);
            record.command_line = record.command_line.or_else(|| existing.command_line.clone());
        }
        self.processes.insert(record.id, record);
        Ok(())
    }

    fn record_exit(
        &mut self,
        id: ProcessInstanceId,
        ended: Timestamp,
        exit_code: Option<u32>,
    ) -> Result<(), RepoError> {
        let record = self.processes.get_mut(&id).ok_or(RepoError::NotFound)?;
        record.ended = Some(ended);
        record.ended_approximate = false;
        record.exit_code = exit_code;
        Ok(())
    }

    fn reconcile_running(&mut self, alive: &[ProcessInstanceId], observed_at: Timestamp) -> Result<u64, RepoError> {
        let mut closed = 0;
        for record in self.processes.values_mut() {
            if record.ended.is_none() && record.started <= observed_at && !alive.contains(&record.id) {
                record.ended = Some(observed_at);
                record.ended_approximate = true;
                closed += 1;
            }
        }
        Ok(closed)
    }

    fn get(&self, id: ProcessInstanceId) -> Result<Option<ProcessRecord>, RepoError> {
        Ok(self.processes.get(&id).cloned())
    }

    fn resolve(&self, pid: u32, at: Timestamp) -> Result<Option<ProcessInstanceId>, RepoError> {
        let owner = self
            .processes
            .values()
            .filter(|p| p.pid == pid && p.started <= at && p.ended.is_none_or(|end| at <= end))
            .max_by_key(|p| p.started)
            .map(|p| p.id);
        Ok(owner)
    }
}

impl ProcessQueries for InMemoryRepository {
    fn list_processes(&self, filter: &ProcessFilter) -> Result<Vec<ProcessRecord>, RepoError> {
        let needle = filter.name_contains.as_deref().filter(|t| !t.is_empty()).map(str::to_lowercase);
        let mut found: Vec<ProcessRecord> = self
            .processes
            .values()
            .filter(|p| !filter.running_only || p.ended.is_none())
            .filter(|p| needle.as_ref().is_none_or(|n| p.image_path.to_lowercase().contains(n.as_str())))
            .cloned()
            .collect();
        found.sort_by(|a, b| b.started.cmp(&a.started).then(a.pid.cmp(&b.pid)));
        found.truncate(filter.limit);
        Ok(found)
    }
}

impl EventRepository for InMemoryRepository {
    fn append(&mut self, events: &[EventEnvelope]) -> Result<(), RepoError> {
        self.events.extend_from_slice(events);
        Ok(())
    }

    fn query(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, RepoError> {
        let mut found: Vec<EventEnvelope> = self
            .events
            .iter()
            .filter(|e| query.from.is_none_or(|from| e.ts >= from))
            .filter(|e| query.to.is_none_or(|to| e.ts <= to))
            .filter(|e| query.category.is_none_or(|c| e.payload.category() == c))
            .filter(|e| query.process.is_none() || e.process_instance_id == query.process)
            .cloned()
            .collect();
        found.sort_by_key(|e| e.ts);
        if let Some(limit) = query.limit {
            found.truncate(limit);
        }
        Ok(found)
    }
}

impl MetricsRepository for InMemoryRepository {
    fn append_metrics(&mut self, sample: &MetricsSample) -> Result<(), RepoError> {
        self.metrics.push(sample.clone());
        Ok(())
    }

    fn system_series(&self, from: Timestamp, to: Timestamp) -> Result<Vec<SystemPoint>, RepoError> {
        let mut points: Vec<SystemPoint> = self
            .metrics
            .iter()
            .filter(|s| s.ts >= from && s.ts <= to)
            .map(|s| SystemPoint { ts: s.ts, system: s.system, others: s.others })
            .collect();
        points.sort_by_key(|p| p.ts);
        Ok(points)
    }

    fn cpu_breakdown(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<CpuBreakdown, RepoError> {
        let samples: Vec<&MetricsSample> = self.metrics.iter().filter(|s| s.ts >= from && s.ts <= to).collect();
        let window_samples = samples.len() as u32;
        if samples.is_empty() {
            return Ok(CpuBreakdown {
                window_samples: 0,
                system_avg_cpu_percent: 0.0,
                processes: Vec::new(),
                others_avg_cpu_percent: 0.0,
                unaccounted_avg_cpu_percent: 0.0,
            });
        }

        // (sum, peak, samples present) per (instance, pid)
        let mut totals: HashMap<(Option<ProcessInstanceId>, u32), (f32, f32, u32)> = HashMap::new();
        for sample in &samples {
            for process in &sample.processes {
                let entry = totals.entry((process.instance, process.pid)).or_insert((0.0, 0.0, 0));
                entry.0 += process.cpu_percent;
                entry.1 = entry.1.max(process.cpu_percent);
                entry.2 += 1;
            }
        }

        let divisor = window_samples as f32;
        let mut processes: Vec<CpuContribution> = totals
            .into_iter()
            .map(|((instance, pid), (sum, peak, present))| CpuContribution {
                instance,
                pid,
                avg_cpu_percent: sum / divisor,
                peak_cpu_percent: peak,
                samples_present: present,
            })
            .collect();
        processes.sort_by(|a, b| b.avg_cpu_percent.total_cmp(&a.avg_cpu_percent));
        // Measured across ALL listed processes, before the limit hides some of them.
        let listed_avg: f32 = processes.iter().map(|p| p.avg_cpu_percent).sum();
        processes.truncate(limit);

        let system_avg = samples.iter().map(|s| s.system.cpu_percent).sum::<f32>() / divisor;
        let others_avg = samples.iter().map(|s| s.others.cpu_percent).sum::<f32>() / divisor;
        Ok(CpuBreakdown {
            window_samples,
            system_avg_cpu_percent: system_avg,
            processes,
            others_avg_cpu_percent: others_avg,
            unaccounted_avg_cpu_percent: (system_avg - listed_avg - others_avg).max(0.0),
        })
    }
}

/// A flow owner: the process instance, or for unattributed flows the PID the source reported.
type Owner = (Option<ProcessInstanceId>, Option<u32>);

#[derive(Default)]
struct DestinationTotals {
    bytes_sent: u64,
    bytes_received: u64,
    flows: u32,
    owners: HashSet<Owner>,
}

impl FlowQueries for InMemoryRepository {
    fn top_destinations(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<Vec<Destination>, RepoError> {
        let mut totals: HashMap<(IpAddr, u16), DestinationTotals> = HashMap::new();

        for event in &self.events {
            let EventPayload::NetworkFlow {
                pid,
                remote_addr,
                remote_port,
                bytes_sent,
                bytes_received,
                window_start,
                ..
            } = &event.payload
            else {
                continue;
            };
            if *window_start < from || *window_start > to {
                continue;
            }
            let entry = totals.entry((*remote_addr, *remote_port)).or_default();
            entry.bytes_sent += bytes_sent;
            entry.bytes_received += bytes_received;
            entry.flows += 1;
            // Unattributed flows count as their own process, keyed by the PID the source reported.
            entry.owners.insert(match event.process_instance_id {
                Some(id) => (Some(id), None),
                None => (None, Some(*pid)),
            });
        }

        let mut destinations: Vec<Destination> = totals
            .into_iter()
            .map(|((remote_addr, remote_port), total)| Destination {
                remote_addr,
                remote_port,
                bytes_sent: total.bytes_sent,
                bytes_received: total.bytes_received,
                flow_count: total.flows,
                process_count: total.owners.len() as u32,
            })
            .collect();
        destinations.sort_by_key(|d| std::cmp::Reverse(d.bytes_sent + d.bytes_received));
        destinations.truncate(limit);
        Ok(destinations)
    }
    fn recent_flows(&self, from: Timestamp, to: Timestamp, limit: usize) -> Result<Vec<FlowRow>, RepoError> {
        let mut rows: Vec<FlowRow> = self
            .events
            .iter()
            .filter_map(|event| {
                let EventPayload::NetworkFlow {
                    pid,
                    protocol,
                    remote_addr,
                    remote_port,
                    bytes_sent,
                    bytes_received,
                    opened,
                    closed,
                    failed,
                    window_start,
                    window_end,
                } = &event.payload
                else {
                    return None;
                };
                if *window_start < from || *window_start > to {
                    return None;
                }
                Some(FlowRow {
                    window_start: *window_start,
                    window_end: *window_end,
                    process: event.process_instance_id,
                    pid: *pid,
                    image_path: event
                        .process_instance_id
                        .and_then(|id| self.processes.get(&id))
                        .map(|process| process.image_path.clone()),
                    protocol: *protocol,
                    remote_addr: *remote_addr,
                    remote_port: *remote_port,
                    bytes_sent: *bytes_sent,
                    bytes_received: *bytes_received,
                    opened: *opened,
                    closed: *closed,
                    failed: *failed,
                })
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.window_start));
        rows.truncate(limit);
        Ok(rows)
    }
}

impl Retention for InMemoryRepository {
    fn purge_before(&mut self, cutoff: Timestamp) -> Result<PurgeCounts, RepoError> {
        let events_before = self.events.len();
        self.events.retain(|e| e.ts >= cutoff);

        let processes_before = self.processes.len();
        // Keep running processes, and ended ones that ended at or after the cutoff.
        self.processes.retain(|_, p| p.ended.is_none_or(|ended| ended >= cutoff));

        let metrics_before = self.metrics.len();
        self.metrics.retain(|s| s.ts >= cutoff);

        Ok(PurgeCounts {
            events: (events_before - self.events.len()) as u64,
            processes: (processes_before - self.processes.len()) as u64,
            metric_samples: (metrics_before - self.metrics.len()) as u64,
        })
    }
}
