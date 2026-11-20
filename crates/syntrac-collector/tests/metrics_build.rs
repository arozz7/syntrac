//! Metrics unit rule: every CPU figure is a share of the WHOLE machine, never per core.

use syntrac_collector::metrics_build::{build_sample, machine_share, RawProcessUsage};
use syntrac_core::Timestamp;
use syntrac_testkit::fixtures::system_metrics;

fn usage(pid: u32, one_core_percent: f32) -> RawProcessUsage {
    RawProcessUsage {
        pid,
        cpu_percent_of_one_core: one_core_percent,
        memory_bytes: 1_000,
        gpu_percent: 0.0,
        gpu_memory_bytes: 0,
    }
}

fn ts() -> Timestamp {
    Timestamp::from_micros(1)
}

#[test]
fn one_fully_busy_thread_is_a_small_share_of_a_big_machine() {
    let share = machine_share(100.0, 28);
    assert!((share - 3.5714).abs() < 1e-3, "got {share}");
}

#[test]
fn a_process_using_every_core_is_one_hundred_percent() {
    assert!((machine_share(2800.0, 28) - 100.0).abs() < 1e-3);
}

#[test]
fn shares_are_clamped_to_the_machine_total() {
    assert!(machine_share(9999.0, 4) <= 100.0);
    assert!(machine_share(-5.0, 4) >= 0.0);
}

#[test]
fn zero_logical_cpus_does_not_divide_by_zero() {
    assert!(machine_share(50.0, 0).is_finite());
}

#[test]
fn the_heaviest_processes_are_listed_and_everything_else_is_summed_as_the_long_tail() {
    let processes: Vec<_> = (1..=30).map(|pid| usage(pid, pid as f32 * 4.0)).collect(); // pid 30 busiest
    let sample = build_sample(ts(), system_metrics(20.0), processes, 4, 5);

    let listed: Vec<u32> = sample.processes.iter().map(|p| p.pid).collect();
    assert_eq!(listed, vec![30, 29, 28, 27, 26], "heaviest first");

    assert_eq!(sample.others.process_count, 25);
    // pids 1..=25: sum of (pid * 4) one-core percent = 4 * 325 = 1300, over 4 CPUs = 325 -> clamped by total share rule per process
    let expected_others: f32 = (1..=25).map(|pid| machine_share(pid as f32 * 4.0, 4)).sum();
    assert!((sample.others.cpu_percent - expected_others).abs() < 1e-2);
    assert_eq!(sample.others.memory_bytes, 25 * 1_000);
}

#[test]
fn listed_plus_long_tail_account_for_all_process_cpu() {
    let processes: Vec<_> = (1..=12).map(|pid| usage(pid, pid as f32)).collect();
    let total: f32 = processes.iter().map(|p| machine_share(p.cpu_percent_of_one_core, 8)).sum();
    let sample = build_sample(ts(), system_metrics(10.0), processes, 8, 3);

    let listed: f32 = sample.processes.iter().map(|p| p.cpu_percent).sum();
    assert!((listed + sample.others.cpu_percent - total).abs() < 1e-3);
}

#[test]
fn fewer_processes_than_the_limit_leaves_no_long_tail() {
    let sample = build_sample(ts(), system_metrics(5.0), vec![usage(1, 10.0), usage(2, 20.0)], 4, 10);
    assert_eq!(sample.processes.len(), 2);
    assert_eq!(sample.others.process_count, 0);
    assert_eq!(sample.others.cpu_percent, 0.0);
}

#[test]
fn listed_processes_start_unresolved_and_are_resolved_later_by_the_pipeline() {
    let sample = build_sample(ts(), system_metrics(5.0), vec![usage(1, 10.0)], 4, 10);
    assert!(sample.processes[0].instance.is_none());
}

#[test]
fn ties_are_broken_by_pid_so_output_is_deterministic() {
    let sample = build_sample(ts(), system_metrics(5.0), vec![usage(9, 10.0), usage(3, 10.0), usage(5, 10.0)], 4, 2);
    let listed: Vec<u32> = sample.processes.iter().map(|p| p.pid).collect();
    assert_eq!(listed, vec![3, 5]);
}

fn gpu_usage(pid: u32, one_core_percent: f32, gpu_percent: f32, gpu_memory_bytes: u64) -> RawProcessUsage {
    RawProcessUsage { gpu_percent, gpu_memory_bytes, ..usage(pid, one_core_percent) }
}

#[test]
fn a_process_heavy_on_the_gpu_and_light_on_the_cpu_is_still_listed_by_name() {
    // pids 1..=30: pid 30 is the busiest on the CPU; pid 3 is the lightest-but-one on the CPU and busy on the GPU.
    let mut processes: Vec<_> = (1..=30).map(|pid| usage(pid, pid as f32 * 4.0)).collect();
    processes[2] = gpu_usage(3, 12.0, 80.0, 2_000);
    let sample = build_sample(ts(), system_metrics(20.0), processes, 4, 5);

    let listed: Vec<u32> = sample.processes.iter().map(|p| p.pid).collect();
    assert_eq!(listed, vec![30, 29, 28, 27, 26, 3], "the top by CPU first, then the GPU extras");
    let extra = sample.processes.last().unwrap();
    assert_eq!((extra.gpu_percent, extra.gpu_memory_bytes), (80.0, 2_000));
}

#[test]
fn a_process_already_listed_for_its_cpu_is_not_listed_twice() {
    let mut processes: Vec<_> = (1..=10).map(|pid| usage(pid, pid as f32 * 4.0)).collect();
    processes[9] = gpu_usage(10, 40.0, 90.0, 5);
    let sample = build_sample(ts(), system_metrics(20.0), processes, 4, 3);
    let listed: Vec<u32> = sample.processes.iter().map(|p| p.pid).collect();
    assert_eq!(listed, vec![10, 9, 8]);
    assert_eq!(sample.processes[0].gpu_percent, 90.0);
}

#[test]
fn only_a_few_gpu_extras_are_listed_the_heaviest_first_and_a_quiet_gpu_is_not_an_extra() {
    let mut processes: Vec<_> = (1..=30).map(|pid| usage(pid, 1.0)).collect();
    // pids 11..=18 use the GPU at 10..=17%, pid 19 only 0.5% (below the listing floor).
    for pid in 11..=18_u32 {
        processes[pid as usize - 1] = gpu_usage(pid, 1.0, pid as f32 - 1.0, 0);
    }
    processes[18] = gpu_usage(19, 1.0, 0.5, 0);
    let sample = build_sample(ts(), system_metrics(5.0), processes, 4, 2);

    let extras: Vec<u32> = sample.processes.iter().skip(2).map(|p| p.pid).collect();
    assert_eq!(extras, vec![18, 17, 16, 15, 14], "five extras, heaviest GPU first");
}

#[test]
fn listed_plus_long_tail_account_for_all_gpu_memory_and_the_tail_keeps_its_largest_gpu_figure() {
    let mut processes: Vec<_> = (1..=12).map(|pid| gpu_usage(pid, pid as f32, 0.0, 100)).collect();
    processes[0] = gpu_usage(1, 1.0, 0.8, 700); // below the listing floor: stays in the tail
    let total: u64 = processes.iter().map(|p| p.gpu_memory_bytes).sum();
    let sample = build_sample(ts(), system_metrics(10.0), processes, 8, 3);

    let listed: u64 = sample.processes.iter().map(|p| p.gpu_memory_bytes).sum();
    assert_eq!(listed + sample.others.gpu_memory_bytes, total);
    assert!((sample.others.gpu_percent - 0.8).abs() < 1e-6);
}

#[test]
fn without_gpu_figures_nothing_changes() {
    let processes: Vec<_> = (1..=30).map(|pid| usage(pid, pid as f32 * 4.0)).collect();
    let sample = build_sample(ts(), system_metrics(20.0), processes, 4, 5);
    assert_eq!(sample.processes.len(), 5);
    assert_eq!(sample.others.gpu_percent, 0.0);
    assert_eq!(sample.others.gpu_memory_bytes, 0);
}
