//! Phase 5, slice B3 (docs/PHASE5-PLAN.md): the read-only tools the question box offers a model. Each renders facts at the person's detail level
//! through the same cleaning as the evidence for Investigate.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use syntrac_app_lib::history_programs::ProgramMetricParam;
use syntrac_app_lib::question_tools::{
    question_registry, CountedName, ProgramFamily, QuestionData, TopProgram, TOOL_NAMES,
};
use syntrac_core::investigate::{DestinationFact, Evidence, FindingFacts, ProgramFacts, PromptLevel};
use syntrac_core::question_config::QuestionConfig;
use syntrac_core::question_loop::{ReadOnlyTool, ToolError};

#[derive(Default)]
struct FakeData {
    known_program: Mutex<Option<Evidence>>,
    findings: Vec<FindingFacts>,
    top: Vec<TopProgram>,
    family: Option<ProgramFamily>,
    fail: bool,
    asked: Mutex<Vec<String>>,
}

impl FakeData {
    fn note(&self, what: String) {
        self.asked.lock().unwrap().push(what);
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl QuestionData for FakeData {
    fn program(&self, path: &str) -> Result<Option<Evidence>, String> {
        self.note(format!("program {path}"));
        if self.fail {
            return Err("the store is busy".into());
        }
        Ok(self.known_program.lock().unwrap().clone())
    }

    fn findings(&self, limit: usize) -> Result<Vec<FindingFacts>, String> {
        self.note(format!("findings {limit}"));
        Ok(self.findings.iter().take(limit).cloned().collect())
    }

    fn top_programs(&self, metric: ProgramMetricParam, days: u64, limit: usize) -> Result<Vec<TopProgram>, String> {
        self.note(format!("top {metric:?} {days} {limit}"));
        Ok(self.top.iter().take(limit).cloned().collect())
    }

    fn family(&self, path: &str) -> Result<Option<ProgramFamily>, String> {
        self.note(format!("family {path}"));
        Ok(self.family.clone())
    }
}

fn program() -> ProgramFacts {
    ProgramFacts {
        file_name: "widgetsync.exe".into(),
        folder: r"C:\Users\alice\AppData\Local\Widget".into(),
        location: "user_profile".into(),
        sha256: Some("ab".repeat(32)),
        destinations: vec![DestinationFact {
            addr: "203.0.113.7".into(),
            internet: true,
            bytes_sent: 10,
            bytes_received: 20,
            ..DestinationFact::default()
        }],
        destinations_total: 1,
        ..ProgramFacts::default()
    }
}

fn data() -> FakeData {
    FakeData {
        known_program: Mutex::new(Some(Evidence {
            program: Some(program()),
            user_name: Some("alice".into()),
            ..Evidence::default()
        })),
        findings: (1..=12)
            .map(|n| FindingFacts {
                kind: "new_program".into(),
                tier: "suspicious".into(),
                title: format!("New program {n}"),
                why: "It appeared today.".into(),
                evidence: vec![r"C:\Users\alice\x.exe".into()],
                hits: 1,
                level: None,
            })
            .collect(),
        top: (1..=12)
            .map(|n| TopProgram {
                name: format!("busy{n}.exe"),
                path: format!(r"C:\Users\alice\busy{n}.exe"),
                location: "user_profile".into(),
                cpu_avg_percent: 12.34,
                memory_peak_mb: 200,
                bytes_sent: 1,
                bytes_received: 2,
                starts: 3,
            })
            .collect(),
        family: Some(ProgramFamily {
            instances: 2,
            started_by: vec![CountedName { name: "explorer.exe".into(), count: 2 }],
            started: vec![CountedName { name: "helper.exe".into(), count: 4 }],
            command_lines: vec![r"widgetsync.exe --token=abc --profile C:\Users\alice\p".into()],
        }),
        ..FakeData::default()
    }
}

fn tool(data: &Arc<FakeData>, level: PromptLevel, name: &str) -> Arc<dyn ReadOnlyTool> {
    let registry = question_registry(
        Arc::clone(data) as Arc<dyn QuestionData>,
        &QuestionConfig::builtin(),
        level,
        Some("alice".into()),
    );
    registry.tool(name).unwrap_or_else(|| panic!("no tool {name}"))
}

fn run(data: &Arc<FakeData>, level: PromptLevel, name: &str, arguments: Value) -> Result<Value, ToolError> {
    tool(data, level, name).run(&arguments)
}

fn bad_arguments(result: Result<Value, ToolError>) -> String {
    match result {
        Err(ToolError::BadArguments(message)) => message,
        other => panic!("expected bad arguments, got {other:?}"),
    }
}

// ---- the set of tools

#[test]
fn exactly_the_four_read_only_tools_are_offered_with_the_wording_from_the_config() {
    let registry = question_registry(
        Arc::new(data()) as Arc<dyn QuestionData>,
        &QuestionConfig::builtin(),
        PromptLevel::Full,
        None,
    );
    let specs = registry.specs();
    let mut names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    names.sort_unstable();
    let mut expected = TOOL_NAMES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert_eq!(names, ["list_findings", "program_facts", "program_family", "top_programs"]);
    let config = QuestionConfig::builtin();
    for spec in &specs {
        assert_eq!(spec.description, config.describe(&spec.name));
        assert_eq!(spec.parameters["type"], "object");
    }
}

// ---- program_facts

#[test]
fn program_facts_needs_a_path_that_is_text() {
    let data = Arc::new(data());
    assert!(bad_arguments(run(&data, PromptLevel::Full, "program_facts", json!({}))).contains("path"));
    assert!(bad_arguments(run(&data, PromptLevel::Full, "program_facts", json!({"path": 5}))).contains("path"));
    assert!(bad_arguments(run(&data, PromptLevel::Full, "program_facts", json!({"path": "  "}))).contains("path"));
    assert!(data.asked().is_empty(), "nothing was looked up for bad arguments");
}

#[test]
fn a_path_the_store_does_not_know_is_refused_and_points_to_the_ranking() {
    let data = Arc::new(FakeData::default());
    let message = bad_arguments(run(&data, PromptLevel::Full, "program_facts", json!({"path": r"C:\nowhere\x.exe"})));
    assert!(message.contains("top_programs"), "{message}");
}

#[test]
fn program_facts_at_the_summary_level_leaves_out_the_hash_and_addresses() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Summary, "program_facts", json!({"path": r"C:\x\widgetsync.exe"})).unwrap();
    assert_eq!(result["detail_level"], "summary");
    assert_eq!(result["program"]["file_name"], "widgetsync.exe");
    let text = result.to_string();
    assert!(!text.contains(&"ab".repeat(32)) && !text.contains("203.0.113.7"), "{text}");
}

#[test]
fn program_facts_at_the_full_level_includes_them_but_never_the_user_name() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Full, "program_facts", json!({"path": r"C:\x\widgetsync.exe"})).unwrap();
    let text = result.to_string();
    assert!(text.contains(&"ab".repeat(32)) && text.contains("203.0.113.7"), "{text}");
    assert!(!text.to_lowercase().contains("alice"), "{text}");
}

#[test]
fn a_failure_of_the_store_is_a_failure_of_the_tool() {
    let data = Arc::new(FakeData { fail: true, ..data() });
    let failed = run(&data, PromptLevel::Full, "program_facts", json!({"path": r"C:\x.exe"}));
    assert_eq!(failed, Err(ToolError::Failed("the store is busy".into())));
}

// ---- list_findings

#[test]
fn list_findings_shows_five_by_default_and_never_more_than_ten() {
    let data = Arc::new(data());
    let by_default = run(&data, PromptLevel::Full, "list_findings", json!({})).unwrap();
    assert_eq!(by_default["findings"].as_array().unwrap().len(), 5);
    let most = run(&data, PromptLevel::Full, "list_findings", json!({"limit": 500})).unwrap();
    assert_eq!(most["findings"].as_array().unwrap().len(), 10);
    run(&data, PromptLevel::Full, "list_findings", json!({"limit": 0})).unwrap();
    assert_eq!(data.asked(), ["findings 5", "findings 10", "findings 1"]);
}

#[test]
fn list_findings_refuses_a_limit_that_is_not_a_number() {
    let data = Arc::new(data());
    assert!(bad_arguments(run(&data, PromptLevel::Full, "list_findings", json!({"limit": "many"}))).contains("limit"));
}

#[test]
fn list_findings_reads_findings_through_the_evidence_cleaning() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Full, "list_findings", json!({"limit": 1})).unwrap();
    let first = &result["findings"][0];
    assert_eq!(first["title"], "New program 1");
    assert_eq!(first["tier"], "suspicious");
    assert!(!result.to_string().to_lowercase().contains("alice"));
}

// ---- top_programs

#[test]
fn top_programs_rejects_a_measure_it_does_not_know() {
    let data = Arc::new(data());
    let message = bad_arguments(run(&data, PromptLevel::Full, "top_programs", json!({"metric": "happiness"})));
    assert!(message.contains("cpu") && message.contains("network"), "{message}");
}

#[test]
fn top_programs_has_defaults_and_keeps_its_numbers_within_bounds() {
    let data = Arc::new(data());
    run(&data, PromptLevel::Full, "top_programs", json!({})).unwrap();
    run(&data, PromptLevel::Full, "top_programs", json!({"metric": "network", "days": 365, "limit": 99})).unwrap();
    run(&data, PromptLevel::Full, "top_programs", json!({"metric": "gpu", "days": 0, "limit": 0})).unwrap();
    assert_eq!(data.asked(), ["top Cpu 7 5", "top Network 14 10", "top Gpu 1 1"]);
}

#[test]
fn top_programs_lists_names_and_paths_with_the_user_name_replaced() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Summary, "top_programs", json!({"limit": 2})).unwrap();
    let programs = result["programs"].as_array().unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(programs[0]["name"], "busy1.exe");
    assert_eq!(programs[0]["cpu_average_percent"], 12.3);
    assert!(programs[0]["path"].as_str().unwrap().contains("<user>"));
    assert!(!result.to_string().to_lowercase().contains("alice"));
}

// ---- program_family

#[test]
fn program_family_hides_command_lines_below_the_full_level() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Summary, "program_family", json!({"path": r"C:\x\widgetsync.exe"})).unwrap();
    assert!(result.get("command_lines").is_none(), "{result}");
    assert!(!result.to_string().contains("token"));
    assert_eq!(result["started_by"][0], json!({"name": "explorer.exe", "count": 2}));
    assert_eq!(result["started"][0], json!({"name": "helper.exe", "count": 4}));
    assert_eq!(result["instances"], 2);
}

#[test]
fn program_family_at_the_full_level_shows_command_lines_without_the_user_name() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Full, "program_family", json!({"path": r"C:\x\widgetsync.exe"})).unwrap();
    let lines = result["command_lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].as_str().unwrap().contains("widgetsync.exe"));
    assert!(!lines[0].as_str().unwrap().to_lowercase().contains("alice"));
}

#[test]
fn program_family_for_a_program_never_seen_running_is_refused() {
    let data = Arc::new(FakeData::default());
    let message = bad_arguments(run(&data, PromptLevel::Full, "program_family", json!({"path": r"C:\x.exe"})));
    assert!(message.contains("top_programs"), "{message}");
}

// ---- secrets in command lines

#[test]
fn values_of_arguments_that_name_a_secret_are_hidden() {
    use syntrac_app_lib::question_tools::hide_secret_arguments;
    assert_eq!(hide_secret_arguments("app.exe --token=abc --name=x"), "app.exe --token=[hidden] --name=x");
    assert_eq!(hide_secret_arguments("app.exe /password:abc /user:bob"), "app.exe /password:[hidden] /user:bob");
    assert_eq!(hide_secret_arguments("app.exe --api-key abc --verbose"), "app.exe --api-key [hidden] --verbose");
    assert_eq!(hide_secret_arguments(r"app.exe C:\data\x.txt"), r"app.exe C:\data\x.txt");
}

#[test]
fn the_full_level_command_line_has_its_secrets_hidden() {
    let data = Arc::new(data());
    let result = run(&data, PromptLevel::Full, "program_family", json!({"path": r"C:\x\widgetsync.exe"})).unwrap();
    let line = result["command_lines"][0].as_str().unwrap();
    assert!(line.contains("hidden") && !line.contains("abc"), "{line}");
}
