//! The aggregation maths of one hour. Pure: rows in, rollup rows out.

use std::net::{IpAddr, Ipv4Addr};

use syntrac_core::rollup::{
    executable_key, hour_start, next_hour, roll_up_hour, FlowInputRow, HourInput, LedgerEntry, ProcessSampleRow,
    StartRow, HOUR_MICROS,
};
use syntrac_core::{OtherProcesses, SystemMetrics, SystemPoint, Timestamp};

const MIN: i64 = 60 * 1_000_000;
/// 2026-11-13 12:00:00 UTC, an exact hour.
const H: i64 = 1_794_571_200_000_000 / HOUR_MICROS * HOUR_MICROS;

fn at(minutes: i64) -> Timestamp {
    Timestamp::from_micros(H + minutes * MIN)
}

fn hour() -> Timestamp {
    Timestamp::from_micros(H)
}

fn point(minutes: i64, cpu: f32, memory: u64, others_cpu: f32) -> SystemPoint {
    SystemPoint {
        ts: at(minutes),
        system: SystemMetrics {
            cpu_percent: cpu,
            memory_used_bytes: memory,
            memory_total_bytes: 16_000,
            disk_read_bytes: 10,
            disk_write_bytes: 20,
            net_received_bytes: 100,
            net_sent_bytes: 200,
            gpu_percent: None,
        },
        others: OtherProcesses { process_count: 5, cpu_percent: others_cpu, memory_bytes: 1, ..Default::default() },
    }
}

fn sample(minutes: i64, exe: Option<&str>, cpu: f32, memory: u64) -> ProcessSampleRow {
    ProcessSampleRow {
        ts: at(minutes),
        exe_path: exe.map(str::to_owned),
        cpu_percent: cpu,
        memory_bytes: memory,
        gpu_percent: 0.0,
        gpu_memory_bytes: 0,
    }
}

fn flow(minutes: i64, exe: Option<&str>, addr: [u8; 4], port: u16, sent: u64, received: u64) -> FlowInputRow {
    FlowInputRow {
        window_start: at(minutes),
        exe_path: exe.map(str::to_owned),
        remote_addr: IpAddr::V4(Ipv4Addr::from(addr)),
        remote_port: port,
        bytes_sent: sent,
        bytes_received: received,
    }
}

fn input() -> HourInput {
    HourInput::empty(hour())
}

// ---- hour boundaries

#[test]
fn a_timestamp_on_the_boundary_belongs_to_the_new_hour() {
    assert_eq!(hour_start(at(0)), hour());
    assert_eq!(hour_start(Timestamp::from_micros(H - 1)), Timestamp::from_micros(H - HOUR_MICROS));
    assert_eq!(hour_start(at(59)), hour());
    assert_eq!(next_hour(hour()), Timestamp::from_micros(H + HOUR_MICROS));
}

#[test]
fn hours_before_the_epoch_round_down_not_toward_zero() {
    assert_eq!(hour_start(Timestamp::from_micros(-1)), Timestamp::from_micros(-HOUR_MICROS));
}

#[test]
fn rows_outside_the_hour_are_ignored() {
    let mut rows = input();
    rows.system = vec![point(10, 10.0, 100, 0.0), point(-1, 99.0, 999, 0.0), point(60, 99.0, 999, 0.0)];
    rows.process_samples = vec![sample(10, Some(r"C:\a.exe"), 5.0, 1), sample(60, Some(r"C:\a.exe"), 50.0, 1)];
    rows.flows = vec![flow(60, Some(r"C:\a.exe"), [1, 1, 1, 1], 443, 1, 1)];
    let rollup = roll_up_hour(&rows);
    assert_eq!(rollup.system.unwrap().samples, 1, "only the sample at minute 10");
    assert_eq!(rollup.processes.len(), 1);
    assert_eq!(rollup.processes[0].samples_present, 1);
    assert!(rollup.traffic.is_empty());
}

// ---- system

#[test]
fn an_empty_hour_has_no_rows() {
    let rollup = roll_up_hour(&input());
    assert!(rollup.system.is_none() && rollup.processes.is_empty() && rollup.traffic.is_empty());
}

#[test]
fn system_rows_average_peak_and_total() {
    let mut rows = input();
    rows.system = vec![point(0, 10.0, 1_000, 1.0), point(5, 30.0, 3_000, 3.0), point(10, 20.0, 2_000, 2.0)];
    let system = roll_up_hour(&rows).system.unwrap();
    assert_eq!(system.samples, 3);
    assert!((system.cpu_avg - 20.0).abs() < 1e-9);
    assert!((system.cpu_max - 30.0).abs() < 1e-9);
    assert!((system.others_cpu_avg - 2.0).abs() < 1e-9);
    assert_eq!((system.memory_used_avg, system.memory_used_max, system.memory_total), (2_000, 3_000, 16_000));
    assert_eq!((system.disk_read, system.disk_write, system.net_received, system.net_sent), (30, 60, 300, 600));
}

#[test]
fn a_partial_hour_reports_how_many_samples_it_really_had() {
    let mut rows = input();
    rows.system = vec![point(50, 40.0, 1, 0.0), point(55, 60.0, 1, 0.0)];
    let system = roll_up_hour(&rows).system.unwrap();
    assert_eq!(system.samples, 2, "a consumer can tell this hour was only partly observed");
    assert!((system.cpu_avg - 50.0).abs() < 1e-9);
}

#[test]
fn the_order_the_rows_arrive_in_does_not_change_the_result() {
    let mut forward = input();
    forward.system = vec![point(0, 10.1, 1, 0.3), point(5, 30.7, 3, 0.9), point(10, 20.3, 2, 0.1)];
    forward.process_samples = vec![
        sample(0, Some(r"C:\a.exe"), 0.7, 5),
        sample(0, Some(r"C:\a.exe"), 0.1, 6),
        sample(5, Some(r"C:\b.exe"), 9.9, 7),
        sample(10, None, 0.3, 8),
    ];
    let mut shuffled = forward.clone();
    shuffled.system.reverse();
    shuffled.process_samples.reverse();
    assert_eq!(roll_up_hour(&forward), roll_up_hour(&shuffled), "float sums are added in one fixed order");
}

#[test]
fn rolling_the_same_rows_twice_gives_the_same_rollup() {
    let mut rows = input();
    rows.system = vec![point(0, 10.0, 1, 0.0)];
    rows.process_samples = vec![sample(0, Some(r"C:\a.exe"), 3.0, 5)];
    rows.starts = vec![StartRow { ts: at(1), exe_path: Some(r"C:\a.exe".into()) }];
    rows.flows = vec![flow(2, Some(r"C:\a.exe"), [8, 8, 8, 8], 53, 1, 2)];
    assert_eq!(roll_up_hour(&rows), roll_up_hour(&rows));
}

// ---- processes

#[test]
fn an_executable_is_summed_over_its_instances_and_averaged_over_every_sample_of_the_hour() {
    let mut rows = input();
    // Four samples in the hour. chrome has two instances in the first two samples, one in the third, none in the fourth.
    rows.system = vec![point(0, 0.0, 0, 0.0), point(1, 0.0, 0, 0.0), point(2, 0.0, 0, 0.0), point(3, 0.0, 0, 0.0)];
    rows.process_samples = vec![
        sample(0, Some(r"C:\Chrome\chrome.exe"), 4.0, 100),
        sample(0, Some(r"C:\Chrome\chrome.exe"), 2.0, 50),
        sample(1, Some(r"C:\Chrome\chrome.exe"), 6.0, 200),
        sample(1, Some(r"C:\Chrome\chrome.exe"), 2.0, 100),
        sample(2, Some(r"C:\Chrome\chrome.exe"), 2.0, 300),
    ];
    let rollup = roll_up_hour(&rows);
    let chrome = &rollup.processes[0];
    assert_eq!(chrome.exe.as_ref().unwrap().key, r"c:\chrome\chrome.exe");
    assert_eq!(chrome.samples_present, 3);
    assert!(
        (chrome.cpu_avg - (6.0 + 8.0 + 2.0) / 4.0).abs() < 1e-9,
        "absent samples count as zero: {}",
        chrome.cpu_avg
    );
    assert!((chrome.cpu_peak - 8.0).abs() < 1e-9, "the highest per-sample SUM, not the highest instance");
    assert_eq!(chrome.memory_peak, 300);
}

#[test]
fn the_same_program_in_different_letter_case_is_one_executable() {
    let mut rows = input();
    rows.system = vec![point(0, 0.0, 0, 0.0)];
    rows.process_samples =
        vec![sample(0, Some(r"C:\Tools\App.EXE"), 1.0, 1), sample(0, Some(r"c:\tools\app.exe"), 2.0, 1)];
    let rollup = roll_up_hour(&rows);
    assert_eq!(rollup.processes.len(), 1);
    assert_eq!(
        rollup.processes[0].exe.as_ref().unwrap().path,
        r"C:\Tools\App.EXE",
        "the smallest spelling, deterministically"
    );
}

#[test]
fn unknown_owners_are_grouped_as_unattributed() {
    let mut rows = input();
    rows.system = vec![point(0, 0.0, 0, 0.0)];
    rows.process_samples = vec![sample(0, None, 1.0, 5), sample(0, Some("  "), 2.0, 5)];
    let rollup = roll_up_hour(&rows);
    assert_eq!(rollup.processes.len(), 1);
    assert!(rollup.processes[0].exe.is_none());
    assert!((rollup.processes[0].cpu_peak - 3.0).abs() < 1e-9);
}

#[test]
fn starts_are_counted_even_when_the_program_was_never_sampled() {
    let mut rows = input();
    rows.starts = vec![
        StartRow { ts: at(3), exe_path: Some(r"C:\quick.exe".into()) },
        StartRow { ts: at(9), exe_path: Some(r"C:\QUICK.exe".into()) },
    ];
    let rollup = roll_up_hour(&rows);
    assert_eq!(rollup.processes.len(), 1);
    assert_eq!((rollup.processes[0].starts, rollup.processes[0].samples_present), (2, 0));
    assert_eq!((rollup.processes[0].first_seen, rollup.processes[0].last_seen), (at(3), at(9)));
}

#[test]
fn a_process_spanning_an_hour_boundary_is_split_by_sample_time_and_starts_only_once() {
    let started = StartRow { ts: at(58), exe_path: Some(r"C:\long.exe".into()) };
    let samples = vec![
        sample(58, Some(r"C:\long.exe"), 1.0, 1),
        sample(59, Some(r"C:\long.exe"), 1.0, 1),
        sample(61, Some(r"C:\long.exe"), 1.0, 1),
        sample(62, Some(r"C:\long.exe"), 1.0, 1),
    ];

    let mut first = input();
    first.system = vec![point(58, 0.0, 0, 0.0), point(59, 0.0, 0, 0.0)];
    first.process_samples = samples.clone();
    first.starts = vec![started.clone()];
    let mut second = HourInput::empty(next_hour(hour()));
    second.system = vec![point(61, 0.0, 0, 0.0), point(62, 0.0, 0, 0.0)];
    second.process_samples = samples;
    second.starts = vec![started];

    let (a, b) = (roll_up_hour(&first), roll_up_hour(&second));
    assert_eq!((a.processes[0].samples_present, a.processes[0].starts), (2, 1));
    assert_eq!(
        (b.processes[0].samples_present, b.processes[0].starts),
        (2, 0),
        "the start belongs to the first hour only"
    );
}

#[test]
fn process_rows_are_sorted_with_unattributed_first() {
    let mut rows = input();
    rows.system = vec![point(0, 0.0, 0, 0.0)];
    rows.process_samples =
        vec![sample(0, Some(r"C:\z.exe"), 1.0, 1), sample(0, None, 1.0, 1), sample(0, Some(r"C:\a.exe"), 1.0, 1)];
    let keys: Vec<_> = roll_up_hour(&rows).processes.iter().map(|p| p.exe.as_ref().map(|e| e.key.clone())).collect();
    assert_eq!(keys, vec![None, Some(r"c:\a.exe".into()), Some(r"c:\z.exe".into())]);
}

// ---- traffic

#[test]
fn traffic_is_summed_per_executable_and_destination_without_the_port_in_the_key() {
    let mut rows = input();
    rows.flows = vec![
        flow(1, Some(r"C:\a.exe"), [9, 9, 9, 9], 50_001, 10, 100),
        flow(2, Some(r"C:\a.exe"), [9, 9, 9, 9], 50_002, 20, 200),
        flow(3, Some(r"C:\a.exe"), [9, 9, 9, 9], 443, 5, 5),
        flow(4, Some(r"C:\a.exe"), [7, 7, 7, 7], 443, 1, 1),
        flow(5, Some(r"C:\b.exe"), [9, 9, 9, 9], 443, 1, 1),
    ];
    let traffic = roll_up_hour(&rows).traffic;
    assert_eq!(traffic.len(), 3, "(a, 7.7.7.7), (a, 9.9.9.9), (b, 9.9.9.9)");
    let a_to_9 = traffic
        .iter()
        .find(|t| t.exe.as_ref().unwrap().key == r"c:\a.exe" && t.addr == IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)))
        .unwrap();
    assert_eq!((a_to_9.bytes_sent, a_to_9.bytes_received, a_to_9.flows), (35, 305, 3));
    assert_eq!((a_to_9.first_seen, a_to_9.last_seen), (at(1), at(3)));
    assert_eq!(a_to_9.distinct_ports, 3);
    assert_eq!(a_to_9.top_port, 50_002, "the port that carried the most bytes (220)");
}

#[test]
fn the_busiest_port_ties_go_to_the_lowest_port() {
    let mut rows = input();
    rows.flows = vec![
        flow(1, Some(r"C:\a.exe"), [9, 9, 9, 9], 8080, 50, 50),
        flow(2, Some(r"C:\a.exe"), [9, 9, 9, 9], 443, 100, 0),
        flow(3, Some(r"C:\a.exe"), [9, 9, 9, 9], 9000, 0, 100),
    ];
    assert_eq!(roll_up_hour(&rows).traffic[0].top_port, 443);
}

#[test]
fn traffic_without_a_known_owner_is_unattributed_but_kept() {
    let mut rows = input();
    rows.flows = vec![flow(1, None, [9, 9, 9, 9], 443, 3, 4)];
    let traffic = roll_up_hour(&rows).traffic;
    assert_eq!(traffic.len(), 1);
    assert!(traffic[0].exe.is_none());
    assert_eq!(traffic[0].bytes_sent + traffic[0].bytes_received, 7);
}

#[test]
fn huge_byte_counts_saturate_instead_of_wrapping() {
    let mut rows = input();
    rows.flows = vec![
        flow(1, Some(r"C:\a.exe"), [9, 9, 9, 9], 443, u64::MAX, 0),
        flow(2, Some(r"C:\a.exe"), [9, 9, 9, 9], 443, 10, 0),
    ];
    assert_eq!(roll_up_hour(&rows).traffic[0].bytes_sent, u64::MAX);
}

// ---- keys and the ledger

#[test]
fn the_executable_key_is_the_lower_cased_trimmed_path_and_empty_has_none() {
    assert_eq!(executable_key(r"  C:\Windows\System32\SVCHOST.exe "), Some(r"c:\windows\system32\svchost.exe".into()));
    assert_eq!(executable_key(""), None);
    assert_eq!(executable_key("   "), None);
}

#[test]
fn a_ledger_entry_only_ever_widens() {
    let entry = LedgerEntry { first_seen: at(10), last_seen: at(20) };
    assert_eq!(entry.widened(at(15), at(16)), entry, "an observation inside the range changes nothing");
    assert_eq!(entry.widened(at(5), at(30)), LedgerEntry { first_seen: at(5), last_seen: at(30) });
    assert_eq!(
        entry.widened(at(5), at(30)).widened(at(10), at(20)),
        LedgerEntry { first_seen: at(5), last_seen: at(30) }
    );
}

// ---- GPU (docs/ALERTS-PLAN.md, slice G1)

fn gpu_point(minutes: i64, gpu: Option<f32>) -> SystemPoint {
    let mut point = point(minutes, 10.0, 1_000, 0.0);
    point.system.gpu_percent = gpu;
    point
}

fn gpu_sample(minutes: i64, exe: &str, gpu: f32, gpu_memory: u64) -> ProcessSampleRow {
    ProcessSampleRow { gpu_percent: gpu, gpu_memory_bytes: gpu_memory, ..sample(minutes, Some(exe), 0.0, 1) }
}

#[test]
fn the_system_gpu_is_averaged_over_the_samples_that_had_a_reading() {
    let mut rows = input();
    rows.system = vec![gpu_point(0, Some(10.0)), gpu_point(5, Some(30.0)), gpu_point(10, None)];
    let system = roll_up_hour(&rows).system.unwrap();
    assert!((system.gpu_avg.unwrap() - 20.0).abs() < 1e-9, "the sample without a reading is not a zero");
    assert!((system.gpu_max.unwrap() - 30.0).abs() < 1e-9);
}

#[test]
fn an_hour_without_any_gpu_reading_has_no_gpu_figures_at_all() {
    let mut rows = input();
    rows.system = vec![gpu_point(0, None), gpu_point(5, None)];
    let system = roll_up_hour(&rows).system.unwrap();
    assert_eq!((system.gpu_avg, system.gpu_max), (None, None));
}

#[test]
fn a_programs_gpu_is_averaged_over_all_samples_of_the_hour_with_its_peak_and_memory_peak() {
    let mut rows = input();
    rows.system = (0..4).map(|n| gpu_point(n * 5, Some(1.0))).collect();
    rows.process_samples = vec![
        gpu_sample(0, r"C:\miner.exe", 40.0, 1_000),
        gpu_sample(5, r"C:\miner.exe", 20.0, 3_000),
        // absent from the other two samples: counts as zero in the average
    ];
    let hour = &roll_up_hour(&rows).processes[0];
    assert!((hour.gpu_avg - 15.0).abs() < 1e-9, "(40 + 20) / 4 samples, got {}", hour.gpu_avg);
    assert!((hour.gpu_peak - 40.0).abs() < 1e-9);
    assert_eq!(hour.gpu_memory_peak, 3_000);
}

#[test]
fn several_instances_of_one_program_are_added_up_per_sample_but_never_above_a_full_engine() {
    let mut rows = input();
    rows.system = vec![gpu_point(0, Some(1.0))];
    rows.process_samples = vec![gpu_sample(0, r"C:\a.exe", 70.0, 100), gpu_sample(0, r"C:\A.exe", 60.0, 200)];
    let hour = &roll_up_hour(&rows).processes[0];
    assert!((hour.gpu_peak - 100.0).abs() < 1e-9, "clamped, got {}", hour.gpu_peak);
    assert!((hour.gpu_avg - 100.0).abs() < 1e-9);
    assert_eq!(hour.gpu_memory_peak, 300, "memory is added up");
}

#[test]
fn a_program_that_never_used_the_gpu_has_zero_gpu_figures() {
    let mut rows = input();
    rows.system = vec![gpu_point(0, Some(1.0))];
    rows.process_samples = vec![sample(0, Some(r"C:\a.exe"), 5.0, 1)];
    let hour = &roll_up_hour(&rows).processes[0];
    assert_eq!((hour.gpu_avg, hour.gpu_peak, hour.gpu_memory_peak), (0.0, 0.0, 0));
}
