//! The HTTP adapter against a scripted server on loopback (docs/PHASE4-PLAN.md, L4a).

mod fake_server;

use std::time::Duration;

use fake_server::{closed_port, reply, sse, FakeServer};
use syntrac_core::llm::{
    ChatMessage, ChatRequest, Flow, LlmError, LlmService, LoopbackEndpoint, Role, ServerState, StreamEvent,
};
use syntrac_llm::{HttpLimits, HttpLlm};

fn client(endpoint: &str) -> HttpLlm {
    client_with(endpoint, HttpLimits::default())
}

fn client_with(endpoint: &str, limits: HttpLimits) -> HttpLlm {
    HttpLlm::new(LoopbackEndpoint::parse(endpoint).unwrap(), limits)
}

fn question() -> ChatRequest {
    ChatRequest {
        model: "Some Model".into(),
        messages: vec![ChatMessage { role: Role::User, content: "what is this?".into() }],
        max_tokens: Some(100),
        temperature: 0.2,
    }
}

fn collect(client: &HttpLlm) -> (Vec<StreamEvent>, Result<(), LlmError>) {
    let mut events = Vec::new();
    let result = client.stream_chat(&question(), &mut |event| {
        events.push(event);
        Flow::Continue
    });
    (events, result)
}

#[test]
fn a_server_with_a_health_answer_is_ready() {
    let server = FakeServer::start(|seen| match seen.request_line.as_str() {
        "GET /health HTTP/1.1" => reply("200 OK", "", "{}"),
        _ => reply("404 Not Found", "", ""),
    });
    assert_eq!(client(&server.endpoint()).state(), ServerState::Ready);
}

#[test]
fn a_server_without_a_health_answer_is_ready_when_it_lists_its_models() {
    let server = FakeServer::start(|seen| match seen.request_line.as_str() {
        "GET /v1/models HTTP/1.1" => reply("200 OK", "", r#"{"data":[]}"#),
        _ => reply("404 Not Found", "", ""),
    });
    assert_eq!(client(&server.endpoint()).state(), ServerState::Ready);
}

#[test]
fn a_server_that_says_it_is_loading_is_loading() {
    let server = FakeServer::start(|_| reply("503 Service Unavailable", "Retry-After: 10\r\n", "loading"));
    assert_eq!(client(&server.endpoint()).state(), ServerState::Loading);
}

#[test]
fn nothing_listening_means_not_running() {
    let adapter = client(&format!("http://127.0.0.1:{}/v1", closed_port()));
    assert_eq!(adapter.state(), ServerState::NotRunning);
    assert_eq!(collect(&adapter).1, Err(LlmError::NotRunning));
}

#[test]
fn a_server_that_never_answers_times_out_instead_of_hanging() {
    let server = FakeServer::start(|_| {
        let mut slow = reply("200 OK", "", "{}");
        slow.delay = Duration::from_secs(3);
        slow
    });
    let limits = HttpLimits { probe_timeout: Duration::from_millis(300), ..HttpLimits::default() };
    let started = std::time::Instant::now();
    let state = client_with(&server.endpoint(), limits).state();
    assert!(matches!(state, ServerState::Failed(_)), "{state:?}");
    assert!(started.elapsed() < Duration::from_secs(2), "the probe took {:?}", started.elapsed());
}

#[test]
fn a_streamed_answer_arrives_piece_by_piece_with_the_reasoning_kept_apart() {
    let server = FakeServer::start(|_| {
        sse(&[
            r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"data: {"choices":[{"delta":{"reasoning_content":"hmm"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"It is "}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"a program."}}]}"#,
            "data: [DONE]",
        ])
    });
    let (events, result) = collect(&client(&server.endpoint()));
    assert_eq!(result, Ok(()));
    assert_eq!(
        events,
        vec![
            StreamEvent::Reasoning("hmm".into()),
            StreamEvent::Answer("It is ".into()),
            StreamEvent::Answer("a program.".into())
        ]
    );
}

#[test]
fn the_request_goes_to_the_chat_path_as_json_with_the_model_name_verbatim_and_no_credentials() {
    let server = FakeServer::start(|_| sse(&["data: [DONE]"]));
    collect(&client(&server.endpoint())).1.unwrap();
    let seen = server.requests();
    let request = seen.last().unwrap();
    assert_eq!(request.request_line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert!(request.header("authorization").is_none() && request.header("cookie").is_none());
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["model"], "Some Model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["content"], "what is this?");
}

#[test]
fn stopping_ends_the_read_after_the_piece_that_asked_for_it() {
    let server = FakeServer::start(|_| {
        sse(&[
            r#"data: {"choices":[{"delta":{"content":"one"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"two"}}]}"#,
            "data: [DONE]",
        ])
    });
    let mut events = Vec::new();
    let result = client(&server.endpoint()).stream_chat(&question(), &mut |event| {
        events.push(event);
        Flow::Stop
    });
    assert_eq!(result, Ok(()));
    assert_eq!(events, vec![StreamEvent::Answer("one".into())]);
}

#[test]
fn a_model_that_is_still_loading_gives_a_busy_error_with_the_wait_the_server_asked_for() {
    let server = FakeServer::start(|_| reply("503 Service Unavailable", "Retry-After: 7\r\n", "loading"));
    assert_eq!(collect(&client(&server.endpoint())).1, Err(LlmError::Loading { retry_after_secs: Some(7) }));
}

#[test]
fn other_error_statuses_are_reported_as_they_are() {
    let server = FakeServer::start(|_| reply("400 Bad Request", "", r#"{"error":"nope"}"#));
    assert_eq!(collect(&client(&server.endpoint())).1, Err(LlmError::Status(400)));
}

#[test]
fn a_redirect_is_never_followed() {
    // If it were, a server could send the program on to an address that is not this machine.
    let other = FakeServer::start(|_| sse(&[r#"data: {"choices":[{"delta":{"content":"leaked"}}]}"#, "data: [DONE]"]));
    let target = format!("http://127.0.0.1:{}/v1/chat/completions", other.port);
    let server = FakeServer::start(move |_| reply("307 Temporary Redirect", &format!("Location: {target}\r\n"), ""));
    let (events, result) = collect(&client(&server.endpoint()));
    assert_eq!(result, Err(LlmError::Status(307)));
    assert!(events.is_empty());
    assert!(other.requests().is_empty(), "the redirect target was contacted");
}

#[test]
fn an_error_inside_the_stream_is_reported() {
    let server = FakeServer::start(|_| sse(&[r#"data: {"error":{"message":"context too long"}}"#]));
    assert_eq!(collect(&client(&server.endpoint())).1, Err(LlmError::BadResponse("context too long".into())));
}

#[test]
fn an_answer_that_grows_past_the_limit_is_cut_off() {
    let server = FakeServer::start(|_| {
        sse(&[
            r#"data: {"choices":[{"delta":{"content":"0123456789"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"0123456789"}}]}"#,
            "data: [DONE]",
        ])
    });
    let limits = HttpLimits { max_answer_bytes: 15, ..HttpLimits::default() };
    let (events, result) = {
        let adapter = client_with(&server.endpoint(), limits);
        let mut events = Vec::new();
        let result = adapter.stream_chat(&question(), &mut |event| {
            events.push(event);
            Flow::Continue
        });
        (events, result)
    };
    assert_eq!(result, Err(LlmError::TooLong));
    assert_eq!(events.len(), 1, "the piece that crossed the limit is not delivered");
}

#[test]
fn a_stream_that_ends_without_the_end_marker_still_ends_cleanly_when_it_gave_text() {
    let server = FakeServer::start(|_| sse(&[r#"data: {"choices":[{"delta":{"content":"partial"}}]}"#]));
    let (events, result) = collect(&client(&server.endpoint()));
    assert_eq!(result, Ok(()));
    assert_eq!(events, vec![StreamEvent::Answer("partial".into())]);
}

#[test]
fn an_answer_cut_off_by_the_length_limit_is_reported_after_the_text_that_arrived() {
    let server = FakeServer::start(|_| {
        sse(&[
            r#"data: {"choices":[{"delta":{"content":"partial"}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"length"}]}"#,
            "data: [DONE]",
        ])
    });
    let (events, result) = collect(&client(&server.endpoint()));
    assert_eq!(result, Ok(()));
    assert_eq!(events, vec![StreamEvent::Answer("partial".into()), StreamEvent::CutOff]);
}
