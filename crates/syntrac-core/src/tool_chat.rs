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

    let tool_calls: Vec<ToolCall> = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().enumerate().filter_map(|(index, call)| tool_call(index, call)).collect())
        .unwrap_or_default();
    Ok(TurnReply {
        content: text_of("content").unwrap_or_default().to_owned(),
        reasoning_chars: text_of("reasoning_content").or_else(|| text_of("reasoning")).map_or(0, |t| t.chars().count()),
        tool_calls,
        cut_off: choice.get("finish_reason").and_then(Value::as_str) == Some("length"),
    })
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
