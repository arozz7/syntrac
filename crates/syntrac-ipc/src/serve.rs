//! Serving one connection. Generic over `Read + Write`, so the same logic runs over a real pipe (with timeouts)
//! and over in-memory streams in tests.

use std::io::{self, Read, Write};

use crate::protocol::{decode_request, encode_response, Request, Response, MAX_REQUEST_BYTES};

/// Answers requests. Implemented by the collector's status board.
pub trait Handler: Send + Sync {
    fn handle(&self, request: Request) -> Response;
}

enum Line {
    /// The client connected and closed without sending anything.
    Closed,
    TooLarge,
    Data(Vec<u8>),
}

/// Reads at most one request line and writes one response. Only the first request on a connection is served.
///
/// Reading stops as soon as the size cap is exceeded, so a hostile client cannot make the service buffer an
/// unbounded amount of data. A read error (for example a timeout) is returned so the caller can drop the connection.
pub fn serve_connection<S: Read + Write>(stream: &mut S, handler: &dyn Handler) -> io::Result<()> {
    let response = match read_request_line(stream)? {
        Line::Closed => return Ok(()),
        Line::TooLarge => Response::Error { message: format!("request too large (limit {MAX_REQUEST_BYTES} bytes)") },
        Line::Data(bytes) => match decode_request(&bytes) {
            Ok(request) => handler.handle(request),
            Err(error) => Response::Error { message: error.to_string() },
        },
    };
    stream.write_all(encode_response(&response).as_bytes())?;
    stream.flush()
}

fn read_request_line<R: Read>(reader: &mut R) -> io::Result<Line> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(if data.is_empty() { Line::Closed } else { finish(data) });
        }
        if let Some(newline) = chunk[..read].iter().position(|byte| *byte == b'\n') {
            data.extend_from_slice(&chunk[..newline]);
            return Ok(finish(data));
        }
        data.extend_from_slice(&chunk[..read]);
        if data.len() > MAX_REQUEST_BYTES {
            return Ok(Line::TooLarge);
        }
    }
}

fn finish(data: Vec<u8>) -> Line {
    if data.len() > MAX_REQUEST_BYTES {
        Line::TooLarge
    } else {
        Line::Data(data)
    }
}
