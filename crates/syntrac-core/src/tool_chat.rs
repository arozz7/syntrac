//! Phase 5, slice B1 (docs/PHASE5-PLAN.md): a chat turn that may ask for tools. A turn is sent whole and answered whole (not streamed), because the
//! pieces of a streamed tool call (a name in one chunk, the arguments split over many) are the part local servers get wrong most often. Pure: the
//! request body in the common chat API's tool form, and the reading of one reply. No I/O.

use serde_json::{json, Map, Value};

use crate::llm::LlmError;

/// One tool the model may ask for: a name, what it does, and the JSON schema of its arguments. The wording comes from configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// A tool the model asked for. `arguments` is JSON TEXT exactly as the model wrote it: it may be malformed, and only the loop that runs the tool
/// decides what to do about that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnMessage {
    System(String),
    User(String),
    /// What the model said, with the tools it asked for. Sent back exactly as received so the server can match the results.
    Assistant {
        content: String,
        tool_calls: Vec<ToolCall>,
    },
    /// The result of one tool call.
    Tool {
        call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnRequest {
    pub model: String,
    pub messages: Vec<TurnMessage>,
    /// Empty means the model may not call tools this turn (the field is then left out of the request).
    pub tools: Vec<ToolSpec>,
    pub max_tokens: Option<u32>,
    pub temperature: f64,
}

/// What the model answered in one turn.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnReply {
    pub content: String,
    /// How much reasoning the model produced (counted, never shown as the answer).
    pub reasoning_chars: usize,
    pub tool_calls: Vec<ToolCall>,
    /// The server stopped at the length limit: `content` is cut off.
    pub cut_off: bool,
    /// The calls were written by the model as `<tool_call>` text (a server that does not parse them itself), so the turn goes back as text.
    pub calls_as_text: bool,
    /// The text holds a `<tool_call>` tag that could not be read as a call (broken JSON, or cut off): worth telling the model, not an answer.
    pub tried_a_call: bool,
}

fn message_json(message: &TurnMessage) -> Value {
    match message {
        TurnMessage::System(content) => json!({"role": "system", "content": content}),
        TurnMessage::User(content) => json!({"role": "user", "content": content}),
        TurnMessage::Assistant { content, tool_calls } => {
            let mut map = Map::new();
            map.insert("role".into(), "assistant".into());
            map.insert("content".into(), content.clone().into());
            if !tool_calls.is_empty() {
                let calls: Vec<Value> = tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {"name": call.name, "arguments": call.arguments},
                        })
                    })
                    .collect();
                map.insert("tool_calls".into(), calls.into());
            }
            Value::Object(map)
        }
        TurnMessage::Tool { call_id, content } => json!({"role": "tool", "tool_call_id": call_id, "content": content}),
    }
}

/// The JSON body of a whole-reply chat request in the common chat API's tool form.
pub fn turn_body(request: &TurnRequest) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(message_json).collect();
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": false,
        "temperature": request.temperature,
    });
    if let Some(limit) = request.max_tokens {
        body["max_tokens"] = limit.into();
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {"name": tool.name, "description": tool.description, "parameters": tool.parameters},
                })
            })
            .collect();
        body["tools"] = tools.into();
    }
    body
}

/// Reads one whole reply. A call with no name is dropped; a call with no id is given one (`call_<n>`) so its result can be matched.
pub fn parse_turn_reply(text: &str) -> Result<TurnReply, LlmError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| LlmError::BadResponse("the reply from the server is not JSON".to_owned()))?;
    if let Some(error) = value.get("error") {
        let message = error.get("message").and_then(Value::as_str).or_else(|| error.as_str());
        return Err(LlmError::BadResponse(message.unwrap_or("the server reported an error").to_owned()));
    }
    let choice = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .ok_or_else(|| LlmError::BadResponse("the reply from the server has no answer in it".to_owned()))?;
    let message = choice.get("message").unwrap_or(&Value::Null);
    let text_of = |key: &str| message.get(key).and_then(Value::as_str).filter(|t| !t.is_empty());

    let mut tool_calls: Vec<ToolCall> = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().enumerate().filter_map(|(index, call)| tool_call(index, call)).collect())
        .unwrap_or_default();
    let (visible, thinking_chars) = strip_thinking(text_of("content").unwrap_or_default());
    let mut content = visible;
    let (mut calls_as_text, mut tried_a_call) = (false, false);
    if tool_calls.is_empty() {
        // A server that does not parse tool calls itself hands back what the model wrote: calls in `<tool_call>` tags.
        (content, tool_calls) = tagged_calls(&content);
        calls_as_text = !tool_calls.is_empty();
        tried_a_call = tool_calls.is_empty() && content.contains(CALL_OPEN);
    }
    let reasoning_chars =
        text_of("reasoning_content").or_else(|| text_of("reasoning")).map_or(0, |t| t.chars().count());
    Ok(TurnReply {
        content,
        reasoning_chars: reasoning_chars + thinking_chars,
        tool_calls,
        cut_off: choice.get("finish_reason").and_then(Value::as_str) == Some("length"),
        calls_as_text,
        tried_a_call,
    })
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// The text without its `<think>...</think>` traces (one that never ends is dropped to the end), and how many characters of thinking there were.
fn strip_thinking(text: &str) -> (String, usize) {
    let mut visible = String::new();
    let mut thinking = 0;
    let mut remaining = text;
    while let Some(start) = remaining.find(THINK_OPEN) {
        visible.push_str(&remaining[..start]);
        let after = &remaining[start + THINK_OPEN.len()..];
        match after.find(THINK_CLOSE) {
            Some(end) => {
                thinking += after[..end].chars().count();
                remaining = &after[end + THINK_CLOSE.len()..];
            }
            None => {
                thinking += after.chars().count();
                remaining = "";
            }
        }
    }
    visible.push_str(remaining);
    (visible.trim().to_owned(), thinking)
}

/// A call as the model writes it in text: the tag, holding a JSON object with its name and arguments.
pub fn render_call_as_text(call: &ToolCall) -> String {
    let arguments =
        serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|_| Value::String(call.arguments.clone()));
    format!(
        "{CALL_OPEN}
{}
{CALL_CLOSE}",
        json!({ "name": call.name, "arguments": arguments })
    )
}

fn tool_call(index: usize, call: &Value) -> Option<ToolCall> {
    let function = call.get("function")?;
    let name = function.get("name").and_then(Value::as_str).filter(|name| !name.is_empty())?;
    // Some servers send the arguments as an object instead of the JSON text the API asks for.
    let arguments = match function.get("arguments") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => "{}".to_owned(),
        Some(other) => other.to_string(),
    };
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(|| format!("call_{}", index + 1), str::to_owned);
    Some(ToolCall { id, name: name.to_owned(), arguments })
}

const CALL_OPEN: &str = "<tool_call>";
const CALL_CLOSE: &str = "</tool_call>";
/// How many calls written as text are taken from one reply.
const MAX_TAGGED_CALLS: usize = 8;

/// The calls a model wrote as `<tool_call>{"name": ..., "arguments": ...}</tool_call>` in its text, and the text without them. A block that is
/// not such a call stays in the text.
fn tagged_calls(content: &str) -> (String, Vec<ToolCall>) {
    let mut calls = Vec::new();
    let mut rest = String::new();
    let mut remaining = content;
    while let Some(start) = remaining.find(CALL_OPEN) {
        let after_open = &remaining[start + CALL_OPEN.len()..];
        let Some(end) = after_open.find(CALL_CLOSE) else { break };
        let block = &after_open[..end];
        let call = (calls.len() < MAX_TAGGED_CALLS).then(|| tagged_call(calls.len(), block)).flatten();
        rest.push_str(&remaining[..start]);
        match call {
            Some(call) => calls.push(call),
            None => rest.push_str(&remaining[start..start + CALL_OPEN.len() + end + CALL_CLOSE.len()]),
        }
        remaining = &after_open[end + CALL_CLOSE.len()..];
    }
    rest.push_str(remaining);
    (rest.trim().to_owned(), calls)
}

fn tagged_call(index: usize, block: &str) -> Option<ToolCall> {
    let value: Value = serde_json::from_str(block.trim()).ok()?;
    // The same shape as a call from the server, so name, arguments and the id are read in one place.
    tool_call(index, &json!({ "function": value }))
}
