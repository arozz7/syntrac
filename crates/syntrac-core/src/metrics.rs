//! System and per-process resource metrics.
//!
//! Unit rule: every CPU figure is a percentage of the WHOLE machine (0-100), never per core.
//! On a 28-thread machine, one fully busy thread is about 3.6%.

use serde::{Deserialize, Serialize};

use crate::identity::ProcessInstanceId;
use crate::time::Timestamp;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemMetrics {
    /// Share of the whole machine, 0-100.
    pub cpu_percent: f32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    /// Bytes moved since the previous sample.
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub net_received_bytes: u64,
    pub net_sent_bytes: u64,
    /// The busiest GPU engine over all adapters, 0-100 (see `gpu`); `None` when the GPU counters are not available.
    #[serde(default)]
    pub gpu_percent: Option<f32>,
}

/// One process's usage in one sample.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProcessMetric {
    pub pid: u32,
    /// Resolved by the pipeline; sources leave this empty.
    pub instance: Option<ProcessInstanceId>,
    /// Share of the whole machine, 0-100.
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    /// Share (0-100) of the process's busiest GPU engine, NOT of the whole machine (see `gpu`). 0 also when the counters are not available.
    #[serde(default)]
    pub gpu_percent: f32,
    /// Dedicated GPU memory.
    #[serde(default)]
    pub gpu_memory_bytes: u64,
}

/// Everything that is not in the per-process list, so the totals still add up.
/// Without this, a long tail of small consumers would be invisible.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct OtherProcesses {
    pub process_count: u32,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    /// The largest single GPU figure in the tail (figures of different engines cannot be added).
    #[serde(default)]
    pub gpu_percent: f32,
    /// The sum of the tail's dedicated GPU memory.
    #[serde(default)]
    pub gpu_memory_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsSample {
    pub ts: Timestamp,
    pub system: SystemMetrics,
    /// The heaviest processes in this sample.
    pub processes: Vec<ProcessMetric>,
    pub others: OtherProcesses,
}

/// One point of the system time series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SystemPoint {
    pub ts: Timestamp,
    pub system: SystemMetrics,
    pub others: OtherProcesses,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CpuContribution {
    pub instance: Option<ProcessInstanceId>,
    pub pid: u32,
    /// Averaged over ALL samples in the window (absent samples count as zero).
    pub avg_cpu_percent: f32,
    pub peak_cpu_percent: f32,
    /// How many samples this process appeared in.
    pub samples_present: u32,
}

/// "What used the CPU between A and B?" The answer to a sustained, unexplained load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuBreakdown {
    pub window_samples: u32,
    pub system_avg_cpu_percent: f32,
    /// Heaviest first.
    pub processes: Vec<CpuContribution>,
    /// Average CPU of everything outside the per-sample top list.
    pub others_avg_cpu_percent: f32,
    /// System CPU not explained by ANY measured process: kernel/interrupt/driver time, and processes the
    /// collector is not allowed to read. Reported explicitly so a gap is visible rather than silently lost.
    pub unaccounted_avg_cpu_percent: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpuContribution {
    pub instance: Option<ProcessInstanceId>,
    pub pid: u32,
    /// A share of the process's busiest GPU engine (see `gpu`), averaged over the samples that HAD a GPU reading (absent samples
    /// count as zero).
    pub avg_percent: f32,
    pub peak_percent: f32,
    /// The highest dedicated GPU memory in the window.
    pub memory_peak_bytes: u64,
}

/// "What used the GPU between A and B?" The figures are shares of ONE engine, so unlike the CPU they do not add up to the system figure
/// (different processes may be on different engines): there is no "unaccounted" line, and the tail is reported as its largest single
/// share, never as a sum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuBreakdown {
    /// Samples in the window, and how many of them had a GPU reading (the counters may have been unavailable for some).
    pub window_samples: u32,
    pub reading_samples: u32,
    /// The busiest engine of the machine, averaged over the samples that had a reading, and its highest value.
    pub system_avg_percent: f32,
    pub system_peak_percent: f32,
    /// Heaviest first; only processes that used the GPU at all.
    pub processes: Vec<GpuContribution>,
    /// The largest single GPU share of any process outside the per-sample list.
    pub others_peak_percent: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeCounts {
    pub events: u64,
    pub processes: u64,
    pub metric_samples: u64,
}
