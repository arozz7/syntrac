//! Phase 5, slice B3 (docs/PHASE5-PLAN.md): the read-only tools the question box offers a model. Each one reads facts through `QuestionData`
//! (the database, in the real program) and renders them at the person's detail level with the same cleaning as the evidence for Investigate
//! (`syntrac_core::investigate`): outside text cleaned and cut, the user name replaced, and below the full level no addresses, hashes or
//! command lines. No tool can write, start or stop anything, and a path is only ever looked up in what the store recorded, never read from
//! the disk.

use std::sync::Arc;

use serde_json::{json, Map, Value};
use syntrac_core::investigate::{evidence_json, safe_text, Evidence, FindingFacts, PromptLevel};
use syntrac_core::question_config::QuestionConfig;
use syntrac_core::question_loop::{ReadOnlyTool, ToolError, ToolRegistry};
use syntrac_core::tool_chat::ToolSpec;

use crate::history_programs::ProgramMetricParam;

pub const TOOL_NAMES: [&str; 4] = ["program_facts", "list_findings", "top_programs", "program_family"];

const DEFAULT_FINDINGS: u64 = 5;
const MAX_FINDINGS: u64 = 10;
const DEFAULT_TOP: u64 = 5;
const MAX_TOP: u64 = 10;
const DEFAULT_DAYS: u64 = 7;
const MAX_DAYS: u64 = 14;
const MAX_COMMAND_LINES: usize = 3;
const MAX_NAMES: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct TopProgram {
    pub name: String,
    pub path: String,
    /// The kind of place it runs from (`user_profile`, `program_files`, ...).
    pub location: String,
    pub cpu_avg_percent: f64,
    pub memory_peak_mb: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub starts: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedName {
    pub name: String,
    pub count: usize,
}

/// Who started a program and what it started, over the days of process detail that are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramFamily {
    pub instances: usize,
    pub started_by: Vec<CountedName>,
    pub started: Vec<CountedName>,
    /// A few command lines it ran with (only ever shown at the full detail level).
    pub command_lines: Vec<String>,
}

/// The reads the tools need. The real one reads the database through the same views the screens use; tests give plain values.
pub trait QuestionData: Send + Sync {
    /// Everything known about a recorded program; `None` when the store has never recorded that path.
    fn program(&self, path: &str) -> Result<Option<Evidence>, String>;
    /// The findings with the most recent activity first.
    fn findings(&self, limit: usize) -> Result<Vec<FindingFacts>, String>;
    fn top_programs(&self, metric: ProgramMetricParam, days: u64, limit: usize) -> Result<Vec<TopProgram>, String>;
    /// `None` when no process of that program was recorded.
    fn family(&self, path: &str) -> Result<Option<ProgramFamily>, String>;
}

type Run = Box<dyn Fn(&Value) -> Result<Value, ToolError> + Send + Sync>;

/// A tool made of its description and one function.
struct FnTool {
    spec: ToolSpec,
    run: Run,
}

impl ReadOnlyTool for FnTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    fn run(&self, arguments: &Value) -> Result<Value, ToolError> {
        (self.run)(arguments)
    }
}

/// What every tool needs to render: where the facts come from and how much may be told.
#[derive(Clone)]
struct Context {
    data: Arc<dyn QuestionData>,
    level: PromptLevel,
    user: String,
}

impl Context {
    fn text(&self, raw: &str, max_chars: usize) -> Value {
        safe_text(raw, max_chars, self.level, &self.user).map_or(Value::Null, Value::from)
    }

    fn evidence(&self, mut evidence: Evidence) -> Value {
        evidence.user_name = Some(self.user.clone()).filter(|name| !name.is_empty());
        evidence_json(&evidence, self.level)
    }
}

/// The tools, wired to a source of facts, at a detail level, with the wording from the config.
pub fn question_registry(
    data: Arc<dyn QuestionData>,
    config: &QuestionConfig,
    level: PromptLevel,
    user_name: Option<String>,
) -> ToolRegistry {
    let context = Context { data, level, user: user_name.unwrap_or_default() };
    let tool = |name: &str, parameters: Value, run: Run| -> Arc<dyn ReadOnlyTool> {
        Arc::new(FnTool {
            spec: ToolSpec { name: name.to_owned(), description: config.describe(name), parameters },
            run,
        })
    };
    let path_only = json!({
        "type": "object",
        "properties": {"path": {"type": "string", "description": "The full path of a program that was recorded."}},
        "required": ["path"],
    });
    let (facts, findings, top, family) = (context.clone(), context.clone(), context.clone(), context);
    ToolRegistry::new(vec![
        tool("program_facts", path_only.clone(), Box::new(move |args| program_facts(&facts, args))),
        tool(
            "list_findings",
            json!({"type": "object", "properties": {"limit": {"type": "integer", "description": "How many, at most 10."}}}),
            Box::new(move |args| list_findings(&findings, args)),
        ),
        tool(
            "top_programs",
            json!({"type": "object", "properties": {
                "metric": {"type": "string", "enum": ["cpu", "memory", "starts", "network", "gpu"]},
                "days": {"type": "integer", "description": "How many days back, at most 14."},
                "limit": {"type": "integer", "description": "How many programs, at most 10."},
            }}),
            Box::new(move |args| top_programs(&top, args)),
        ),
        tool("program_family", path_only, Box::new(move |args| program_family(&family, args))),
    ])
}

fn store_failure(message: String) -> ToolError {
    ToolError::Failed(message)
}

fn required_path(arguments: &Value) -> Result<String, ToolError> {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ToolError::BadArguments("path is required: give the full path of a program as text".to_owned()))
}

/// A whole number argument kept within bounds; a missing one is the default and one that is not a number is refused.
fn bounded(arguments: &Value, key: &str, default: u64, min: u64, max: u64) -> Result<u64, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_i64()
            .map(|number| u64::try_from(number).unwrap_or(0).clamp(min, max))
            .ok_or_else(|| ToolError::BadArguments(format!("{key} must be a whole number"))),
    }
}

const NOT_RECORDED: &str = "no program with that path was recorded; use top_programs to find the path of a program";

fn program_facts(context: &Context, arguments: &Value) -> Result<Value, ToolError> {
    let path = required_path(arguments)?;
    let evidence = context
        .data
        .program(&path)
        .map_err(store_failure)?
        .ok_or_else(|| ToolError::BadArguments(NOT_RECORDED.to_owned()))?;
    Ok(context.evidence(evidence))
}

fn list_findings(context: &Context, arguments: &Value) -> Result<Value, ToolError> {
    let limit = bounded(arguments, "limit", DEFAULT_FINDINGS, 1, MAX_FINDINGS)?;
    let facts = context.data.findings(usize::try_from(limit).unwrap_or(1)).map_err(store_failure)?;
    let findings: Vec<Value> = facts
        .into_iter()
        .map(|finding| {
            let evidence = Evidence { finding: Some(finding), ..Evidence::default() };
            context.evidence(evidence).get("finding").cloned().unwrap_or(Value::Null)
        })
        .collect();
    Ok(json!({ "shown": findings.len(), "findings": findings }))
}

fn parse_metric(arguments: &Value) -> Result<ProgramMetricParam, ToolError> {
    let Some(name) = arguments.get("metric").filter(|value| !value.is_null()) else {
        return Ok(ProgramMetricParam::Cpu);
    };
    serde_json::from_value(name.clone())
        .map_err(|_| ToolError::BadArguments("metric must be one of cpu, memory, starts, network, gpu".to_owned()))
}

fn top_programs(context: &Context, arguments: &Value) -> Result<Value, ToolError> {
    let metric = parse_metric(arguments)?;
    let days = bounded(arguments, "days", DEFAULT_DAYS, 1, MAX_DAYS)?;
    let limit = bounded(arguments, "limit", DEFAULT_TOP, 1, MAX_TOP)?;
    let rows = context.data.top_programs(metric, days, usize::try_from(limit).unwrap_or(1)).map_err(store_failure)?;
    let programs: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "name": context.text(&row.name, 120),
                "path": context.text(&row.path, 260),
                "location": context.text(&row.location, 40),
                "cpu_average_percent": round1(row.cpu_avg_percent),
                "memory_peak_mb": row.memory_peak_mb,
                "bytes_sent": row.bytes_sent,
                "bytes_received": row.bytes_received,
                "starts": row.starts,
            })
        })
        .collect();
    Ok(json!({ "days": days, "programs": programs }))
}

fn program_family(context: &Context, arguments: &Value) -> Result<Value, ToolError> {
    let path = required_path(arguments)?;
    let family = context
        .data
        .family(&path)
        .map_err(store_failure)?
        .ok_or_else(|| ToolError::BadArguments(NOT_RECORDED.to_owned()))?;
    let names = |list: &[CountedName]| -> Vec<Value> {
        list.iter()
            .take(MAX_NAMES)
            .map(|item| json!({ "name": context.text(&item.name, 120), "count": item.count }))
            .collect()
    };
    let mut map = Map::new();
    map.insert("instances".into(), family.instances.into());
    map.insert("started_by".into(), names(&family.started_by).into());
    map.insert("started".into(), names(&family.started).into());
    // Command lines often hold tokens, passwords and the profile folder: only at the full level, with secrets hidden and the user name replaced.
    if context.level == PromptLevel::Full {
        let lines: Vec<Value> = family
            .command_lines
            .iter()
            .take(MAX_COMMAND_LINES)
            .map(|line| context.text(&hide_secret_arguments(line), 200))
            .collect();
        map.insert("command_lines".into(), lines.into());
    }
    Ok(Value::Object(map))
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

const SECRET_WORDS: [&str; 9] = ["token", "password", "passwd", "pwd", "secret", "key", "auth", "credential", "cookie"];

fn names_a_secret(argument_name: &str) -> bool {
    let lower = argument_name.to_lowercase();
    SECRET_WORDS.iter().any(|word| lower.contains(word))
}

/// The command line with the value of every argument that names a secret (`--token=abc`, `/password:abc`, `--key abc`) replaced by `[hidden]`.
pub fn hide_secret_arguments(line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut hide_next = false;
    for word in line.split_whitespace() {
        if hide_next && !word.starts_with(['-', '/']) {
            out.push("[hidden]".to_owned());
            hide_next = false;
            continue;
        }
        hide_next = false;
        match word.find(['=', ':']) {
            Some(at) if word.starts_with(['-', '/']) && names_a_secret(&word[..at]) => {
                out.push(format!("{}[hidden]", &word[..=at]));
            }
            None if word.starts_with(['-', '/']) && names_a_secret(word) => {
                hide_next = true;
                out.push(word.to_owned());
            }
            _ => out.push(word.to_owned()),
        }
    }
    out.join(" ")
}
