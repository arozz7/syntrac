//! A whole (not streamed) chat turn that may ask for tools, against a scripted server on loopback (docs/PHASE5-PLAN.md, B1).

mod fake_server;

use std::time::Duration;

use fake_server::{closed_port, reply, FakeServer};
use serde_json::{json, Value};
use syntrac_core::llm::{LlmError, LlmService, LoopbackEndpoint};
use syntrac_core::tool_chat::{ToolSpec, TurnMessage, TurnRequest};
use syntrac_llm::{HttpLimits, HttpLlm};

fn client(endpoint: &str) -> HttpLlm {
    client_with(endpoint, HttpLimits::default())
}

fn client_with(endpoint: &str, limits: HttpLimits) -> HttpLlm {
    HttpLlm::new(LoopbackEndpoint::parse(endpoint).unwrap(), limits)
}

fn turn(with_tools: bool) -> TurnRequest {
    TurnRequest {
        model: "Some Model".into(),
        messages: vec![TurnMessage::User("which programs are busiest?".into())],
        tools: if with_tools {
            vec![ToolSpec {
                name: "top_programs".into(),
                description: "The busiest programs.".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }]
        } else {
            vec![]
        },
        max_tokens: Some(100),
        temperature: 0.2,
    }
}

fn json_reply(body: &Value) -> fake_server::Reply {
    reply("200 OK", "Content-Type: application/json\r\n", &body.to_string())
}

#[test]
fn a_turn_is_posted_whole_with_the_tools_and_the_reply_is_read() {
    let server = FakeServer::start(|_| {
        json_reply(&json!({"choices": [{"message": {"content": null, "tool_calls": [
            {"id": "c1", "type": "function", "function": {"name": "top_programs", "arguments": "{}"}}
        ]}, "finish_reason": "tool_calls"}]}))
    });

    let reply = client(&server.endpoint()).chat_turn(&turn(true)).unwrap();

    assert_eq!(reply.tool_calls[0].name, "top_programs");
    let seen = &server.requests()[0];
    assert_eq!(seen.request_line, "POST /v1/chat/completions HTTP/1.1");
    let body: Value = serde_json::from_str(&seen.body).unwrap();
    assert_eq!(body["stream"], false);
    assert_eq!(body["model"], "Some Model");
    assert_eq!(body["tools"][0]["function"]["name"], "top_programs");
}

#[test]
fn a_plain_text_reply_is_a_final_answer() {
    let server = FakeServer::start(|_| json_reply(&json!({"choices": [{"message": {"content": "Chrome."}}]})));
    let reply = client(&server.endpoint()).chat_turn(&turn(true)).unwrap();
    assert_eq!(reply.content, "Chrome.");
    assert!(reply.tool_calls.is_empty());
}

#[test]
fn a_refusal_that_names_tools_or_the_template_means_tools_are_unsupported() {
    for message in
        ["tools param requires --jinja flag", "this model does not support tools", "Tool use is not supported"]
    {
        let body = json!({"error": {"message": message}}).to_string();
        let server =
            FakeServer::start(move |_| reply("500 Internal Server Error", "Content-Type: application/json\r\n", &body));
        let failed = client(&server.endpoint()).chat_turn(&turn(true)).unwrap_err();
        assert_eq!(failed, LlmError::ToolsUnsupported, "{message}");
    }
}

#[test]
fn an_error_status_that_does_not_mention_tools_is_just_a_status() {
    let server =
        FakeServer::start(|_| reply("500 Internal Server Error", "", r#"{"error":{"message":"out of memory"}}"#));
    assert_eq!(client(&server.endpoint()).chat_turn(&turn(true)).unwrap_err(), LlmError::Status(500));
}

#[test]
fn a_request_without_tools_never_reports_tools_unsupported() {
    let body = json!({"error": {"message": "tools param requires --jinja flag"}}).to_string();
    let server = FakeServer::start(move |_| reply("500 Internal Server Error", "", &body));
    assert_eq!(client(&server.endpoint()).chat_turn(&turn(false)).unwrap_err(), LlmError::Status(500));
}

#[test]
fn a_loading_server_is_reported_as_loading_with_its_retry_hint() {
    let server = FakeServer::start(|_| reply("503 Service Unavailable", "Retry-After: 7\r\n", ""));
    assert_eq!(
        client(&server.endpoint()).chat_turn(&turn(true)).unwrap_err(),
        LlmError::Loading { retry_after_secs: Some(7) }
    );
}

#[test]
fn a_reply_larger_than_the_limit_is_refused() {
    let big = "x".repeat(4096);
    let server = FakeServer::start(move |_| json_reply(&json!({"choices": [{"message": {"content": big}}]})));
    let limits = HttpLimits { max_answer_bytes: 1024, ..HttpLimits::default() };
    assert_eq!(client_with(&server.endpoint(), limits).chat_turn(&turn(true)).unwrap_err(), LlmError::TooLong);
}

#[test]
fn a_reply_that_is_not_json_is_unusable() {
    let server = FakeServer::start(|_| reply("200 OK", "", "<html>nope</html>"));
    assert!(matches!(client(&server.endpoint()).chat_turn(&turn(true)), Err(LlmError::BadResponse(_))));
}

#[test]
fn a_server_that_is_not_running_is_reported_as_such() {
    let endpoint = format!("http://127.0.0.1:{}/v1", closed_port());
    assert_eq!(client(&endpoint).chat_turn(&turn(true)).unwrap_err(), LlmError::NotRunning);
}

#[test]
fn a_server_that_never_answers_times_out() {
    let server = FakeServer::start(|_| fake_server::Reply { raw: String::new(), delay: Duration::from_secs(5) });
    let limits = HttpLimits { answer_timeout: Duration::from_millis(200), ..HttpLimits::default() };
    assert_eq!(client_with(&server.endpoint(), limits).chat_turn(&turn(true)).unwrap_err(), LlmError::TimedOut);
}
