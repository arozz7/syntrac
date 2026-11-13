//! Metrics unit rule: every CPU figure is a share of the WHOLE machine, never per core.

use syntrac_collector::metrics_build::{build_sample, machine_share, RawProcessUsage};
use syntrac_core::Timestamp;
use syntrac_testkit::fixtures::system_metrics;

fn usage(pid: u32, one_core_percent: f32) -> RawProcessUsage {
    RawProcessUsage { pid, cpu_percent_of_one_core: one_core_percent, memory_bytes: 1_000 }
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
