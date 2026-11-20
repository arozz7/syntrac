//! GPU figures from the Windows performance counters (docs/ALERTS-PLAN.md, slice G1). Pure: the counter rows are plain text and numbers.

use syntrac_core::{gpu_figures, parse_engine_instance, parse_memory_instance, CounterRow, GpuReading};

fn engine(name: &str, percent: f64) -> CounterRow {
    CounterRow { instance: name.to_owned(), value: percent }
}

fn memory(name: &str, bytes: f64) -> CounterRow {
    CounterRow { instance: name.to_owned(), value: bytes }
}

const ADAPTER_A: &str = "luid_0x00000000_0x0000E5A5";
const ADAPTER_B: &str = "luid_0x00000000_0x0001C0DE";

fn engine_name(pid: u32, adapter: &str, phys: u32, eng: u32, kind: &str) -> String {
    format!("pid_{pid}_{adapter}_phys_{phys}_eng_{eng}_engtype_{kind}")
}

#[test]
fn an_engine_instance_name_gives_the_process_and_the_engine() {
    let parsed = parse_engine_instance(&engine_name(4242, ADAPTER_A, 0, 3, "Compute_0")).expect("parses");
    assert_eq!(parsed.pid, 4242);
    // The same engine of the same adapter gets the same key whatever the process.
    let other = parse_engine_instance(&engine_name(7, ADAPTER_A, 0, 3, "Compute_0")).expect("parses");
    assert_eq!(parsed.engine, other.engine);
    // A different engine number, adapter or physical GPU is a different engine.
    for different in [
        engine_name(4242, ADAPTER_A, 0, 4, "Compute_0"),
        engine_name(4242, ADAPTER_B, 0, 3, "Compute_0"),
        engine_name(4242, ADAPTER_A, 1, 3, "Compute_0"),
    ] {
        assert_ne!(parsed.engine, parse_engine_instance(&different).unwrap().engine, "{different}");
    }
}

#[test]
fn a_duplicate_instance_suffix_is_the_same_engine() {
    // The counter service numbers instances that would have the same name ("#1", "#2").
    let plain = parse_engine_instance(&engine_name(9, ADAPTER_A, 0, 0, "3D")).unwrap();
    let numbered = parse_engine_instance(&format!("{}#2", engine_name(9, ADAPTER_A, 0, 0, "3D"))).unwrap();
    assert_eq!(plain, numbered);
}

#[test]
fn a_name_that_does_not_parse_is_refused() {
    for name in ["", "_Total", "pid_x_luid_0x0_0x1_phys_0_eng_0_engtype_3D", "pid_5", "luid_0x0_0x1_phys_0", "total"] {
        assert_eq!(parse_engine_instance(name), None, "{name:?}");
    }
    assert_eq!(parse_memory_instance("pid_5"), None);
    assert_eq!(parse_memory_instance("nonsense"), None);
}

#[test]
fn a_memory_instance_name_gives_the_process() {
    let parsed = parse_memory_instance(&format!("pid_321_{ADAPTER_A}_phys_0")).expect("parses");
    assert_eq!(parsed, 321);
    assert_eq!(parse_memory_instance(&format!("pid_321_{ADAPTER_A}_phys_0#1")), Some(321));
}

#[test]
fn a_process_is_as_busy_as_its_busiest_engine_not_the_sum_of_engines() {
    let reading = GpuReading {
        engines: vec![
            engine(&engine_name(10, ADAPTER_A, 0, 0, "3D"), 30.0),
            engine(&engine_name(10, ADAPTER_A, 0, 1, "Copy"), 20.0),
            engine(&engine_name(10, ADAPTER_A, 0, 2, "Compute_0"), 45.0),
        ],
        memory: Vec::new(),
    };
    let figures = gpu_figures(&reading);
    assert!((figures.by_pid[&10].percent - 45.0).abs() < 1e-4);
}

#[test]
fn the_same_engine_listed_twice_is_added_up() {
    let reading = GpuReading {
        engines: vec![
            engine(&engine_name(10, ADAPTER_A, 0, 0, "3D"), 20.0),
            engine(&format!("{}#1", engine_name(10, ADAPTER_A, 0, 0, "3D")), 15.0),
        ],
        memory: Vec::new(),
    };
    assert!((gpu_figures(&reading).by_pid[&10].percent - 35.0).abs() < 1e-4);
}

#[test]
fn the_busiest_engine_is_taken_over_all_adapters() {
    let reading = GpuReading {
        engines: vec![
            engine(&engine_name(10, ADAPTER_A, 0, 0, "3D"), 12.0),
            engine(&engine_name(10, ADAPTER_B, 0, 0, "3D"), 64.0),
        ],
        memory: Vec::new(),
    };
    assert!((gpu_figures(&reading).by_pid[&10].percent - 64.0).abs() < 1e-4);
}

#[test]
fn memory_is_added_up_over_adapters_and_keeps_processes_apart() {
    let reading = GpuReading {
        engines: Vec::new(),
        memory: vec![
            memory(&format!("pid_10_{ADAPTER_A}_phys_0"), 1_000.0),
            memory(&format!("pid_10_{ADAPTER_B}_phys_0"), 2_500.0),
            memory(&format!("pid_11_{ADAPTER_A}_phys_0"), 7.0),
        ],
    };
    let figures = gpu_figures(&reading);
    assert_eq!(figures.by_pid[&10].memory_bytes, 3_500);
    assert_eq!(figures.by_pid[&11].memory_bytes, 7);
    assert_eq!(figures.by_pid[&10].percent, 0.0, "a process with memory but no engine reading is idle");
}

#[test]
fn the_system_figure_is_the_busiest_engine_over_all_processes() {
    let reading = GpuReading {
        engines: vec![
            engine(&engine_name(10, ADAPTER_A, 0, 0, "3D"), 30.0),
            engine(&engine_name(11, ADAPTER_A, 0, 0, "3D"), 25.0),
            engine(&engine_name(12, ADAPTER_B, 0, 0, "3D"), 40.0),
        ],
        memory: Vec::new(),
    };
    // Two processes share one engine (55%), another is alone on a second adapter (40%): the busiest engine is 55%.
    assert!((gpu_figures(&reading).busiest_percent - 55.0).abs() < 1e-4);
}

#[test]
fn values_are_kept_inside_their_range() {
    let reading = GpuReading {
        engines: vec![
            engine(&engine_name(10, ADAPTER_A, 0, 0, "3D"), 180.0),
            engine(&engine_name(11, ADAPTER_A, 0, 1, "3D"), -3.0),
            engine(&engine_name(12, ADAPTER_A, 0, 2, "3D"), f64::NAN),
        ],
        memory: vec![memory(&format!("pid_10_{ADAPTER_A}_phys_0"), -5.0)],
    };
    let figures = gpu_figures(&reading);
    assert_eq!(figures.by_pid[&10].percent, 100.0);
    assert_eq!(figures.by_pid[&11].percent, 0.0);
    assert_eq!(figures.by_pid[&12].percent, 0.0);
    assert_eq!(figures.by_pid[&10].memory_bytes, 0);
    assert!(figures.busiest_percent <= 100.0);
}

#[test]
fn an_unparsable_row_is_skipped_and_does_not_hide_the_rest() {
    let reading = GpuReading {
        engines: vec![engine("garbage", 99.0), engine(&engine_name(10, ADAPTER_A, 0, 0, "3D"), 5.0)],
        memory: vec![memory("garbage", 1.0)],
    };
    let figures = gpu_figures(&reading);
    assert_eq!(figures.by_pid.len(), 1);
    assert!((figures.by_pid[&10].percent - 5.0).abs() < 1e-4);
}

#[test]
fn an_empty_reading_gives_no_processes_and_an_idle_gpu() {
    let figures = gpu_figures(&GpuReading::default());
    assert!(figures.by_pid.is_empty());
    assert_eq!(figures.busiest_percent, 0.0);
}
