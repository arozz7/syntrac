//! Resource metrics from the OS via `sysinfo`, shaped by the tested `metrics_build` functions.
//!
//! Unit rule: `sysinfo` reports per-process CPU as a percentage of ONE core; `build_sample` converts
//! it to a share of the whole machine. The global figure is already whole-machine.

use syntrac_core::{gpu_figures, GpuSource, MetricsSample, MetricsSource, SourceError, SystemMetrics, Timestamp};
use sysinfo::{Networks, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::metrics_build::{build_sample, RawProcessUsage};

/// How many of the heaviest processes are listed individually per sample.
pub const DEFAULT_TOP_PROCESSES: usize = 25;

pub struct SysinfoMetricsSource {
    system: System,
    networks: Networks,
    logical_cpus: usize,
    top_n: usize,
    /// Where the GPU figures come from; `None` means they are not recorded (switched off, or the counters are not available).
    gpu: Option<Box<dyn GpuSource + Send>>,
}

impl SysinfoMetricsSource {
    pub fn new(top_n: usize) -> Self {
        let mut system = System::new();
        // Prime the counters: CPU usage is a delta, so the first reading after creation is meaningless.
        system.refresh_cpu_usage();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
        let logical_cpus = system.cpus().len().max(1);
        Self { system, networks: Networks::new_with_refreshed_list(), logical_cpus, top_n, gpu: None }
    }

    /// Records GPU figures from this source as well (docs/ALERTS-PLAN.md, slice G1).
    pub fn with_gpu(mut self, gpu: Box<dyn GpuSource + Send>) -> Self {
        self.gpu = Some(gpu);
        self
    }
}

fn process_refresh() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage()
}

impl MetricsSource for SysinfoMetricsSource {
    fn name(&self) -> &str {
        "sysinfo"
    }

    fn sample(&mut self, now: Timestamp) -> Result<MetricsSample, SourceError> {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh());
        self.networks.refresh(true);

        // `None` when the counters are not available (or switched off): recorded as "not available", never as zero.
        let gpu = self.gpu.as_mut().and_then(|source| source.read()).map(|reading| gpu_figures(&reading));

        let processes: Vec<RawProcessUsage> = self
            .system
            .processes()
            .values()
            .map(|process| {
                let pid = process.pid().as_u32();
                let figure = gpu.as_ref().and_then(|figures| figures.by_pid.get(&pid)).copied().unwrap_or_default();
                RawProcessUsage {
                    pid,
                    cpu_percent_of_one_core: process.cpu_usage(),
                    memory_bytes: process.memory(),
                    gpu_percent: figure.percent,
                    gpu_memory_bytes: figure.memory_bytes,
                }
            })
            .collect();

        // Bytes moved since the previous sample, summed over all processes / all interfaces.
        let (disk_read, disk_written) = self.system.processes().values().fold((0u64, 0u64), |(read, written), p| {
            let usage = p.disk_usage();
            (read + usage.read_bytes, written + usage.written_bytes)
        });
        let (net_received, net_sent) = self
            .networks
            .iter()
            .fold((0u64, 0u64), |(received, sent), (_, data)| (received + data.received(), sent + data.transmitted()));

        let system = SystemMetrics {
            cpu_percent: self.system.global_cpu_usage().clamp(0.0, 100.0),
            memory_used_bytes: self.system.used_memory(),
            memory_total_bytes: self.system.total_memory(),
            disk_read_bytes: disk_read,
            disk_write_bytes: disk_written,
            net_received_bytes: net_received,
            net_sent_bytes: net_sent,
            gpu_percent: gpu.as_ref().map(|figures| figures.busiest_percent),
        };
        Ok(build_sample(now, system, processes, self.logical_cpus, self.top_n))
    }
}
