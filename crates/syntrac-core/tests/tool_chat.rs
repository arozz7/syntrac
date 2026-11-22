//! Phase 5, slice B1 (docs/PHASE5-PLAN.md): a chat turn that may ask for tools. The request body in the common chat API's tool form, and the
//! reading of a whole (not streamed) reply. Pure: no I/O.

use serde_json::{json, Value};
use syntrac_core::llm::LlmError;
use syntrac_core::tool_chat::{parse_turn_reply, turn_body, ToolCall, ToolSpec, TurnMessage, TurnRequest};

fn spec() -> ToolSpec {
    ToolSpec {
        name: "program_facts".into(),
        description: "What is known about one program.".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
    }
}

fn request(messages: Vec<TurnMessage>, tools: Vec<ToolSpec>) -> TurnRequest {
    TurnRequest { model: "m".into(), messages, tools, max_tokens: Some(500), temperature: 0.2 }
}

fn call(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall { id: id.into(), name: name.into(), arguments: arguments.into() }
}

fn reply(body: Value) -> Result<syntrac_core::tool_chat::TurnReply, LlmError> {
    parse_turn_reply(&body.to_string())
}

// ---- the request

#[test]
fn a_turn_is_not_streamed_and_carries_the_tools_in_the_function_form() {
    let body = turn_body(&request(vec![TurnMessage::User("hi".into())], vec![spec()]));
    assert_eq!(body["stream"], false);
    assert_eq!(body["model"], "m");
    assert_eq!(body["max_tokens"], 500);
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "program_facts");
    assert_eq!(body["tools"][0]["function"]["description"], "What is known about one program.");
    assert_eq!(body["tools"][0]["function"]["parameters"]["required"][0], "path");
}

#[test]
fn with_no_tools_the_field_is_left_out_altogether() {
    let body = turn_body(&request(vec![TurnMessage::User("hi".into())], vec![]));
    assert!(body.get("tools").is_none(), "an empty list is refused by some servers");
}

#[test]
fn messages_carry_their_roles() {
    let body =
        turn_body(&request(vec![TurnMessage::System("rules".into()), TurnMessage::User("question".into())], vec![]));
    assert_eq!(body["messages"][0], json!({"role": "system", "content": "rules"}));
    assert_eq!(body["messages"][1], json!({"role": "user", "content": "question"}));
}

#[test]
fn an_assistant_turn_that_called_tools_is_sent_back_as_it_was_received() {
    let body = turn_body(&request(
        vec![TurnMessage::Assistant {
            content: String::new(),
            tool_calls: vec![call("call_1", "program_facts", r#"{"path":"c:\\a.exe"}"#)],
        }],
        vec![spec()],
    ));
    let message = &body["messages"][0];
    assert_eq!(message["role"], "assistant");
    assert_eq!(message["tool_calls"][0]["id"], "call_1");
    assert_eq!(message["tool_calls"][0]["type"], "function");
    assert_eq!(message["tool_calls"][0]["function"]["name"], "program_facts");
    assert_eq!(
        message["tool_calls"][0]["function"]["arguments"], r#"{"path":"c:\\a.exe"}"#,
        "the arguments are a JSON text, exactly as received"
    );
}

#[test]
fn an_assistant_turn_with_no_tool_calls_has_none() {
    let body =
        turn_body(&request(vec![TurnMessage::Assistant { content: "an answer".into(), tool_calls: vec![] }], vec![]));
    assert_eq!(body["messages"][0], json!({"role": "assistant", "content": "an answer"}));
}

#[test]
fn a_tool_result_names_the_call_it_answers() {
    let body = turn_body(&request(
        vec![TurnMessage::Tool { call_id: "call_1".into(), content: r#"{"ok":true}"#.into() }],
        vec![spec()],
    ));
    assert_eq!(body["messages"][0], json!({"role": "tool", "tool_call_id": "call_1", "content": r#"{"ok":true}"#}));
}

// ---- the reply

#[test]
fn a_plain_answer_is_its_text_with_no_tool_calls() {
    let parsed = reply(json!({"choices": [{"message": {"role": "assistant", "content": "It looks ordinary."}, "finish_reason": "stop"}]}))
        .unwrap();
    assert_eq!(parsed.content, "It looks ordinary.");
    assert!(parsed.tool_calls.is_empty());
    assert!(!parsed.cut_off);
}

#[test]
fn tool_calls_come_back_with_their_id_name_and_arguments_text() {
    let parsed = reply(json!({"choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [
        {"id": "abc", "type": "function", "function": {"name": "program_facts", "arguments": "{\"path\":\"x\"}"}},
        {"id": "def", "type": "function", "function": {"name": "list_findings", "arguments": "{}"}}
    ]}, "finish_reason": "tool_calls"}]}))
    .unwrap();
    assert_eq!(parsed.content, "");
    assert_eq!(
        parsed.tool_calls,
        vec![call("abc", "program_facts", r#"{"path":"x"}"#), call("def", "list_findings", "{}")]
    );
}

#[test]
fn arguments_given_as_an_object_are_turned_into_json_text() {
    let parsed = reply(json!({"choices": [{"message": {"tool_calls": [
        {"id": "a", "type": "function", "function": {"name": "n", "arguments": {"path": "x"}}}
    ]}}]}))
    .unwrap();
    let args: Value = serde_json::from_str(&parsed.tool_calls[0].arguments).unwrap();
    assert_eq!(args, json!({"path": "x"}));
}

#[test]
fn a_call_with_no_id_is_given_one_so_its_result_can_be_matched() {
    let parsed = reply(json!({"choices": [{"message": {"tool_calls": [
        {"type": "function", "function": {"name": "a", "arguments": "{}"}},
        {"type": "function", "function": {"name": "b", "arguments": "{}"}}
    ]}}]}))
    .unwrap();
    assert_eq!(parsed.tool_calls.len(), 2);
    assert_ne!(parsed.tool_calls[0].id, parsed.tool_calls[1].id);
    assert!(!parsed.tool_calls[0].id.is_empty());
}

#[test]
fn a_call_with_no_name_is_dropped() {
    let parsed = reply(json!({"choices": [{"message": {"tool_calls": [
        {"id": "a", "type": "function", "function": {"arguments": "{}"}}
    ]}}]}))
    .unwrap();
    assert!(parsed.tool_calls.is_empty());
}

#[test]
fn reasoning_is_counted_and_never_part_of_the_answer() {
    let parsed =
        reply(json!({"choices": [{"message": {"content": "Answer.", "reasoning_content": "thinking hard"}}]})).unwrap();
    assert_eq!(parsed.content, "Answer.");
    assert_eq!(parsed.reasoning_chars, "thinking hard".chars().count());
}

#[test]
fn a_reply_that_stopped_at_the_length_limit_is_marked_cut_off() {
    let parsed = reply(json!({"choices": [{"message": {"content": "Partial"}, "finish_reason": "length"}]})).unwrap();
    assert!(parsed.cut_off);
}

#[test]
fn an_error_in_the_reply_is_reported_with_the_servers_words() {
    let failed = reply(json!({"error": {"message": "model not loaded"}})).unwrap_err();
    assert_eq!(failed, LlmError::BadResponse("model not loaded".into()));
}

#[test]
fn text_that_is_not_json_or_has_no_choice_is_unusable() {
    assert!(matches!(parse_turn_reply("<html>"), Err(LlmError::BadResponse(_))));
    assert!(matches!(reply(json!({"choices": []})), Err(LlmError::BadResponse(_))));
    assert!(matches!(reply(json!({"something": "else"})), Err(LlmError::BadResponse(_))));
}
