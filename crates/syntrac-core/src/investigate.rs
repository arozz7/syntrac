//! Phase 4, slice L4b (docs/PHASE4-PLAN.md): what a model is told about a finding or a program, at three detail levels. Pure: it takes plain
//! facts (`Evidence`, filled in by the app from the read-only views) and produces text. The privacy rules live here, in one place, so a test
//! can check every level for what must never be sent.
//!
//! The evidence is attacker-influenced text (file names, descriptions, finding text), so it is cleaned of control and direction characters,
//! cut to a fixed length, stripped of angle brackets and then quoted as values inside one JSON document in one marked block. The wording
//! around it comes only from `config/ai/prompts.toml`.

use std::net::{IpAddr, SocketAddr};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::clean_text::clean_text;
use crate::llm::DetailLevel;

const MAX_FINDING_LINES: usize = 12;
const MAX_TOP_DESTINATIONS: usize = 10;
const MAX_ACCOUNTS: usize = 6;
const MAX_LAUNCHERS: usize = 8;

/// How much is told. `Summary` and `Full` go to a model server on this machine; `External` is the text a person may paste into a hosted
/// assistant, so it is the most reduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptLevel {
    Summary,
    Full,
    External,
}

impl From<DetailLevel> for PromptLevel {
    fn from(level: DetailLevel) -> Self {
        match level {
            DetailLevel::Summary => Self::Summary,
            DetailLevel::Full => Self::Full,
        }
    }
}

impl PromptLevel {
    fn name(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Full => "full",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evidence {
    pub finding: Option<FindingFacts>,
    pub program: Option<ProgramFacts>,
    /// The Windows user name, replaced by `<user>` wherever it appears.
    pub user_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FindingFacts {
    pub kind: String,
    pub tier: String,
    pub title: String,
    pub why: String,
    pub evidence: Vec<String>,
    pub hits: u32,
    /// What the rule measured, when it measures a level.
    pub level: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccountFact {
    pub kind: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryFacts {
    pub hours_covered: u32,
    pub hours_present: u32,
    pub cpu_avg_percent: f64,
    pub cpu_peak_percent: f64,
    pub gpu_peak_percent: f64,
    pub memory_peak_mb: u64,
    pub starts: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DestinationFact {
    /// The network's registered owner, when known.
    pub owner: Option<String>,
    /// The name that was asked for, when known.
    pub name: Option<String>,
    pub addr: String,
    /// On the Internet (not the local network).
    pub internet: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProgramFacts {
    pub file_name: String,
    /// The folder the file is in.
    pub folder: String,
    /// The kind of place it runs from (`user_profile`, `program_files`, ...).
    pub location: String,
    pub signature: Option<String>,
    pub signer: Option<String>,
    pub publisher: Option<String>,
    pub product: Option<String>,
    pub description: Option<String>,
    pub file_version: Option<String>,
    pub sha256: Option<String>,
    pub downloaded: bool,
    pub runs_as: Vec<AccountFact>,
    pub service: bool,
    pub starts_at_boot: bool,
    pub launchers: Vec<String>,
    pub registered_as: Option<String>,
    /// Where it stands against the user's known programs, in a few words.
    pub standing: Option<String>,
    pub first_seen_days_ago: Option<u64>,
    pub last_seen_hours_ago: Option<u64>,
    pub history: Option<HistoryFacts>,
    /// The busiest destinations first (the caller may pass more; only the first few are used).
    pub destinations: Vec<DestinationFact>,
    /// How many distinct destinations there were in all.
    pub destinations_total: usize,
}

/// The wording around the evidence. Loaded from `config/ai/prompts.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct PromptConfig {
    pub system: String,
    pub instruction: String,
}

impl PromptConfig {
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// The wording shipped with the program.
    pub fn builtin() -> Self {
        Self::from_toml(include_str!("../../../config/ai/prompts.toml")).expect("config/ai/prompts.toml is valid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    /// The instruction followed by the evidence block.
    pub user: String,
}

impl Prompt {
    /// Everything in one text, for pasting into an assistant that has no separate instruction field.
    pub fn as_single_text(&self) -> String {
        format!("{}\n\n{}", self.system.trim(), self.user.trim())
    }
}

pub fn build_prompt(evidence: &Evidence, level: PromptLevel, config: &PromptConfig) -> Prompt {
    let block = serde_json::to_string_pretty(&evidence_json(evidence, level)).unwrap_or_else(|_| "{}".to_owned());
    Prompt {
        system: config.system.trim().to_owned(),
        user: format!("{}\n\n<evidence>\n{block}\n</evidence>", config.instruction.trim()),
    }
}

/// The evidence as one JSON document, with everything the level does not allow left out.
pub fn evidence_json(evidence: &Evidence, level: PromptLevel) -> Value {
    let writer = Writer { level, user: evidence.user_name.as_deref().unwrap_or("") };
    let mut root = Map::new();
    root.insert("detail_level".into(), level.name().into());
    if let Some(finding) = &evidence.finding {
        root.insert("finding".into(), writer.finding(finding));
    }
    if let Some(program) = &evidence.program {
        root.insert("program".into(), writer.program(program));
    }
    Value::Object(root)
}

/// One piece of outside text made safe to quote at a detail level: cleaned, cut, angle brackets neutralised, the user name replaced and (below
/// `Full`) addresses removed. For the tools of the question box, which must treat text exactly as the evidence does.
pub fn safe_text(raw: &str, max_chars: usize, level: PromptLevel, user_name: &str) -> Option<String> {
    Writer { level, user: user_name }.text(raw, max_chars)
}

struct Writer<'a> {
    level: PromptLevel,
    user: &'a str,
}

impl Writer<'_> {
    /// One piece of outside text, made safe to quote: cleaned, cut, the user name replaced, and (below `Full`) addresses removed.
    fn text(&self, raw: &str, max_chars: usize) -> Option<String> {
        let cleaned = clean_text(raw, max_chars)?.replace('<', "\u{2039}").replace('>', "\u{203A}");
        let redacted = redact_user_name(&cleaned, self.user);
        Some(if self.level == PromptLevel::Full { redacted } else { scrub_addresses(&redacted) })
    }

    fn text_of(&self, raw: &Option<String>, max_chars: usize) -> Option<Value> {
        raw.as_deref().and_then(|text| self.text(text, max_chars)).map(Value::from)
    }

    fn finding(&self, finding: &FindingFacts) -> Value {
        let mut map = Map::new();
        map.insert("kind".into(), self.text(&finding.kind, 80).into());
        map.insert("tier".into(), self.text(&finding.tier, 40).into());
        map.insert("times_found".into(), finding.hits.into());
        if let Some(level) = finding.level {
            map.insert("measured_level".into(), round1(level).into());
        }
        // The title, the reason and the evidence lines name programs, folders and addresses: they stay on this machine.
        if self.level != PromptLevel::External {
            map.insert("title".into(), self.text(&finding.title, 200).into());
            map.insert("why".into(), self.text(&finding.why, 400).into());
            let lines: Vec<Value> = finding
                .evidence
                .iter()
                .filter_map(|line| self.text(line, 300))
                .take(MAX_FINDING_LINES)
                .map(Value::from)
                .collect();
            map.insert("evidence".into(), lines.into());
        }
        Value::Object(map)
    }

    fn program(&self, program: &ProgramFacts) -> Value {
        let external = self.level == PromptLevel::External;
        let mut map = Map::new();
        put(&mut map, "location", self.text(&program.location, 40).map(Value::from));
        put(&mut map, "signature", self.text_of(&program.signature, 40));
        put(&mut map, "publisher", self.text_of(&program.publisher.clone().or_else(|| program.signer.clone()), 120));
        map.insert("downloaded_from_internet".into(), program.downloaded.into());
        map.insert("is_a_service".into(), program.service.into());
        map.insert("starts_with_windows".into(), program.starts_at_boot.into());
        let launchers: Vec<Value> = program
            .launchers
            .iter()
            .filter_map(|kind| self.text(kind, 40))
            .take(MAX_LAUNCHERS)
            .map(Value::from)
            .collect();
        map.insert("started_by".into(), launchers.into());
        map.insert("runs_as".into(), self.accounts(&program.runs_as));
        put(&mut map, "first_seen_days_ago", program.first_seen_days_ago.map(Value::from));
        put(&mut map, "last_seen_hours_ago", program.last_seen_hours_ago.map(Value::from));
        if let Some(history) = &program.history {
            map.insert("history".into(), history_json(history));
        }
        map.insert("destinations".into(), self.destinations(program));
        if !external {
            put(&mut map, "file_name", self.text(&program.file_name, 120).map(Value::from));
            put(&mut map, "folder", self.text(&program.folder, 260).map(Value::from));
            put(&mut map, "product", self.text_of(&program.product, 120));
            put(&mut map, "description", self.text_of(&program.description, 200));
            put(&mut map, "file_version", self.text_of(&program.file_version, 60));
            put(&mut map, "registered_as", self.text_of(&program.registered_as, 120));
            put(&mut map, "standing", self.text_of(&program.standing, 200));
        }
        if self.level == PromptLevel::Full {
            put(&mut map, "sha256", self.text_of(&program.sha256, 64));
        }
        Value::Object(map)
    }

    fn accounts(&self, accounts: &[AccountFact]) -> Value {
        let full = self.level == PromptLevel::Full;
        let items: Vec<Value> = accounts
            .iter()
            .take(MAX_ACCOUNTS)
            .map(|account| {
                let mut map = Map::new();
                put(&mut map, "kind", self.text(&account.kind, 40).map(Value::from));
                if full {
                    put(&mut map, "name", self.text_of(&account.name, 120));
                }
                Value::Object(map)
            })
            .collect();
        items.into()
    }

    fn destinations(&self, program: &ProgramFacts) -> Value {
        let mut map = Map::new();
        let internet = program.destinations.iter().filter(|d| d.internet).count();
        let listed = program.destinations.len();
        map.insert("total".into(), program.destinations_total.max(listed).into());
        // Counts and bytes cover the destinations that were given; the caller passes the busiest ones.
        map.insert("internet".into(), internet.into());
        map.insert("local_network".into(), (listed - internet).into());
        let sent: u64 = program.destinations.iter().fold(0, |sum, d| sum.saturating_add(d.bytes_sent));
        let received: u64 = program.destinations.iter().fold(0, |sum, d| sum.saturating_add(d.bytes_received));
        map.insert("bytes_sent".into(), sent.into());
        map.insert("bytes_received".into(), received.into());
        if self.level != PromptLevel::External {
            let top: Vec<Value> =
                program.destinations.iter().take(MAX_TOP_DESTINATIONS).map(|d| self.destination(d)).collect();
            map.insert("top".into(), top.into());
        }
        Value::Object(map)
    }

    fn destination(&self, destination: &DestinationFact) -> Value {
        let mut map = Map::new();
        put(&mut map, "network_owner", self.text_of(&destination.owner, 80));
        put(&mut map, "name_asked_for", self.text_of(&destination.name, 120));
        map.insert("kind".into(), if destination.internet { "internet" } else { "local network" }.into());
        if self.level == PromptLevel::Full {
            put(&mut map, "address", self.text(&destination.addr, 64).map(Value::from));
        }
        map.insert("bytes_sent".into(), destination.bytes_sent.into());
        map.insert("bytes_received".into(), destination.bytes_received.into());
        Value::Object(map)
    }
}

fn history_json(history: &HistoryFacts) -> Value {
    json!({
        "hours_covered": history.hours_covered,
        "hours_present": history.hours_present,
        "cpu_average_percent": round1(history.cpu_avg_percent),
        "cpu_peak_percent": round1(history.cpu_peak_percent),
        "gpu_peak_percent": round1(history.gpu_peak_percent),
        "memory_peak_mb": history.memory_peak_mb,
        "starts": history.starts,
    })
}

fn put(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        map.insert(key.to_owned(), value);
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// `text` with the Windows user name replaced by `<user>`, ignoring case. A name is replaced where it stands alone (not inside a longer
/// word); a very short one (under 3 characters) only inside a `Users` folder path, so ordinary words are not damaged.
pub fn redact_user_name(text: &str, user: &str) -> String {
    let user = user.trim();
    if user.is_empty() {
        return text.to_owned();
    }
    let (haystack, needle) = (text.to_ascii_lowercase(), user.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut search_from = 0;
    while let Some(found) = haystack[search_from..].find(&needle) {
        let start = search_from + found;
        let end = start + needle.len();
        let before_ok = haystack[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after_ok = haystack[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        let in_users_path = haystack[..start].ends_with("users\\") || haystack[..start].ends_with("users/");
        if before_ok && after_ok && (needle.chars().count() >= 3 || in_users_path) {
            out.push_str(&text[copied..start]);
            out.push_str("<user>");
            copied = end;
        }
        search_from = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// `text` with every IP address (with or without a port) replaced by `<address>`.
pub fn scrub_addresses(text: &str) -> String {
    let is_address_char = |c: char| c.is_ascii_hexdigit() || matches!(c, ':' | '.' | '[' | ']');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(is_address_char) {
        out.push_str(&rest[..start]);
        let run_length = rest[start..].find(|c: char| !is_address_char(c)).unwrap_or(rest.len() - start);
        let run = &rest[start..start + run_length];
        let candidate = run.trim_end_matches(['.', ':']);
        if is_address(candidate) {
            out.push_str("<address>");
            out.push_str(&run[candidate.len()..]);
        } else {
            out.push_str(run);
        }
        rest = &rest[start + run_length..];
    }
    out.push_str(rest);
    out
}

fn is_address(candidate: &str) -> bool {
    candidate.parse::<IpAddr>().is_ok() || candidate.parse::<SocketAddr>().is_ok()
}
