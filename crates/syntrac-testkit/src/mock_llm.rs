//! A scripted stand-in for a model server (docs/PHASE4-PLAN.md): the pieces it will "stream", the state it reports, and a record of what it was
//! asked. It lets the investigation logic run with no server and no network.

use std::sync::Mutex;

use syntrac_core::llm::{ChatRequest, Flow, LlmError, LlmService, ServerState, StreamEvent};

pub struct MockLlm {
    state: ServerState,
    outcome: Result<Vec<StreamEvent>, LlmError>,
    asked: Mutex<Vec<ChatRequest>>,
}

impl MockLlm {
    /// A server that is ready and streams these answer pieces.
    pub fn answering(pieces: &[&str]) -> Self {
        let events = pieces.iter().map(|piece| StreamEvent::Answer((*piece).to_owned())).collect();
        Self { state: ServerState::Ready, outcome: Ok(events), asked: Mutex::new(Vec::new()) }
    }

    /// A server that is in this state and fails every chat request with this error.
    pub fn failing(state: ServerState, error: LlmError) -> Self {
        Self { state, outcome: Err(error), asked: Mutex::new(Vec::new()) }
    }

    /// Every request received so far, oldest first.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

impl LlmService for MockLlm {
    fn state(&self) -> ServerState {
        self.state.clone()
    }

    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent) -> Flow,
    ) -> Result<(), LlmError> {
        self.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(request.clone());
        for event in self.outcome.clone()? {
            if on_event(event) == Flow::Stop {
                break;
            }
        }
        Ok(())
    }
}
