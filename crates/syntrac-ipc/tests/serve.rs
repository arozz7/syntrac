//! Request handling for one connection, proven over in-memory streams.

use std::io::{self, Cursor, Read, Write};

use syntrac_ipc::protocol::{decode_response, CollectorTotals, Request, Response, StatusReport, MAX_REQUEST_BYTES};
use syntrac_ipc::serve::{serve_connection, Handler};

struct FixedHandler;

impl Handler for FixedHandler {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into() },
            Request::Status => Response::Status(StatusReport {
                version: "test".into(),
                pid: 1,
                started_at_micros: 0,
                uptime_secs: 0,
                database_path: String::new(),
                etw_enabled: false,
                last_tick_at_micros: None,
                totals: CollectorTotals::default(),
                etw: None,
                storage: None,
            }),
            other => Response::Error { message: format!("not handled: {other:?}") },
        }
    }
}

/// A duplex stream: reads from `input`, collects what is written.
struct Duplex {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Duplex {
    fn new(input: &[u8]) -> Self {
        Self { input: Cursor::new(input.to_vec()), output: Vec::new() }
    }

    fn responses(&self) -> Vec<Response> {
        String::from_utf8(self.output.clone())
            .unwrap()
            .lines()
            .map(|line| decode_response(line.as_bytes()).unwrap())
            .collect()
    }
}

impl Read for Duplex {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for Duplex {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_ping_gets_a_pong() {
    let mut stream = Duplex::new(b"{\"request\":\"ping\"}\n");
    serve_connection(&mut stream, &FixedHandler).unwrap();
    assert_eq!(stream.responses(), vec![Response::Pong { version: "test".into() }]);
}

#[test]
fn a_status_request_gets_a_status() {
    let mut stream = Duplex::new(b"{\"request\":\"status\"}\n");
    serve_connection(&mut stream, &FixedHandler).unwrap();
    assert!(matches!(stream.responses()[0], Response::Status(_)));
}

#[test]
fn a_request_without_a_trailing_newline_is_still_answered_at_end_of_stream() {
    let mut stream = Duplex::new(b"{\"request\":\"ping\"}");
    serve_connection(&mut stream, &FixedHandler).unwrap();
    assert_eq!(stream.responses().len(), 1);
}

#[test]
fn malformed_input_gets_an_error_response_and_no_panic() {
    let mut stream = Duplex::new(b"definitely not json\n");
    serve_connection(&mut stream, &FixedHandler).unwrap();
    assert!(matches!(stream.responses()[0], Response::Error { .. }));
}

#[test]
fn an_oversized_line_is_refused_without_reading_it_all() {
    let mut input = vec![b'x'; MAX_REQUEST_BYTES * 50];
    input.push(b'\n');
    let mut stream = Duplex::new(&input);
    serve_connection(&mut stream, &FixedHandler).unwrap();

    let responses = stream.responses();
    assert!(matches!(&responses[0], Response::Error { message } if message.contains("too large")));
    assert!(
        (stream.input.position() as usize) <= MAX_REQUEST_BYTES + 1024,
        "must stop reading near the cap, read {} bytes",
        stream.input.position()
    );
}

#[test]
fn a_client_that_connects_and_says_nothing_gets_no_response_and_no_error() {
    let mut stream = Duplex::new(b"");
    serve_connection(&mut stream, &FixedHandler).unwrap();
    assert!(stream.output.is_empty());
}

#[test]
fn only_the_first_request_on_a_connection_is_served() {
    let mut stream = Duplex::new(b"{\"request\":\"ping\"}\n{\"request\":\"status\"}\n");
    serve_connection(&mut stream, &FixedHandler).unwrap();
    assert_eq!(stream.responses().len(), 1);
}

/// A stream whose reads fail, like a pipe that timed out.
struct TimedOut;

impl Read for TimedOut {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::TimedOut, "no request in time"))
    }
}

impl Write for TimedOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_read_timeout_surfaces_as_an_error_for_the_caller_to_drop_the_connection() {
    let error = serve_connection(&mut TimedOut, &FixedHandler).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}
