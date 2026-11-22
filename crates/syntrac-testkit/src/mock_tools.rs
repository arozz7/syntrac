//! Stand-ins for the question box (docs/PHASE5-PLAN.md, B2): a model that plays a script of turns, and a read-only tool that answers from a
//! closure. With them the whole reasoning loop runs with no server and no data.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::{json, Value};
use syntrac_core::llm::{ChatRequest, Flow, LlmError, LlmService, ServerState, StreamEvent};
use syntrac_core::question_loop::{ReadOnlyTool, ToolError};
use syntrac_core::tool_chat::{ToolCall, ToolSpec, TurnReply, TurnRequest};

fn unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A model that answers each turn with the next scripted reply (or error), and keeps every request it was sent.
pub struct ScriptedTurns {
    script: Mutex<VecDeque<Result<TurnReply, LlmError>>>,
    asked: Mutex<Vec<TurnRequest>>,
}

impl ScriptedTurns {
    pub fn new(script: Vec<Result<TurnReply, LlmError>>) -> Self {
        Self { script: Mutex::new(script.into()), asked: Mutex::new(Vec::new()) }
    }

    /// Every request received so far, oldest first.
    pub fn requests(&self) -> Vec<TurnRequest> {
        unpoisoned(&self.asked).clone()
    }
}

/// A reply that asks for these tools, as (name, arguments text) pairs. The calls are numbered `call_1`, `call_2`, ... across the whole script
/// only if the caller numbers them; here each reply numbers its own.
pub fn asks_for(calls: &[(&str, &str)]) -> Result<TurnReply, LlmError> {
    let tool_calls = calls
        .iter()
        .enumerate()
        .map(|(index, (name, arguments))| ToolCall {
            id: format!("call_{}", index + 1),
            name: (*name).to_owned(),
            arguments: (*arguments).to_owned(),
        })
        .collect();
    Ok(TurnReply { tool_calls, ..TurnReply::default() })
}

/// A reply that is a final answer.
pub fn says(text: &str) -> Result<TurnReply, LlmError> {
    Ok(TurnReply { content: text.to_owned(), ..TurnReply::default() })
}

impl LlmService for ScriptedTurns {
    fn state(&self) -> ServerState {
        ServerState::Ready
    }

    fn stream_chat(&self, _: &ChatRequest, _: &mut dyn FnMut(StreamEvent) -> Flow) -> Result<(), LlmError> {
        Err(LlmError::BadResponse("ScriptedTurns only plays whole turns".to_owned()))
    }

    fn chat_turn(&self, request: &TurnRequest) -> Result<TurnReply, LlmError> {
        unpoisoned(&self.asked).push(request.clone());
        unpoisoned(&self.script)
            .pop_front()
            .unwrap_or_else(|| Err(LlmError::BadResponse("the script ran out".to_owned())))
    }
}

type Answer = Box<dyn Fn(&Value) -> Result<Value, ToolError> + Send + Sync>;

/// A read-only tool that answers from a closure and records the arguments it was run with.
pub struct MockTool {
    name: String,
    answer: Answer,
    ran_with: Mutex<Vec<Value>>,
}

impl MockTool {
    pub fn answering(name: &str, answer: impl Fn(&Value) -> Result<Value, ToolError> + Send + Sync + 'static) -> Self {
        Self { name: name.to_owned(), answer: Box::new(answer), ran_with: Mutex::new(Vec::new()) }
    }

    /// A tool that always answers with this value.
    pub fn returning(name: &str, value: Value) -> Self {
        Self::answering(name, move |_| Ok(value.clone()))
    }

    /// The arguments of every run so far, oldest first.
    pub fn runs(&self) -> Vec<Value> {
        unpoisoned(&self.ran_with).clone()
    }
}

impl ReadOnlyTool for MockTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: format!("The mock tool {}.", self.name),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }
    }

    fn run(&self, arguments: &Value) -> Result<Value, ToolError> {
        unpoisoned(&self.ran_with).push(arguments.clone());
        (self.answer)(arguments)
    }
}
