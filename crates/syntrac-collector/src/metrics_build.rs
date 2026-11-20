//! Turns raw per-process usage into a [`MetricsSample`] using one unit: share of the WHOLE machine (CPU). GPU figures are the
//! exception by design: a share of one GPU engine (see `syntrac_core::gpu`).

use syntrac_core::{MetricsSample, OtherProcesses, ProcessMetric, SystemMetrics, Timestamp};

/// How many processes are listed for their GPU use on top of the heaviest by CPU, so a program that is heavy on the GPU and light on
/// the CPU (a miner) is named instead of vanishing into the long tail.
pub const GPU_EXTRA_PROCESSES: usize = 5;

/// A process must use at least this share of a GPU engine to be listed for its GPU use alone.
pub const GPU_LISTING_FLOOR_PERCENT: f32 = 1.0;

/// One process as measured by the OS-facing sampler: CPU is a percentage of ONE core (can exceed 100).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawProcessUsage {
    pub pid: u32,
    pub cpu_percent_of_one_core: f32,
    pub memory_bytes: u64,
    /// Share (0-100) of the process's busiest GPU engine; 0 when the GPU counters are not available.
    pub gpu_percent: f32,
    pub gpu_memory_bytes: u64,
}

/// Converts a per-core percentage to a share of the whole machine, clamped to 0-100.
pub fn machine_share(per_core_percent: f32, logical_cpus: usize) -> f32 {
    (per_core_percent / logical_cpus.max(1) as f32).clamp(0.0, 100.0)
}

/// Keeps the `top_n` heaviest processes by CPU (ties broken by PID), then up to [`GPU_EXTRA_PROCESSES`] more that use the GPU
/// (heaviest first), and sums everything else into the long tail, so listed + tail always account for all measured process CPU and
/// GPU memory.
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
    let mut rest = processes.split_off(top_n.min(processes.len()));

    // The GPU extras come out of the rest: heaviest GPU first, ties broken by PID.
    rest.sort_by(|a, b| b.gpu_percent.total_cmp(&a.gpu_percent).then_with(|| a.pid.cmp(&b.pid)));
    let extras =
        rest.iter().take(GPU_EXTRA_PROCESSES).take_while(|p| p.gpu_percent >= GPU_LISTING_FLOOR_PERCENT).count();
    let tail = rest.split_off(extras);
    processes.extend(rest);

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
                gpu_percent: p.gpu_percent,
                gpu_memory_bytes: p.gpu_memory_bytes,
            })
            .collect(),
        others: OtherProcesses {
            process_count: u32::try_from(tail.len()).unwrap_or(u32::MAX),
            cpu_percent: tail.iter().map(|p| machine_share(p.cpu_percent_of_one_core, logical_cpus)).sum(),
            memory_bytes: tail.iter().map(|p| p.memory_bytes).sum(),
            gpu_percent: tail.iter().map(|p| p.gpu_percent).fold(0.0, f32::max),
            gpu_memory_bytes: tail.iter().map(|p| p.gpu_memory_bytes).sum(),
        },
    }
}
