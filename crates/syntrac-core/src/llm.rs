//! Phase 4, slice L4a (docs/PHASE4-PLAN.md): the optional AI help talks to a model server on THIS machine only. This module is the pure
//! part: the settings, the guard that accepts only a loopback address, the chat request body, the reading of one streamed line, and the
//! `LlmService` port that an adapter (a real HTTP client, or a fake in tests) fills in. No I/O and no vendor imports.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::server_launch::{LaunchCommand, LaunchProblem};

/// How much detail goes into what is asked of a model server on this machine (docs/PHASE4-PLAN.md, "What is sent").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetailLevel {
    /// Network owner and the name asked for, no addresses, ports or hashes.
    #[default]
    Summary,
    /// Adds addresses, ports, hashes and the account name. A deliberate choice.
    Full,
}

/// The user's AI help settings. Everything is off or empty until the user sets it up. Unknown or missing values fall back to these defaults,
/// so an older settings file still loads.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub enabled: bool,
    /// For example `http://127.0.0.1:1234/v1`. Checked by [`LoopbackEndpoint::parse`] whenever it is used.
    pub endpoint: String,
    /// Sent to the server exactly as typed: some servers switch models when the name changes, so it is never chosen for the user.
    pub model: String,
    pub detail_level: DetailLevel,
    /// Optional: how to start the model server from the app. Empty means the app does not manage the server.
    pub launch: Option<LaunchCommand>,
    /// Start the server when Investigate is pressed and nothing answers (otherwise the button says the server is not running).
    pub start_on_investigate: bool,
    /// Stop a server this app started when the app quits.
    pub stop_on_quit: bool,
}

impl AiSettings {
    /// The endpoint to use, or why the help cannot be used yet. The one gate every AI action passes through.
    pub fn ready_to_use(&self) -> Result<LoopbackEndpoint, SettingsProblem> {
        if !self.enabled {
            return Err(SettingsProblem::TurnedOff);
        }
        if self.model.trim().is_empty() {
            return Err(SettingsProblem::NoModel);
        }
        LoopbackEndpoint::parse(&self.endpoint).map_err(SettingsProblem::Endpoint)
    }

    /// The launch command, when one is set and passes the rules.
    pub fn launch_command(&self) -> Result<&LaunchCommand, LaunchProblem> {
        let command = self.launch.as_ref().ok_or(LaunchProblem::NoProgram)?;
        command.check()?;
        Ok(command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsProblem {
    #[error("AI help is turned off")]
    TurnedOff,
    #[error("no model name is set")]
    NoModel,
    #[error("the server address is not usable: {0}")]
    Endpoint(EndpointError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EndpointError {
    #[error("no address is set")]
    Empty,
    #[error("the address must start with http:// (https is not used for a server on this machine)")]
    NotHttp,
    #[error("the address is not written as host, optional port and optional path")]
    Malformed,
    #[error("the address is not this machine (use 127.0.0.1, localhost or [::1])")]
    NotThisMachine,
}

/// A server address that is known to be on this machine. The only way to get one is [`LoopbackEndpoint::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackEndpoint {
    /// Written the way it goes into a URL: `127.0.0.1`, or `[::1]`. `localhost` is stored as `127.0.0.1` so no name is ever resolved.
    host: String,
    port: u16,
    /// Starts with `/` or is empty; never ends with `/`.
    base_path: String,
}

impl LoopbackEndpoint {
    pub fn parse(text: &str) -> Result<Self, EndpointError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(EndpointError::Empty);
        }
        if text.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\\' | '?' | '#' | '@')) {
            return Err(EndpointError::Malformed);
        }
        let rest = strip_scheme(text).ok_or(EndpointError::NotHttp)?;
        let (authority, path) = match rest.find('/') {
            Some(slash) => rest.split_at(slash),
            None => (rest, ""),
        };
        let (host, port) = split_host_port(authority)?;
        let base_path = clean_path(path)?;
        Ok(Self { host: loopback_host(host)?, port, base_path })
    }

    /// A URL under the API path, for example `api_url("/chat/completions")` gives `http://127.0.0.1:1234/v1/chat/completions`.
    pub fn api_url(&self, suffix: &str) -> String {
        format!("{}{}{}", self.origin(), self.base_path, suffix)
    }

    /// A URL at the server's root, for example `/health`, which servers keep outside the API path.
    pub fn origin_url(&self, suffix: &str) -> String {
        format!("{}{}", self.origin(), suffix)
    }

    fn origin(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

fn strip_scheme(text: &str) -> Option<&str> {
    let (scheme, rest) = text.split_once("://")?;
    scheme.eq_ignore_ascii_case("http").then_some(rest)
}

fn split_host_port(authority: &str) -> Result<(&str, u16), EndpointError> {
    let (host, port_text) = if let Some(inner) = authority.strip_prefix('[') {
        let (address, after) = inner.split_once(']').ok_or(EndpointError::Malformed)?;
        let port_text = match after {
            "" => None,
            after => Some(after.strip_prefix(':').ok_or(EndpointError::Malformed)?),
        };
        (&authority[..address.len() + 2], port_text)
    } else {
        match authority.split_once(':') {
            Some((host, port_text)) => (host, Some(port_text)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(EndpointError::Malformed);
    }
    let port = match port_text {
        None => 80,
        Some(text) => text.parse::<u16>().ok().filter(|port| *port != 0).ok_or(EndpointError::Malformed)?,
    };
    Ok((host, port))
}

/// The address as it goes into a URL, or why it is not this machine.
fn loopback_host(host: &str) -> Result<String, EndpointError> {
    if host.eq_ignore_ascii_case("localhost") {
        return Ok("127.0.0.1".to_owned());
    }
    if host == "[::1]" {
        return Ok(host.to_owned());
    }
    match host.parse::<Ipv4Addr>() {
        Ok(address) if address.is_loopback() => Ok(host.to_owned()),
        _ => Err(EndpointError::NotThisMachine),
    }
}

fn clean_path(path: &str) -> Result<String, EndpointError> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.split('/').any(|segment| segment == "..") {
        return Err(EndpointError::Malformed);
    }
    Ok(trimmed.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// `None` leaves the length to the server.
    pub max_tokens: Option<u32>,
    pub temperature: f64,
}

/// The JSON body of a streamed chat request in the OpenAI-compatible form that local servers accept.
pub fn chat_body(request: &ChatRequest) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": request.model,
        "messages": request.messages,
        "stream": true,
        "temperature": request.temperature,
    });
    if let Some(limit) = request.max_tokens {
        body["max_tokens"] = limit.into();
    }
    body
}

/// A piece of a streamed answer. Reasoning text (from models that think first) is kept apart so it is never shown as the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Answer(String),
    Reasoning(String),
}

/// What one line of a streamed (server-sent events) response means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamLine {
    /// Zero or more pieces of text; blank lines, comments and role-only chunks give none.
    Events(Vec<StreamEvent>),
    Done,
    /// The server reported an error inside the stream, or the line could not be understood.
    Failed(String),
}

pub fn parse_stream_line(line: &str) -> StreamLine {
    let Some(data) = line.trim_end().strip_prefix("data:") else {
        return StreamLine::Events(Vec::new());
    };
    let data = data.trim_start();
    if data == "[DONE]" {
        return StreamLine::Done;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
        return StreamLine::Failed("the server sent a line that is not JSON".to_owned());
    };
    if let Some(error) = value.get("error") {
        let message = error.get("message").and_then(|m| m.as_str()).or_else(|| error.as_str());
        return StreamLine::Failed(message.unwrap_or("the server reported an error").to_owned());
    }
    let delta = value.get("choices").and_then(|choices| choices.get(0)).and_then(|choice| choice.get("delta"));
    let text_of = |key: &str| {
        delta.and_then(|d| d.get(key)).and_then(|t| t.as_str()).filter(|t| !t.is_empty()).map(str::to_owned)
    };
    let mut events = Vec::new();
    if let Some(text) = text_of("reasoning_content").or_else(|| text_of("reasoning")) {
        events.push(StreamEvent::Reasoning(text));
    }
    if let Some(text) = text_of("content") {
        events.push(StreamEvent::Answer(text));
    }
    StreamLine::Events(events)
}

/// Whether to keep reading a streamed answer. The caller answers after each piece, so Cancel takes effect between pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// What a probe of the server found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    NotRunning,
    /// Running but not ready (a large model can take minutes to load).
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmError {
    #[error("the model server is not running")]
    NotRunning,
    #[error("the model server is busy loading a model")]
    Loading { retry_after_secs: Option<u64> },
    #[error("the model server answered with status {0}")]
    Status(u16),
    #[error("the model server did not answer in time")]
    TimedOut,
    #[error("the answer was cut off because it grew too long")]
    TooLong,
    #[error("the model server sent something unusable: {0}")]
    BadResponse(String),
    #[error("the connection to the model server failed: {0}")]
    Connection(String),
}

/// The port behind which a model server sits: a real HTTP adapter, or a fake in tests. Blocking; callers run it off the UI thread.
pub trait LlmService: Send + Sync {
    /// A quick probe, never longer than a few seconds.
    fn state(&self) -> ServerState;

    /// Sends the request and calls `on_event` for each piece of the answer as it arrives, until the answer ends or `on_event` says stop.
    fn stream_chat(&self, request: &ChatRequest, on_event: &mut dyn FnMut(StreamEvent) -> Flow)
        -> Result<(), LlmError>;
}
