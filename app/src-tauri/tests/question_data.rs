//! The facts behind the question box's tools, read through the same views the screens use (docs/PHASE5-PLAN.md, B3).

use syntrac_app_lib::history_programs::ProgramMetricParam;
use syntrac_app_lib::question_data::{open_findings, program_family, ranked_programs, recorded_program};
use syntrac_core::{
    roll_up_due, Candidate, EventRepository, FindingRepository, FindingWrite, MetricsRepository, ProcessRecord,
    ProcessRepository, RollupSettings, Tier, Timestamp,
};
use syntrac_testkit::fixtures::{
    at_secs, metrics_sample, net_flow, process_metric, running_process, EventFactory, BEACON_TARGET,
};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const PROGRAM: &str = r"C:\Users\alice\AppData\Local\Fabrikam\widgetsync.exe";
const SHELL: &str = r"C:\Windows\explorer.exe";
const HELPER: &str = r"C:\Apps\helper.exe";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

fn start(
    repo: &mut InMemoryRepository,
    pid: u32,
    path: &str,
    started: i64,
    parent: Option<&ProcessRecord>,
) -> ProcessRecord {
    let mut record = running_process(pid, at_secs(started));
    record.image_path = path.to_owned();
    if let Some(parent) = parent {
        record.parent_pid = parent.pid;
        record.parent_id = Some(parent.id);
    }
    repo.record_start(record.clone()).unwrap();
    record
}

/// widgetsync.exe, started by explorer.exe, which started two helpers; it used 10% and 20% CPU and talked to one address.
fn repo() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let shell = start(&mut repo, 5, SHELL, secs(0, 0), None);
    let mut widget = running_process(10, at_secs(secs(0, 1)));
    widget.image_path = PROGRAM.to_owned();
    widget.parent_pid = shell.pid;
    widget.parent_id = Some(shell.id);
    widget.command_line = Some("widgetsync.exe --profile one".into());
    repo.record_start(widget.clone()).unwrap();
    start(&mut repo, 11, HELPER, secs(0, 2), Some(&widget));
    start(&mut repo, 12, HELPER, secs(0, 3), Some(&widget));
    for (minute, cpu) in [(10, 10.0), (20, 20.0)] {
        repo.append_metrics(&metrics_sample(
            secs(0, minute),
            30.0,
            vec![process_metric(10, Some(widget.id), cpu)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let mut factory = EventFactory::default();
    let mut flow = factory.make(at_secs(secs(0, 15)), net_flow(10, BEACON_TARGET, 100, 900, secs(0, 15)));
    flow.process_instance_id = Some(widget.id);
    repo.append(&[flow]).unwrap();
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();
    repo
}

fn candidate(n: i64) -> Candidate {
    Candidate {
        kind: "new_program".into(),
        subject: format!("subject {n}"),
        subject_label: format!("label {n}"),
        tier: Tier::Unusual,
        title: format!("Finding {n}"),
        why: "It appeared.".into(),
        evidence: vec![],
        dedupe_key: format!("key {n}"),
        level: None,
        at: at_secs(secs(1, n)),
    }
}

#[test]
fn a_recorded_program_has_evidence() {
    let evidence = recorded_program(&repo(), now(), PROGRAM, Some("alice".into())).unwrap().expect("recorded");
    assert_eq!(evidence.program.unwrap().file_name, "widgetsync.exe");
}

#[test]
fn the_path_is_matched_without_regard_to_case() {
    assert!(recorded_program(&repo(), now(), &PROGRAM.to_uppercase(), None).unwrap().is_some());
}

#[test]
fn a_path_the_store_never_recorded_has_no_evidence() {
    assert!(recorded_program(&repo(), now(), r"C:\Nowhere\ghost.exe", None).unwrap().is_none());
}

#[test]
fn findings_are_the_open_ones_newest_first_up_to_the_limit() {
    let mut repo = InMemoryRepository::default();
    let writes: Vec<FindingWrite> = (1..=4).map(|n| FindingWrite::Insert(candidate(n))).collect();
    repo.apply_writes(&writes).unwrap();

    let facts = open_findings(&repo, 2).unwrap();

    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].title, "Finding 4");
    assert_eq!(facts[0].tier, "unusual");
}

#[test]
fn the_ranking_gives_programs_with_paths_and_leaves_the_unknown_bucket_out() {
    let rows = ranked_programs(&repo(), now(), ProgramMetricParam::Cpu, 7, 10).unwrap();

    assert_eq!(rows[0].name, "widgetsync.exe");
    assert_eq!(rows[0].path, PROGRAM);
    assert!(rows[0].cpu_avg_percent > 0.0);
    assert!(rows.iter().all(|row| !row.path.is_empty()));
}

#[test]
fn a_programs_family_counts_who_started_it_and_what_it_started() {
    let family = program_family(&repo(), now(), PROGRAM).unwrap().expect("seen running");

    assert_eq!(family.instances, 1);
    assert_eq!(family.started_by.iter().map(|n| (n.name.as_str(), n.count)).collect::<Vec<_>>(), [("explorer.exe", 1)]);
    assert_eq!(family.started.iter().map(|n| (n.name.as_str(), n.count)).collect::<Vec<_>>(), [("helper.exe", 2)]);
    assert_eq!(family.command_lines, ["widgetsync.exe --profile one"]);
}

#[test]
fn a_program_never_seen_running_has_no_family() {
    assert!(program_family(&repo(), now(), r"C:\Nowhere\ghost.exe").unwrap().is_none());
}
