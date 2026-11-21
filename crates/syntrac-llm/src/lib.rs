//! HTTP adapter for the optional AI help (docs/PHASE4-PLAN.md, slice L4a). It fills the `LlmService` port with a blocking client that talks
//! only to an address already proven to be on this machine (`LoopbackEndpoint`). It follows no redirects, uses no proxy, sends no credentials
//! and reads at most a bounded answer.

use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use syntrac_core::llm::{
    chat_body, parse_stream_line, ChatRequest, Flow, LlmError, LlmService, LoopbackEndpoint, ServerState, StreamEvent,
    StreamLine,
};

/// The limits of one adapter. The defaults suit a slow local model: a long wait for the first words, a bounded answer.
#[derive(Debug, Clone, Copy)]
pub struct HttpLimits {
    /// Connecting to a server on this machine either works at once or not at all.
    pub connect_timeout: Duration,
    /// The wait for the answer to begin: a large model may read a long question for minutes before the first word.
    pub first_byte_timeout: Duration,
    /// The longest a whole answer may take.
    pub answer_timeout: Duration,
    /// For the quick state probe.
    pub probe_timeout: Duration,
    /// Text of the answer and the reasoning together; more than this ends the read with `TooLong`.
    pub max_answer_bytes: usize,
    /// A single streamed line longer than this is refused (a well-formed one is a few hundred bytes).
    pub max_line_bytes: u64,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            first_byte_timeout: Duration::from_secs(600),
            answer_timeout: Duration::from_secs(1800),
            probe_timeout: Duration::from_secs(3),
            max_answer_bytes: 64 * 1024,
            max_line_bytes: 1024 * 1024,
        }
    }
}

pub struct HttpLlm {
    endpoint: LoopbackEndpoint,
    limits: HttpLimits,
}

impl HttpLlm {
    pub fn new(endpoint: LoopbackEndpoint, limits: HttpLimits) -> Self {
        Self { endpoint, limits }
    }

    fn agent(&self, first_byte: Duration, whole: Duration) -> ureq::Agent {
        ureq::Agent::config_builder()
            // A status such as 503 is data here (the server is loading), not a transport failure.
            .http_status_as_error(false)
            // A redirect could send the request on to an address that is not this machine.
            .max_redirects(0)
            // Never go through a proxy named in the environment.
            .proxy(None)
            .timeout_connect(Some(self.limits.connect_timeout))
            .timeout_recv_response(Some(first_byte))
            .timeout_global(Some(whole))
            .build()
            .into()
    }

    fn probe(&self, url: &str) -> Result<u16, ureq::Error> {
        let agent = self.agent(self.limits.probe_timeout, self.limits.probe_timeout);
        agent.get(url).call().map(|response| response.status().as_u16())
    }
}

impl LlmService for HttpLlm {
    fn state(&self) -> ServerState {
        // Servers keep a health answer at the root; those that do not are asked for their model list under the API path.
        let first = match self.probe(&self.endpoint.origin_url("/health")) {
            Ok(200) => return ServerState::Ready,
            Ok(503) => return ServerState::Loading,
            Ok(_) => self.probe(&self.endpoint.api_url("/models")),
            Err(error) => Err(error),
        };
        match first {
            Ok(200) => ServerState::Ready,
            Ok(503) => ServerState::Loading,
            Ok(status) => ServerState::Failed(format!("the server answered with status {status}")),
            Err(error) => match failure_of(&error) {
                LlmError::NotRunning => ServerState::NotRunning,
                other => ServerState::Failed(other.to_string()),
            },
        }
    }

    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent) -> Flow,
    ) -> Result<(), LlmError> {
        let agent = self.agent(self.limits.first_byte_timeout, self.limits.answer_timeout);
        let body = chat_body(request).to_string();
        let mut response = agent
            .post(&self.endpoint.api_url("/chat/completions"))
            .content_type("application/json")
            .send(body.as_str())
            .map_err(|error| failure_of(&error))?;

        let status = response.status().as_u16();
        if status == 503 {
            let retry_after_secs = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|text| text.trim().parse().ok());
            return Err(LlmError::Loading { retry_after_secs });
        }
        if status != 200 {
            return Err(LlmError::Status(status));
        }
        read_stream(response.body_mut().as_reader(), self.limits, on_event)
    }
}

/// Reads server-sent lines until the end marker, the end of the data, a failure, or a request to stop.
fn read_stream(
    source: impl Read,
    limits: HttpLimits,
    on_event: &mut dyn FnMut(StreamEvent) -> Flow,
) -> Result<(), LlmError> {
    let mut reader = BufReader::new(source);
    let mut delivered_bytes = 0usize;
    loop {
        let mut raw = Vec::new();
        // One line at most `max_line_bytes` long: a longer one is cut here and then fails to read as a whole line.
        let read = (&mut reader)
            .take(limits.max_line_bytes)
            .read_until(b'\n', &mut raw)
            .map_err(|error| io_failure(&error))?;
        if read == 0 {
            return Ok(());
        }
        if raw.last() != Some(&b'\n') && read as u64 >= limits.max_line_bytes {
            return Err(LlmError::BadResponse("a line from the server was too long".to_owned()));
        }
        match parse_stream_line(&String::from_utf8_lossy(&raw)) {
            StreamLine::Done => return Ok(()),
            StreamLine::Failed(message) => return Err(LlmError::BadResponse(message)),
            StreamLine::Events(events) => {
                for event in events {
                    delivered_bytes += text_of(&event).len();
                    if delivered_bytes > limits.max_answer_bytes {
                        return Err(LlmError::TooLong);
                    }
                    if on_event(event) == Flow::Stop {
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn text_of(event: &StreamEvent) -> &str {
    match event {
        StreamEvent::Answer(text) | StreamEvent::Reasoning(text) => text,
    }
}

fn failure_of(error: &ureq::Error) -> LlmError {
    match error {
        ureq::Error::Timeout(_) => LlmError::TimedOut,
        ureq::Error::ConnectionFailed => LlmError::NotRunning,
        ureq::Error::Io(io) => io_failure(io),
        other => LlmError::Connection(other.to_string()),
    }
}

fn io_failure(error: &std::io::Error) -> LlmError {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => LlmError::NotRunning,
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => LlmError::TimedOut,
        _ => LlmError::Connection(error.to_string()),
    }
}
