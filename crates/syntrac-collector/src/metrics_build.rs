//! Turns raw per-process usage into a [`MetricsSample`] using one unit: share of the WHOLE machine.

use syntrac_core::{MetricsSample, OtherProcesses, ProcessMetric, SystemMetrics, Timestamp};

/// One process as measured by the OS-facing sampler: CPU is a percentage of ONE core (can exceed 100).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawProcessUsage {
    pub pid: u32,
    pub cpu_percent_of_one_core: f32,
    pub memory_bytes: u64,
}

/// Converts a per-core percentage to a share of the whole machine, clamped to 0-100.
pub fn machine_share(per_core_percent: f32, logical_cpus: usize) -> f32 {
    (per_core_percent / logical_cpus.max(1) as f32).clamp(0.0, 100.0)
}

/// Keeps the `top_n` heaviest processes by CPU (ties broken by PID) and sums everything else into
/// the long tail, so listed + tail always account for all measured process CPU.
pub fn build_sample(
    ts: Timestamp,
    system: SystemMetrics,
    mut processes: Vec<RawProcessUsage>,
    logical_cpus: usize,
    top_n: usize,
) -> MetricsSample {
    processes.sort_by(|a, b| {
        b.cpu_percent_of_one_core.total_cmp(&a.cpu_percent_of_one_core).then_with(|| a.pid.cmp(&b.pid))
    });
    let tail = processes.split_off(top_n.min(processes.len()));

    MetricsSample {
        ts,
        system,
        processes: processes
            .into_iter()
            .map(|p| ProcessMetric {
                pid: p.pid,
                instance: None,
                cpu_percent: machine_share(p.cpu_percent_of_one_core, logical_cpus),
                memory_bytes: p.memory_bytes,
            })
            .collect(),
        others: OtherProcesses {
            process_count: u32::try_from(tail.len()).unwrap_or(u32::MAX),
            cpu_percent: tail.iter().map(|p| machine_share(p.cpu_percent_of_one_core, logical_cpus)).sum(),
            memory_bytes: tail.iter().map(|p| p.memory_bytes).sum(),
        },
    }
}
