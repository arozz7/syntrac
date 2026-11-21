//! Phase 4, slice L4a (docs/PHASE4-PLAN.md): the loopback-only endpoint guard, the chat request body and the reading of a streamed answer.

use syntrac_core::llm::{
    chat_body, parse_stream_line, AiSettings, ChatMessage, ChatRequest, DetailLevel, EndpointError, LoopbackEndpoint,
    Role, StreamEvent, StreamLine,
};

fn endpoint(text: &str) -> LoopbackEndpoint {
    LoopbackEndpoint::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn refused(text: &str) -> EndpointError {
    LoopbackEndpoint::parse(text).expect_err(text)
}

#[test]
fn the_usual_local_server_addresses_are_accepted() {
    assert_eq!(
        endpoint("http://127.0.0.1:1234/v1").api_url("/chat/completions"),
        "http://127.0.0.1:1234/v1/chat/completions"
    );
    assert_eq!(endpoint("http://127.0.0.1:8080/v1/").api_url("/models"), "http://127.0.0.1:8080/v1/models");
    assert_eq!(endpoint("HTTP://127.5.6.7:80").api_url("/models"), "http://127.5.6.7:80/models");
    assert_eq!(endpoint("http://[::1]:8080/v1").api_url("/models"), "http://[::1]:8080/v1/models");
}

#[test]
fn localhost_is_connected_to_as_the_numeric_loopback_address() {
    // The name is never resolved, so a changed hosts file or a DNS answer cannot send the traffic elsewhere.
    let local = endpoint("http://LocalHost:1234/v1");
    assert_eq!(local.api_url("/models"), "http://127.0.0.1:1234/v1/models");
    assert_eq!(local.origin_url("/health"), "http://127.0.0.1:1234/health");
}

#[test]
fn a_missing_port_means_the_web_default() {
    assert_eq!(endpoint("http://127.0.0.1/v1").origin_url("/health"), "http://127.0.0.1:80/health");
}

#[test]
fn an_address_that_is_not_this_machine_is_refused() {
    for text in [
        "http://192.168.1.10:1234/v1",
        "http://10.0.0.5/v1",
        "http://example.com/v1",
        "http://localhost.evil.example/v1",
        "http://127.0.0.1.evil.example/v1",
        "http://0.0.0.0:1234/v1",
        "http://[::2]:1234/v1",
        "http://[fe80::1]/v1",
        "http://128.0.0.1/v1",
    ] {
        assert_eq!(refused(text), EndpointError::NotThisMachine, "{text}");
    }
}

#[test]
fn only_plain_http_is_accepted() {
    assert_eq!(refused("https://127.0.0.1:1234/v1"), EndpointError::NotHttp);
    assert_eq!(refused("ftp://127.0.0.1/v1"), EndpointError::NotHttp);
    assert_eq!(refused("127.0.0.1:1234/v1"), EndpointError::NotHttp);
}

#[test]
fn tricks_in_the_address_are_refused() {
    // A user name before an @ would make the real host the part after it.
    assert_eq!(refused("http://127.0.0.1@evil.example/v1"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:1234@evil.example/v1"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:1234/v1?key=1"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:1234/v1#top"), EndpointError::Malformed);
    assert_eq!(refused(r"http://127.0.0.1:1234\@evil.example/"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:1234/v1 x"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:1234/../v1"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:0/v1"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:99999/v1"), EndpointError::Malformed);
    assert_eq!(refused("http://127.0.0.1:/v1"), EndpointError::Malformed);
    assert_eq!(refused("http://"), EndpointError::Malformed);
    assert_eq!(refused(""), EndpointError::Empty);
    assert_eq!(refused("   "), EndpointError::Empty);
}

#[test]
fn ai_help_is_off_and_empty_until_the_user_sets_it_up() {
    let settings = AiSettings::default();
    assert!(!settings.enabled);
    assert!(settings.endpoint.is_empty() && settings.model.is_empty());
    assert_eq!(settings.detail_level, DetailLevel::Summary);
    assert!(settings.ready_to_use().is_err(), "nothing is usable before it is configured");
}

#[test]
fn settings_become_usable_only_when_enabled_with_a_loopback_endpoint_and_a_model() {
    let mut settings = AiSettings {
        enabled: true,
        endpoint: "http://127.0.0.1:1234/v1".into(),
        model: "m".into(),
        ..AiSettings::default()
    };
    assert!(settings.ready_to_use().is_ok());

    settings.model = "  ".into();
    assert!(settings.ready_to_use().is_err(), "the model name is sent verbatim, so an empty one is not allowed");
    settings.model = "m".into();
    settings.endpoint = "http://example.com/v1".into();
    assert!(settings.ready_to_use().is_err());
    settings.endpoint = "http://127.0.0.1:1234/v1".into();
    settings.enabled = false;
    assert!(settings.ready_to_use().is_err());
}

#[test]
fn old_settings_files_without_ai_values_still_load() {
    let settings: AiSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
    assert!(settings.enabled);
    assert_eq!(settings.detail_level, DetailLevel::Summary);
}

#[test]
fn the_chat_body_asks_for_a_streamed_answer_with_the_model_name_as_given() {
    let request = ChatRequest {
        model: "My Model 7B".into(),
        messages: vec![
            ChatMessage { role: Role::System, content: "be careful".into() },
            ChatMessage { role: Role::User, content: "what is this?".into() },
        ],
        max_tokens: Some(800),
        temperature: 0.2,
    };
    let body = chat_body(&request);
    assert_eq!(body["model"], "My Model 7B");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 800);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["content"], "what is this?");
    assert!((body["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-9);
}

#[test]
fn the_chat_body_leaves_out_an_unset_length_limit() {
    let request = ChatRequest { model: "m".into(), messages: vec![], max_tokens: None, temperature: 0.0 };
    assert!(chat_body(&request).get("max_tokens").is_none());
}

#[test]
fn a_streamed_line_with_text_gives_an_answer_event() {
    let line = r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#;
    assert_eq!(parse_stream_line(line), StreamLine::Events(vec![StreamEvent::Answer("Hello".into())]));
    // Some servers write no space after the colon.
    let line = r#"data:{"choices":[{"delta":{"content":" world"}}]}"#;
    assert_eq!(parse_stream_line(line), StreamLine::Events(vec![StreamEvent::Answer(" world".into())]));
}

#[test]
fn reasoning_text_is_kept_apart_from_the_answer() {
    let line = r#"data: {"choices":[{"delta":{"reasoning_content":"thinking","content":null}}]}"#;
    assert_eq!(parse_stream_line(line), StreamLine::Events(vec![StreamEvent::Reasoning("thinking".into())]));
    let line = r#"data: {"choices":[{"delta":{"reasoning":"hm","content":"ok"}}]}"#;
    assert_eq!(
        parse_stream_line(line),
        StreamLine::Events(vec![StreamEvent::Reasoning("hm".into()), StreamEvent::Answer("ok".into())])
    );
}

#[test]
fn the_end_marker_and_the_lines_that_carry_nothing_are_recognised() {
    assert_eq!(parse_stream_line("data: [DONE]"), StreamLine::Done);
    assert_eq!(parse_stream_line("data:[DONE]"), StreamLine::Done);
    for quiet in [
        "",
        ": keep-alive",
        "event: message",
        r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#,
        r#"data: {"choices":[]}"#,
    ] {
        assert_eq!(parse_stream_line(quiet), StreamLine::Events(vec![]), "{quiet:?}");
    }
    assert_eq!(parse_stream_line(r#"data: {"choices":[{"delta":{"content":""}}]}"#), StreamLine::Events(vec![]));
}

#[test]
fn a_streamed_error_and_a_broken_line_are_reported_not_ignored() {
    assert_eq!(
        parse_stream_line(r#"data: {"error":{"message":"context too long"}}"#),
        StreamLine::Failed("context too long".into())
    );
    assert!(matches!(parse_stream_line("data: {not json"), StreamLine::Failed(_)));
}
