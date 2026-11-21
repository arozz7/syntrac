//! The scripted model server used by later slices behaves as its name says.

use syntrac_core::llm::{ChatMessage, ChatRequest, Flow, LlmError, LlmService, Role, ServerState, StreamEvent};
use syntrac_testkit::mock_llm::MockLlm;

fn request() -> ChatRequest {
    ChatRequest {
        model: "m".into(),
        messages: vec![ChatMessage { role: Role::User, content: "q".into() }],
        max_tokens: None,
        temperature: 0.0,
    }
}

#[test]
fn it_streams_its_pieces_and_remembers_what_it_was_asked() {
    let mock = MockLlm::answering(&["a", "b"]);
    let mut seen = Vec::new();
    let result = mock.stream_chat(&request(), &mut |event| {
        seen.push(event);
        Flow::Continue
    });
    assert_eq!(result, Ok(()));
    assert_eq!(seen, vec![StreamEvent::Answer("a".into()), StreamEvent::Answer("b".into())]);
    assert_eq!(mock.requests(), vec![request()]);
    assert_eq!(mock.state(), ServerState::Ready);
}

#[test]
fn it_stops_when_asked_and_can_be_made_to_fail() {
    let mock = MockLlm::answering(&["a", "b"]);
    let mut count = 0;
    mock.stream_chat(&request(), &mut |_| {
        count += 1;
        Flow::Stop
    })
    .unwrap();
    assert_eq!(count, 1);

    let down = MockLlm::failing(ServerState::NotRunning, LlmError::NotRunning);
    assert_eq!(down.state(), ServerState::NotRunning);
    assert_eq!(down.stream_chat(&request(), &mut |_| Flow::Continue), Err(LlmError::NotRunning));
}

#[test]
fn it_can_play_any_sequence_of_events() {
    let mock = MockLlm::playing(vec![
        StreamEvent::Reasoning("hm".into()),
        StreamEvent::Answer("a".into()),
        StreamEvent::CutOff,
    ]);
    let mut seen = Vec::new();
    mock.stream_chat(&request(), &mut |event| {
        seen.push(event);
        Flow::Continue
    })
    .unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2], StreamEvent::CutOff);
}
