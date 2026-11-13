//! The pipe client: send one request, read one response, with a hard overall timeout.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::pipe_path;
use crate::protocol::{decode_response, encode_request, Request, Response, MAX_RESPONSE_BYTES};

/// Windows error code for "all pipe instances are busy": try again shortly.
const ERROR_PIPE_BUSY: i32 = 231;
const BUSY_RETRY: Duration = Duration::from_millis(20);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// No such pipe: the service is not running (or not installed).
    #[error("the collector is not running")]
    NotRunning,
    /// The pipe exists but this user is not allowed to use it.
    #[error("access to the collector was denied")]
    AccessDenied,
    #[error("the collector did not answer in time")]
    Timeout,
    #[error("pipe error: {0}")]
    Io(String),
    #[error("unexpected answer from the collector: {0}")]
    Protocol(String),
}

/// Sends `request` to the pipe called `name` and returns the response. Never blocks longer than `timeout`.
pub fn request(name: &str, request: &Request, timeout: Duration) -> Result<Response, ClientError> {
    let path = pipe_path(name);
    let line = encode_request(request);
    let deadline = Instant::now() + timeout;

    // The blocking I/O runs on a worker so the caller's timeout is honoured even if the server stalls.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(exchange(&path, &line, deadline));
    });
    receiver.recv_timeout(timeout).unwrap_or(Err(ClientError::Timeout))
}

fn exchange(path: &str, request_line: &str, deadline: Instant) -> Result<Response, ClientError> {
    let mut pipe = loop {
        match OpenOptions::new().read(true).write(true).open(path) {
            Ok(pipe) => break pipe,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(ClientError::NotRunning),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(ClientError::AccessDenied)
            }
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                if Instant::now() >= deadline {
                    return Err(ClientError::Timeout);
                }
                thread::sleep(BUSY_RETRY);
            }
            Err(error) => return Err(ClientError::Io(error.to_string())),
        }
    };

    pipe.write_all(request_line.as_bytes()).map_err(|e| ClientError::Io(e.to_string()))?;

    let mut answer = Vec::new();
    BufReader::new((&mut pipe).take(MAX_RESPONSE_BYTES as u64 + 1))
        .read_until(b'\n', &mut answer)
        .map_err(|e| ClientError::Io(e.to_string()))?;
    decode_response(&answer).map_err(|e| ClientError::Protocol(e.to_string()))
}
