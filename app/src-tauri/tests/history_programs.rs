//! The Programs history: a ranking of programs over a range, and one program's history hour by hour.

use syntrac_app_lib::backend::{Backend, Data};
use syntrac_app_lib::history_programs::{
    program_history, program_ranking, Grouping, ProgramMetricParam, MAX_PROGRAM_ROWS, UNKNOWN_PROGRAM,
};
use syntrac_app_lib::settings::AppSettings;
use syntrac_core::{roll_up_due, EventRepository, MetricsRepository, ProcessRepository, RollupSettings, Timestamp};
use syntrac_store::SqliteStore;
use syntrac_testkit::fixtures::{
    at_secs, metrics_sample, net_flow, process_metric, running_process, EventFactory, BEACON_TARGET,
};
use syntrac_testkit::{InMemoryRepository, ManualClock};

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const CHROME: &str = r"C:\Program Files\Google\Chrome\chrome.exe";
const CODE: &str = r"C:\Apps\code.exe";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

fn start<R: ProcessRepository>(repo: &mut R, pid: u32, started: i64, path: &str) -> syntrac_core::ProcessRecord {
    let mut record = running_process(pid, at_secs(started));
    record.image_path = path.to_owned();
    repo.record_start(record.clone()).unwrap();
    record
}

/// Chrome: 10% and 20% in the two samples of hour 0, absent in hour 1's single sample. Code: 4% in hour 1. Some traffic too.
fn scenario<R: ProcessRepository + MetricsRepository + EventRepository + syntrac_core::RollupRepository>(repo: &mut R) {
    let chrome = start(repo, 10, secs(0, 0), CHROME);
    let code = start(repo, 11, secs(0, 0), CODE);
    repo.append_metrics(&metrics_sample(secs(0, 10), 30.0, vec![process_metric(10, Some(chrome.id), 10.0)], 0.0, 0))
        .unwrap();
    repo.append_metrics(&metrics_sample(secs(0, 20), 30.0, vec![process_metric(10, Some(chrome.id), 20.0)], 0.0, 0))
        .unwrap();
    repo.append_metrics(&metrics_sample(secs(1, 10), 10.0, vec![process_metric(11, Some(code.id), 4.0)], 0.0, 0))
        .unwrap();
    let mut f = EventFactory::default();
    let mut flow = f.make(at_secs(secs(0, 15)), net_flow(10, BEACON_TARGET, 100, 900, secs(0, 15)));
    flow.process_instance_id = Some(chrome.id);
    repo.append(&[flow]).unwrap();
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(repo, at_secs(secs(5, 0)), &settings).unwrap();
}

fn ranked(metric: ProgramMetricParam) -> syntrac_app_lib::history_programs::ProgramsView {
    let mut repo = InMemoryRepository::default();
    scenario(&mut repo);
    program_ranking(&repo, now(), 7 * 24 * 3600, metric, 10, Grouping::None).unwrap()
}

#[test]
fn programs_are_ranked_by_the_chosen_metric_with_readable_names() {
    let by_cpu = ranked(ProgramMetricParam::Cpu);
    let names: Vec<_> = by_cpu.programs.iter().map(|p| p.name.as_str()).collect();
    // Chrome: (15% x 2 samples) / 3 samples in the range = 10%; Code: 4 / 3 = 1.33%.
    assert_eq!(names, ["chrome.exe", "code.exe"]);
    assert!((by_cpu.programs[0].cpu_avg_percent - 10.0).abs() < 1e-6);
    assert_eq!(by_cpu.programs[0].path.as_deref(), Some(CHROME));

    let by_network = ranked(ProgramMetricParam::Network);
    assert_eq!(by_network.programs.len(), 1, "only chrome has traffic");
    assert_eq!((by_network.programs[0].bytes_sent, by_network.programs[0].bytes_received), (100, 900));
}

#[test]
fn each_program_says_how_many_hours_it_was_seen_in_and_when_first_and_last() {
    let view = ranked(ProgramMetricParam::Cpu);
    let chrome = &view.programs[0];
    assert_eq!(chrome.hours_present, 1);
    assert_eq!(view.hours_in_range, 2, "two hours have data in the range");
    assert_eq!(chrome.first_seen_ms, Some(secs(0, 10) * 1000));
    assert_eq!(chrome.last_seen_ms, Some(secs(0, 20) * 1000));
}

#[test]
fn the_unattributed_bucket_is_called_unknown_program_and_has_no_path() {
    let mut repo = InMemoryRepository::default();
    repo.append_metrics(&metrics_sample(secs(0, 10), 5.0, vec![process_metric(99, None, 3.0)], 0.0, 0)).unwrap();
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();

    let view = program_ranking(&repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 10, Grouping::None).unwrap();

    assert_eq!(view.programs[0].name, UNKNOWN_PROGRAM);
    assert_eq!(view.programs[0].path, None);
}

#[test]
fn the_number_of_rows_is_limited() {
    let view = {
        let mut repo = InMemoryRepository::default();
        scenario(&mut repo);
        program_ranking(&repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, 1, Grouping::None).unwrap()
    };
    assert_eq!(view.programs.len(), 1);
    let huge = {
        let mut repo = InMemoryRepository::default();
        scenario(&mut repo);
        program_ranking(&repo, now(), 7 * 24 * 3600, ProgramMetricParam::Cpu, usize::MAX, Grouping::None).unwrap()
    };
    assert!(huge.programs.len() <= MAX_PROGRAM_ROWS);
}

#[test]
fn the_metric_arrives_as_a_plain_word() {
    let parsed: ProgramMetricParam = serde_json::from_str("\"memory\"").unwrap();
    assert_eq!(parsed, ProgramMetricParam::Memory);
    assert!(serde_json::from_str::<ProgramMetricParam>("\"nonsense\"").is_err());
}

#[test]
fn one_programs_history_has_every_hour_with_data_and_zero_where_it_was_absent() {
    let mut repo = InMemoryRepository::default();
    scenario(&mut repo);

    let view = program_history(&repo, now(), 7 * 24 * 3600, r"c:\PROGRAM FILES\google\chrome\CHROME.EXE").unwrap();

    assert_eq!(view.name, "chrome.exe");
    assert_eq!(view.hours.len(), 2, "one row per hour that has system data, not per hour the program was present");
    let (first, second) = (&view.hours[0], &view.hours[1]);
    assert!(first.present && !second.present);
    assert!((first.cpu_avg_percent - 15.0).abs() < 1e-6);
    assert_eq!(first.samples, 2);
    assert_eq!((second.cpu_avg_percent, second.cpu_peak_percent, second.memory_peak_bytes), (0.0, 0.0, 0));
    assert_eq!(second.samples, 1, "the weight is the hour's samples, so averaging days stays honest");
    assert_eq!(view.first_seen_ms, Some(secs(0, 10) * 1000));
}

#[test]
fn an_unknown_path_has_an_empty_history_not_an_error() {
    let mut repo = InMemoryRepository::default();
    scenario(&mut repo);
    let view = program_history(&repo, now(), 7 * 24 * 3600, r"C:\nowhere\ghost.exe").unwrap();
    assert!(view.hours.iter().all(|h| !h.present));
    assert_eq!(view.first_seen_ms, None);
    assert_eq!(view.name, "ghost.exe");
}

#[test]
fn the_backend_serves_programs_from_a_real_database_and_reports_missing_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    {
        let mut store = SqliteStore::open(&path).unwrap();
        scenario(&mut store);
    }
    let settings = |db: &std::path::Path| {
        AppSettings::for_database(db.to_path_buf(), &format!("syntrac-programs-test-{}", std::process::id()))
    };
    let backend = Backend::new(settings(&path), ManualClock::new(now()));

    match backend.top_programs(7 * 24 * 3600, ProgramMetricParam::Cpu, Some(10), Grouping::None) {
        Data::Ok { data } => assert_eq!(data.programs[0].name, "chrome.exe"),
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    }
    match backend.program_history(7 * 24 * 3600, CHROME.to_owned()) {
        Data::Ok { data } => assert_eq!(data.hours.len(), 2),
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    }

    let missing = Backend::new(settings(&dir.path().join("nope.db")), ManualClock::new(now()));
    assert!(matches!(
        missing.top_programs(3600, ProgramMetricParam::Cpu, None, Grouping::None),
        Data::NoDatabase { .. }
    ));
}

#[test]
fn a_newly_rolled_up_hour_shows_up_even_though_the_sums_of_a_range_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("syntrac.db");
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    {
        let mut store = SqliteStore::open(&path).unwrap();
        scenario(&mut store);
    }
    let backend = Backend::new(
        AppSettings::for_database(path.clone(), &format!("syntrac-cache-test-{}", std::process::id())),
        ManualClock::new(now()),
    );
    let names = |backend: &Backend<ManualClock>, metric| match backend.top_programs(
        7 * 24 * 3600,
        metric,
        Some(10),
        Grouping::None,
    ) {
        Data::Ok { data } => data.programs.into_iter().map(|p| p.name).collect::<Vec<_>>(),
        other => panic!("unexpected {}", serde_json::to_string(&other).unwrap()),
    };
    assert_eq!(names(&backend, ProgramMetricParam::Cpu), ["chrome.exe", "code.exe"]);
    assert_eq!(names(&backend, ProgramMetricParam::Network), ["chrome.exe"], "another metric from the same sums");

    // The collector rolls up three more hours (5 to 7), and a new program is in one of them.
    {
        let mut store = SqliteStore::open(&path).unwrap();
        let late = start(&mut store, 12, secs(6, 0), r"C:\Apps\late.exe");
        store
            .append_metrics(&metrics_sample(secs(6, 10), 90.0, vec![process_metric(12, Some(late.id), 80.0)], 0.0, 0))
            .unwrap();
        roll_up_due(&mut store, at_secs(secs(8, 0)), &settings).unwrap();
    }

    assert_eq!(names(&backend, ProgramMetricParam::Cpu)[0], "late.exe", "the new hour is not hidden by kept sums");
}
