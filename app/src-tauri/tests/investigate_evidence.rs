//! The facts the AI help reads, gathered through the same views the screens use (docs/PHASE4-PLAN.md, slice L4b).

use syntrac_app_lib::investigate_evidence::{finding_evidence, program_evidence, program_path_of_subject};
use syntrac_core::investigate::{build_prompt, PromptConfig, PromptLevel};
use syntrac_core::{
    roll_up_due, EventRepository, Finding, FindingStatus, MetricsRepository, ProcessRepository, RollupSettings, Tier,
    Timestamp,
};
use syntrac_testkit::fixtures::{
    at_secs, metrics_sample, net_flow, process_metric, running_process, EventFactory, BEACON_TARGET,
};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200 / 3600 * 3600;
const PROGRAM: &str = r"C:\Users\alice\AppData\Local\Fabrikam\widgetsync.exe";

fn secs(hour: i64, minutes: i64) -> i64 {
    BASE + hour * HOUR + minutes * 60
}

fn now() -> Timestamp {
    at_secs(BASE + 20 * HOUR)
}

/// One program with two CPU samples (10% and 20%) and one flow to a documentation address, rolled up.
fn repo() -> InMemoryRepository {
    let mut repo = InMemoryRepository::default();
    let mut record = running_process(10, at_secs(secs(0, 0)));
    record.image_path = PROGRAM.to_owned();
    repo.record_start(record.clone()).unwrap();
    for (minute, cpu) in [(10, 10.0), (20, 20.0)] {
        repo.append_metrics(&metrics_sample(
            secs(0, minute),
            30.0,
            vec![process_metric(10, Some(record.id), cpu)],
            0.0,
            0,
        ))
        .unwrap();
    }
    let mut factory = EventFactory::default();
    let mut flow = factory.make(at_secs(secs(0, 15)), net_flow(10, BEACON_TARGET, 100, 900, secs(0, 15)));
    flow.process_instance_id = Some(record.id);
    repo.append(&[flow]).unwrap();
    let settings = RollupSettings { settle: std::time::Duration::from_secs(300), max_hours_per_pass: 10 };
    roll_up_due(&mut repo, at_secs(secs(5, 0)), &settings).unwrap();
    repo
}

fn finding(subject: &str) -> Finding {
    Finding {
        id: 1,
        kind: "usage".into(),
        subject: subject.into(),
        subject_label: "widgetsync.exe".into(),
        tier: Tier::Unusual,
        title: "widgetsync.exe used more CPU than usual".into(),
        why: "It used 20% for an hour".into(),
        evidence: vec![format!("Folder: {PROGRAM}")],
        dedupe_key: "k".into(),
        level: Some(20.0),
        status: FindingStatus::Open,
        first_at: at_secs(secs(0, 20)),
        last_at: at_secs(secs(0, 20)),
        hits: 2,
        decided_at: None,
        snoozed_until: None,
    }
}

#[test]
fn a_subject_names_a_program_only_when_it_starts_with_a_file_path() {
    assert_eq!(program_path_of_subject(r"C:\Apps\x.exe"), Some(r"C:\Apps\x.exe"));
    assert_eq!(program_path_of_subject(r"C:\Apps\x.exe|cpu"), Some(r"C:\Apps\x.exe"));
    assert_eq!(program_path_of_subject("203.0.113.9"), None);
    assert_eq!(program_path_of_subject("owner:AS64500"), None);
    assert_eq!(program_path_of_subject(r"C:\"), None);
    assert_eq!(program_path_of_subject(""), None);
}

#[test]
fn a_programs_facts_come_from_its_history_and_its_destinations() {
    let evidence = program_evidence(&repo(), now(), PROGRAM, Some("alice".into())).unwrap();
    let program = evidence.program.expect("a program");

    assert_eq!(program.file_name, "widgetsync.exe");
    assert_eq!(program.folder, r"C:\Users\alice\AppData\Local\Fabrikam");
    let history = program.history.expect("history");
    assert_eq!(history.hours_present, 1);
    assert!(history.hours_covered >= 1);
    assert!((history.cpu_peak_percent - 20.0).abs() < 1e-6, "{history:?}");
    assert_eq!(program.destinations_total, 1);
    assert_eq!(program.destinations[0].addr, BEACON_TARGET.to_string());
    assert_eq!((program.destinations[0].bytes_sent, program.destinations[0].bytes_received), (100, 900));
    assert!(program.destinations[0].internet);
    assert_eq!(program.standing.as_deref(), Some("not on your known list"));
}

#[test]
fn a_program_that_was_never_recorded_still_gives_facts_with_nothing_invented() {
    let evidence = program_evidence(&repo(), now(), r"C:\Nowhere\ghost.exe", None).unwrap();
    let program = evidence.program.unwrap();
    assert_eq!(program.file_name, "ghost.exe");
    assert_eq!(program.destinations_total, 0);
    assert!(program.destinations.is_empty());
    assert_eq!(program.first_seen_days_ago, None);
}

#[test]
fn a_finding_brings_its_program_along_only_when_the_subject_names_one() {
    let with_program = finding_evidence(&repo(), now(), &finding(&format!("{PROGRAM}|cpu")), None).unwrap();
    assert!(with_program.program.is_some());
    assert_eq!(with_program.finding.as_ref().unwrap().tier, "unusual");

    let without = finding_evidence(&repo(), now(), &finding("203.0.113.9"), None).unwrap();
    assert!(without.program.is_none() && without.finding.is_some());
}

#[test]
fn what_is_gathered_is_cut_down_by_the_level_before_it_leaves() {
    let evidence = finding_evidence(&repo(), now(), &finding(&format!("{PROGRAM}|cpu")), Some("alice".into())).unwrap();
    let config = PromptConfig::builtin();
    let summary = build_prompt(&evidence, PromptLevel::Summary, &config).user;
    let external = build_prompt(&evidence, PromptLevel::External, &config).user;

    assert!(summary.contains("widgetsync.exe") && summary.contains("<user>"));
    assert!(!summary.to_lowercase().contains("alice") && !summary.contains(&BEACON_TARGET.to_string()));
    for hidden in ["widgetsync", "alice", "Fabrikam", &BEACON_TARGET.to_string()] {
        assert!(!external.contains(hidden), "the outside copy contains {hidden:?}:\n{external}");
    }
}

/// 2026-11-21 10:37 UTC, the moment the demo tests use.
const DEMO_NOW: i64 = 1_795_257_420;

#[test]
fn every_finding_of_the_demo_machine_gives_a_prompt_that_keeps_its_promises() {
    use syntrac_core::{investigate::scrub_addresses, FindingRepository};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.db");
    syntrac_demo::generate(&path, DEMO_NOW).unwrap();
    let store = syntrac_store::SqliteStore::open(&path).unwrap();
    let findings = store.findings(None, 500).unwrap();
    assert!(findings.len() >= 4, "the demo has findings of several kinds");
    let config = PromptConfig::builtin();

    for finding in &findings {
        let evidence = finding_evidence(&store, at_secs(DEMO_NOW), finding, Some("demo".into())).unwrap();
        for level in [PromptLevel::Summary, PromptLevel::External] {
            let text = build_prompt(&evidence, level, &config).user;
            if std::env::var_os("SYNTRAC_SHOW_PROMPT").is_some() && finding.kind.starts_with("pattern") {
                println!(
                    "=== {} {level:?}
{text}",
                    finding.kind
                );
            }
            assert!(text.len() < 16_000, "{} {level:?}: {} bytes", finding.kind, text.len());
            assert_eq!(scrub_addresses(&text), text, "{} {level:?} holds an address:\n{text}", finding.kind);
            assert!(!text.to_lowercase().contains("demo"), "{} {level:?} holds the user name:\n{text}", finding.kind);
            if level == PromptLevel::External {
                assert!(!text.contains(".example"), "{}: a name in the outside copy:\n{text}", finding.kind);
                assert!(!text.contains(".exe"), "{}: a file name in the outside copy:\n{text}", finding.kind);
            }
        }
    }
}

#[test]
fn the_page_names_a_finding_by_number_or_a_program_by_path_and_an_unknown_number_is_reported() {
    use syntrac_app_lib::ai_investigate::SubjectRef;
    use syntrac_app_lib::investigate_evidence::evidence_for_subject;
    use syntrac_core::FindingRepository;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.db");
    syntrac_demo::generate(&path, DEMO_NOW).unwrap();
    let store = syntrac_store::SqliteStore::open(&path).unwrap();
    let findings = store.findings(None, 500).unwrap();
    let with_program = findings.iter().find(|f| f.kind.starts_with("pattern")).expect("a pattern finding");

    let by_number =
        evidence_for_subject(&store, at_secs(DEMO_NOW), &SubjectRef::Finding { id: with_program.id }, None).unwrap();
    assert_eq!(by_number.finding.as_ref().unwrap().kind, with_program.kind);
    assert!(by_number.program.is_some(), "the program the finding names comes along");

    let program_path =
        by_number.program.as_ref().map(|_| with_program.subject.split('|').next().unwrap().to_owned()).unwrap();
    let by_path =
        evidence_for_subject(&store, at_secs(DEMO_NOW), &SubjectRef::Program { path: program_path }, None).unwrap();
    assert!(by_path.finding.is_none() && by_path.program.is_some());

    let missing = evidence_for_subject(&store, at_secs(DEMO_NOW), &SubjectRef::Finding { id: 987_654 }, None);
    assert_eq!(missing.unwrap_err(), "The finding no longer exists.");
}
