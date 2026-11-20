//! The ranking maths behind the trend views, as pure functions: merging what the store sums up, then ranking.

use syntrac_core::trend::{merge_programs, rank_programs, ProcessTotals, ProgramMetric, TrafficTotals};
use syntrac_core::Executable;

fn exe(name: &str) -> Option<Executable> {
    Some(Executable { key: format!(r"c:\apps\{}", name.to_lowercase()), path: format!(r"C:\Apps\{name}") })
}

fn process(name: &str, cpu_weighted: f64, memory_peak: u64, starts: u64) -> ProcessTotals {
    ProcessTotals {
        exe: exe(name),
        hours_present: 3,
        samples_present: 200,
        cpu_weighted,
        cpu_peak: cpu_weighted / 100.0 * 2.0,
        memory_peak,
        gpu_peak: 0.0,
        gpu_memory_peak: 0,
        starts,
    }
}

fn traffic(name: &str, sent: u64, received: u64) -> TrafficTotals {
    TrafficTotals { exe: exe(name), bytes_sent: sent, bytes_received: received, flows: 4, destinations: 2 }
}

fn names(programs: &[syntrac_core::trend::ProgramSummary]) -> Vec<String> {
    programs.iter().map(|p| p.exe.as_ref().map_or("unknown".to_owned(), |e| e.path.clone())).collect()
}

#[test]
fn cpu_is_averaged_over_every_sample_of_the_range_not_just_those_where_the_program_ran() {
    // 1000 samples in the range; the program's hours add up to 5000 percent-samples: 5% of the machine on average.
    let merged = merge_programs(1000, vec![process("a.exe", 5000.0, 10, 0)], vec![]);
    assert!((merged[0].cpu_avg_percent - 5.0).abs() < 1e-9);
}

#[test]
fn with_no_samples_at_all_the_average_is_zero_not_a_division_by_zero() {
    let merged = merge_programs(0, vec![process("a.exe", 0.0, 10, 0)], vec![]);
    assert_eq!(merged[0].cpu_avg_percent, 0.0);
}

#[test]
fn a_program_seen_only_in_traffic_or_only_in_processes_is_still_listed_once() {
    let merged = merge_programs(
        100,
        vec![process("proc-only.exe", 100.0, 1, 0), process("both.exe", 100.0, 1, 0)],
        vec![traffic("both.exe", 10, 20), traffic("net-only.exe", 5, 5)],
    );
    let mut listed = names(&merged);
    listed.sort();
    assert_eq!(listed, vec![r"C:\Apps\both.exe", r"C:\Apps\net-only.exe", r"C:\Apps\proc-only.exe"]);
    let both = merged.iter().find(|p| p.exe.as_ref().is_some_and(|e| e.path.ends_with("both.exe"))).unwrap();
    assert_eq!((both.bytes_sent, both.bytes_received), (10, 20));
    assert_eq!(both.memory_peak_bytes, 1);
}

#[test]
fn the_unattributed_bucket_merges_with_itself_and_stays_distinct_from_named_programs() {
    let unknown_process = ProcessTotals { exe: None, ..process("x", 100.0, 1, 0) };
    let unknown_traffic = TrafficTotals { exe: None, ..traffic("x", 7, 8) };
    let merged = merge_programs(100, vec![unknown_process, process("a.exe", 100.0, 1, 0)], vec![unknown_traffic]);
    assert_eq!(merged.len(), 2);
    let unknown = merged.iter().find(|p| p.exe.is_none()).unwrap();
    assert_eq!(unknown.bytes_sent + unknown.bytes_received, 15);
}

#[test]
fn programs_rank_highest_first_by_the_chosen_metric() {
    let merged = merge_programs(
        100,
        vec![process("low.exe", 100.0, 900, 1), process("high.exe", 900.0, 100, 5), process("mid.exe", 500.0, 500, 3)],
        vec![traffic("low.exe", 1_000, 0), traffic("mid.exe", 10, 10)],
    );
    assert_eq!(
        names(&rank_programs(merged.clone(), ProgramMetric::CpuAverage, 10)),
        [r"C:\Apps\high.exe", r"C:\Apps\mid.exe", r"C:\Apps\low.exe"]
    );
    assert_eq!(names(&rank_programs(merged.clone(), ProgramMetric::MemoryPeak, 10))[0], r"C:\Apps\low.exe");
    assert_eq!(names(&rank_programs(merged.clone(), ProgramMetric::Starts, 10))[0], r"C:\Apps\high.exe");
    assert_eq!(names(&rank_programs(merged, ProgramMetric::NetworkBytes, 10))[0], r"C:\Apps\low.exe");
}

#[test]
fn a_program_with_nothing_to_show_for_the_metric_is_left_out_of_that_ranking() {
    let merged = merge_programs(
        100,
        vec![process("quiet.exe", 100.0, 1, 0), process("busy.exe", 100.0, 1, 0)],
        vec![traffic("busy.exe", 5, 5)],
    );
    assert_eq!(names(&rank_programs(merged.clone(), ProgramMetric::NetworkBytes, 10)), [r"C:\Apps\busy.exe"]);
    assert_eq!(names(&rank_programs(merged, ProgramMetric::Starts, 10)).len(), 0);
}

#[test]
fn the_limit_is_applied_after_ranking() {
    let merged = merge_programs(
        100,
        (0..20).map(|i| process(&format!("p{i:02}.exe"), 100.0 + f64::from(i), 1, 0)).collect(),
        vec![],
    );
    let top = rank_programs(merged, ProgramMetric::CpuAverage, 3);
    assert_eq!(names(&top), [r"C:\Apps\p19.exe", r"C:\Apps\p18.exe", r"C:\Apps\p17.exe"]);
}

#[test]
fn ties_are_broken_by_path_with_the_unattributed_bucket_last() {
    let unknown = ProcessTotals { exe: None, ..process("x", 100.0, 1, 0) };
    let merged =
        merge_programs(100, vec![process("b.exe", 100.0, 1, 0), unknown, process("a.exe", 100.0, 1, 0)], vec![]);
    let ranked = rank_programs(merged, ProgramMetric::CpuAverage, 10);
    assert_eq!(names(&ranked), [r"C:\Apps\a.exe", r"C:\Apps\b.exe", "unknown"]);
}
