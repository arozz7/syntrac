//! Phase 5, slice B2 (docs/PHASE5-PLAN.md): the reasoning loop of the question box. The model may ask for read-only tools; the loop runs the
//! registered ones, hands each result back as marked data, and always ends: after a fixed number of calls, when the time or the conversation
//! budget is used up, or when the person presses Stop. The last turn is made without tools and asks for an answer from what was gathered.
//!
//! Safety lives here, in one place. No tool can write, start or stop anything (the trait only reads). A name that is not registered is never
//! run and never echoed back. Arguments are parsed and size-checked before a tool sees them. Results are cleaned of the user name and of angle
//! brackets (so a result cannot close its own marked block), cut to a size, and counted against the conversation. Pure: the model, the tools and
//! the observer are all passed in.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use thiserror::Error;

use crate::clean_text::clean_text;
use crate::investigate::redact_user_name;
use crate::llm::{LlmError, LlmService};
use crate::tool_chat::{ToolCall, ToolSpec, TurnMessage, TurnRequest};

/// Room that must be left in the conversation for another tool result to be worth fetching.
const MIN_ROOM_BYTES: usize = 512;
/// Space kept for the "cut" note when a result is shortened.
const CUT_NOTE_RESERVE: usize = 64;
/// How much of a call's arguments the page is shown.
const SHOWN_ARGUMENT_CHARS: usize = 200;
const SHOWN_NAME_CHARS: usize = 60;
/// Names of the placeholders written by `redact_user_name` and `scrub_addresses`.
const PLACEHOLDERS: [&str; 2] = ["user", "address"];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolError {
    /// The arguments were not acceptable. The text goes back to the model so it can try again.
    #[error("{0}")]
    BadArguments(String),
    /// The tool could not do its work (the store was busy, the program is unknown).
    #[error("{0}")]
    Failed(String),
}

/// A tool the model may ask for. It can only read: nothing in this interface writes, starts or stops anything.
pub trait ReadOnlyTool: Send + Sync {
    fn spec(&self) -> ToolSpec;

    /// `arguments` is a JSON object. The result is data about the computer; the tool applies the person's detail level to it, and the loop
    /// then cleans and cuts it.
    fn run(&self, arguments: &Value) -> Result<Value, ToolError>;
}

/// The tools the model may ask for, by name. Nothing else can be run.
pub struct ToolRegistry {
    tools: Vec<(ToolSpec, Arc<dyn ReadOnlyTool>)>,
}

impl ToolRegistry {
    pub fn new(tools: Vec<Arc<dyn ReadOnlyTool>>) -> Self {
        Self { tools: tools.into_iter().map(|tool| (tool.spec(), tool)).collect() }
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|(spec, _)| spec.clone()).collect()
    }

    /// The tool by that name, if it is registered.
    pub fn tool(&self, name: &str) -> Option<Arc<dyn ReadOnlyTool>> {
        self.get(name).cloned()
    }

    fn get(&self, name: &str) -> Option<&Arc<dyn ReadOnlyTool>> {
        self.tools.iter().find(|(spec, _)| spec.name == name).map(|(_, tool)| tool)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopLimits {
    /// Tool calls in all, counting refused and repeated ones, so a model that loops still reaches the end.
    pub max_calls: usize,
    pub time_budget: Duration,
    /// The longest one result may be (the marked block included).
    pub result_bytes: usize,
    /// Everything sent to the model, summed over the messages: small local models have small windows.
    pub conversation_bytes: usize,
    pub arguments_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopSetup {
    pub model: String,
    pub system: String,
    /// The message that asks for an answer from what was gathered, sent with the last turn.
    pub final_request: String,
    /// The Windows user name, replaced by `<user>` in everything that goes to the model or to the page.
    pub user_name: Option<String>,
    pub limits: LoopLimits,
    pub max_tokens: Option<u32>,
    pub temperature: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStatus {
    Done,
    /// Not run: an unknown tool, bad arguments, a repeat, or a limit.
    Refused,
    /// Run, and the tool could not do its work.
    Failed,
}

/// One tool call, as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub name: String,
    /// The arguments as asked, cleaned and cut for display.
    pub arguments: String,
    pub status: CallStatus,
    /// Why it was refused or failed.
    pub note: Option<String>,
    pub result_bytes: usize,
    /// The result was cut to fit.
    pub cut: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Answered,
    CallLimit,
    TimeBudget,
    ContextFull,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopOutcome {
    pub answer: String,
    pub cut_off: bool,
    pub reasoning_chars: usize,
    pub calls: Vec<CallRecord>,
    pub stopped: StopReason,
}

/// What the loop reports to, and asks of, whoever runs it (the job, in the app).
pub trait LoopObserver {
    /// Whether the person pressed Stop. Asked between turns and between calls.
    fn cancelled(&self) -> bool;

    /// A call was made (or refused). Lets the page list it while the loop is still going.
    fn call_made(&self, _record: &CallRecord) {}
}

/// Runs one question to its end. `Err` is a failure of the model server (including a server that cannot do tool calls); what was gathered
/// before then was already reported to the observer.
pub fn run_question_loop(
    llm: &dyn LlmService,
    registry: &ToolRegistry,
    setup: &LoopSetup,
    question: &str,
    observer: &dyn LoopObserver,
) -> Result<LoopOutcome, LlmError> {
    Loop::new(llm, registry, setup, question, observer).run()
}

struct Loop<'a> {
    llm: &'a dyn LlmService,
    registry: &'a ToolRegistry,
    setup: &'a LoopSetup,
    observer: &'a dyn LoopObserver,
    started: Instant,
    messages: Vec<TurnMessage>,
    used_bytes: usize,
    calls: Vec<CallRecord>,
    already_asked: HashSet<(String, String)>,
    reasoning_chars: usize,
    /// Set when the conversation is full: the next turn is the last.
    full: bool,
}

impl<'a> Loop<'a> {
    fn new(
        llm: &'a dyn LlmService,
        registry: &'a ToolRegistry,
        setup: &'a LoopSetup,
        question: &str,
        observer: &'a dyn LoopObserver,
    ) -> Self {
        Self {
            llm,
            registry,
            setup,
            observer,
            started: Instant::now(),
            used_bytes: setup.system.len() + question.len(),
            messages: vec![TurnMessage::System(setup.system.clone()), TurnMessage::User(question.to_owned())],
            calls: Vec::new(),
            already_asked: HashSet::new(),
            reasoning_chars: 0,
            full: false,
        }
    }

    fn run(mut self) -> Result<LoopOutcome, LlmError> {
        loop {
            if self.observer.cancelled() {
                return Ok(self.outcome(String::new(), false, StopReason::Cancelled));
            }
            let last_turn = self.reason_to_end();
            if last_turn.is_some() {
                self.messages.push(TurnMessage::User(self.setup.final_request.clone()));
            }
            let reply = self.llm.chat_turn(&self.request(last_turn.is_none()))?;
            self.reasoning_chars += reply.reasoning_chars;

            if let Some(reason) = last_turn {
                return Ok(self.outcome(reply.content.trim().to_owned(), reply.cut_off, reason));
            }
            if reply.tool_calls.is_empty() {
                return Ok(self.outcome(reply.content.trim().to_owned(), reply.cut_off, StopReason::Answered));
            }
            self.used_bytes +=
                reply.content.len() + reply.tool_calls.iter().map(|c| c.name.len() + c.arguments.len()).sum::<usize>();
            self.messages.push(TurnMessage::Assistant { content: reply.content, tool_calls: reply.tool_calls.clone() });
            for call in &reply.tool_calls {
                if self.observer.cancelled() {
                    return Ok(self.outcome(String::new(), false, StopReason::Cancelled));
                }
                self.answer_call(call);
            }
            self.full = self.room() < MIN_ROOM_BYTES;
        }
    }

    /// Why the next turn must be the last, if it must.
    fn reason_to_end(&self) -> Option<StopReason> {
        if self.full {
            Some(StopReason::ContextFull)
        } else if self.calls.len() >= self.setup.limits.max_calls {
            Some(StopReason::CallLimit)
        } else if self.started.elapsed() >= self.setup.limits.time_budget {
            Some(StopReason::TimeBudget)
        } else {
            None
        }
    }

    fn request(&self, with_tools: bool) -> TurnRequest {
        TurnRequest {
            model: self.setup.model.clone(),
            messages: self.messages.clone(),
            tools: if with_tools { self.registry.specs() } else { Vec::new() },
            max_tokens: self.setup.max_tokens,
            temperature: self.setup.temperature,
        }
    }

    fn outcome(self, answer: String, cut_off: bool, stopped: StopReason) -> LoopOutcome {
        LoopOutcome { answer, cut_off, reasoning_chars: self.reasoning_chars, calls: self.calls, stopped }
    }

    fn room(&self) -> usize {
        self.setup.limits.conversation_bytes.saturating_sub(self.used_bytes)
    }

    /// Runs (or refuses) one call and adds its result to the conversation. Every call gets a result, so the server can match them.
    fn answer_call(&mut self, call: &ToolCall) {
        let shown_name = self.registry.get(&call.name).map_or("unknown", |_| call.name.as_str()).to_owned();
        let (outcome, note) = self.execute(call);
        let (status, body) = match outcome {
            Ok(value) => (CallStatus::Done, value),
            Err((status, message)) => (status, json!({ "error": message })),
        };
        let (content, cut) = self.wrap_result(&shown_name, &body);
        let record = CallRecord {
            name: self.display(&call.name, SHOWN_NAME_CHARS),
            arguments: self.display(&call.arguments, SHOWN_ARGUMENT_CHARS),
            status,
            note,
            result_bytes: content.len(),
            cut,
        };
        self.used_bytes += content.len();
        self.messages.push(TurnMessage::Tool { call_id: call.id.clone(), content });
        self.observer.call_made(&record);
        self.calls.push(record);
    }

    /// The result of a call, or why it was not run, and the note for the page.
    fn execute(&mut self, call: &ToolCall) -> (Result<Value, (CallStatus, String)>, Option<String>) {
        let refuse = |message: &str| (Err((CallStatus::Refused, message.to_owned())), Some(message.to_owned()));
        if self.calls.len() >= self.setup.limits.max_calls {
            return refuse("the limit of tool calls was reached, so this call was not run; answer from what you have");
        }
        if self.room() < MIN_ROOM_BYTES {
            return refuse("the conversation is full, so this call was not run; answer from what you have");
        }
        let Some(tool) = self.registry.get(&call.name).cloned() else {
            return refuse("there is no tool by that name; use only the tools you were given");
        };
        if call.arguments.len() > self.setup.limits.arguments_bytes {
            return refuse("the arguments are too long");
        }
        let arguments = match parse_arguments(&call.arguments) {
            Ok(arguments) => arguments,
            Err(message) => return refuse(message),
        };
        // Written differently, the same arguments are still the same call: objects are compared by content, not by spelling.
        if !self.already_asked.insert((call.name.clone(), arguments.to_string())) {
            return refuse("you already asked this and the answer is above; do not repeat it");
        }
        match tool.run(&arguments) {
            Ok(value) => (Ok(value), None),
            Err(ToolError::BadArguments(message)) => (Err((CallStatus::Refused, message.clone())), Some(message)),
            Err(ToolError::Failed(message)) => (Err((CallStatus::Failed, message.clone())), Some(message)),
        }
    }

    /// Text for the page or the model: the user name replaced, control characters removed, cut to a length.
    fn display(&self, text: &str, max_chars: usize) -> String {
        let redacted = redact_user_name(text, self.setup.user_name.as_deref().unwrap_or(""));
        clean_text(&redacted, max_chars).unwrap_or_default()
    }

    /// The result as the model reads it: a marked block holding the cleaned, cut data.
    fn wrap_result(&self, name: &str, body: &Value) -> (String, bool) {
        let safe = self.safe_value(body).to_string();
        let (open, close) = (format!("<tool_result name=\"{name}\">\n"), "\n</tool_result>");
        let allowed = self.setup.limits.result_bytes.min(self.room());
        let body_room = allowed.saturating_sub(open.len() + close.len());
        let (fitted, cut) = fit(&safe, body_room);
        (format!("{open}{fitted}{close}"), cut)
    }

    /// The data with every text made safe to quote: angle brackets neutralised (so nothing in it can close or open a marked block), then the
    /// user name replaced (the replacement is the only angle-bracket text left).
    fn safe_value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.safe_text(text)),
            Value::Array(items) => Value::Array(items.iter().map(|item| self.safe_value(item)).collect()),
            Value::Object(map) => {
                Value::Object(map.iter().map(|(key, item)| (self.safe_text(key), self.safe_value(item))).collect())
            }
            other => other.clone(),
        }
    }

    fn safe_text(&self, text: &str) -> String {
        let neutral = text.replace('<', "\u{2039}").replace('>', "\u{203A}");
        let redacted = redact_user_name(&neutral, self.setup.user_name.as_deref().unwrap_or(""));
        // The placeholders the tools themselves write (the cleaning of the evidence) are put back: they close nothing.
        PLACEHOLDERS
            .iter()
            .fold(redacted, |text, name| text.replace(&format!("\u{2039}{name}\u{203A}"), &format!("<{name}>")))
    }
}

/// An empty text counts as no arguments; anything else must be a JSON object.
fn parse_arguments(text: &str) -> Result<Value, &'static str> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(value @ Value::Object(_)) => Ok(value),
        _ => Err("the arguments must be a JSON object"),
    }
}

/// `text` cut to at most `max_bytes` (a cut is marked in the text itself).
fn fit(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let mut end = max_bytes.saturating_sub(CUT_NOTE_RESERVE);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}\u{2026} [cut: {} bytes left out]", &text[..end], text.len() - end), true)
}
