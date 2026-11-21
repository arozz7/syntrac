//! A stand-in "model server" for trying the AI help on the demo machine (docs/PHASE4-PLAN.md). It speaks the small part of the common chat API
//! that SYNTRAC uses (`/health`, `/v1/models`, a streamed `/v1/chat/completions`) on loopback, and answers with a CANNED text that says plainly
//! it is not a language model. It exists so the whole flow (settings, Investigate, streaming, Stop, the copy box) can be exercised and
//! photographed with no real model and no real data. Every request body can be written to a log file, to check what a model server receives.
//!
//!   demo-model <port> [request-log-file]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// Pause between words, so the answer visibly streams and Stop has something to stop.
const WORD_PAUSE: Duration = Duration::from_millis(90);

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(port) = args.next().and_then(|text| text.parse::<u16>().ok()) else {
        eprintln!("usage: demo-model <port> [request-log-file]");
        std::process::exit(2);
    };
    let log = args.next();
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|error| {
        eprintln!("demo-model: cannot listen on 127.0.0.1:{port}: {error}");
        std::process::exit(1);
    });
    println!("demo-model: listening on 127.0.0.1:{port}");
    for stream in listener.incoming().flatten() {
        let log = log.clone();
        thread::spawn(move || {
            // A client that goes away in the middle is normal (Stop), so no error is reported.
            let _ = serve(stream, log.as_deref());
        });
    }
}

struct Request {
    method: String,
    path: String,
    body: String,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    let mut parts = first.split_whitespace();
    let (method, path) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok()?;
            }
        }
    }
    let mut body = vec![0u8; length.min(4 * 1024 * 1024)];
    reader.read_exact(&mut body).ok()?;
    Some(Request { method, path, body: String::from_utf8_lossy(&body).into_owned() })
}

fn serve(mut stream: TcpStream, log: Option<&str>) -> std::io::Result<()> {
    let Some(request) = read_request(&stream) else { return Ok(()) };
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => plain(&mut stream, "200 OK", r#"{"status":"ok"}"#),
        ("GET", "/v1/models") => plain(&mut stream, "200 OK", r#"{"data":[{"id":"demo-model"}]}"#),
        ("POST", "/v1/chat/completions") => {
            if let Some(file) = log {
                let mut out = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
                writeln!(out, "{}", request.body.replace('\n', " "))?;
            }
            stream_answer(&mut stream, &request.body)
        }
        _ => plain(&mut stream, "404 Not Found", ""),
    }
}

fn plain(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The value of `"key": "value"` in pretty-printed JSON text, the first time it appears.
fn quoted_value(text: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\": \"");
    let start = text.find(&marker)? + marker.len();
    let end = text[start..].find('"')?;
    Some(text[start..start + end].to_owned())
}

fn canned_answer(body: &str) -> String {
    let user_text = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["messages"].as_array()?.last()?["content"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let program = quoted_value(&user_text, "file_name").unwrap_or_else(|| "the program".to_owned());
    let place = quoted_value(&user_text, "location").unwrap_or_else(|| "an unknown place".to_owned());
    let signature = quoted_value(&user_text, "signature").unwrap_or_else(|| "unknown".to_owned());
    let kind = quoted_value(&user_text, "kind").unwrap_or_else(|| "this finding".to_owned());
    format!(
        "This is the demo stand-in model, not a language model, so it can only repeat what it was given.\n\n\
         It was asked about {program}. The evidence says it runs from {place}, its signature state is {signature}, \
         and the finding is of the kind {kind}.\n\n\
         A real model would say what that program probably is, what could be harmless, and what to check next. \
         Here, only the plumbing is being tested: the request arrived, the answer streamed back, and Stop works."
    )
}

fn stream_answer(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    )?;
    let send = |stream: &mut TcpStream, key: &str, text: &str| -> std::io::Result<()> {
        let chunk = serde_json::json!({ "choices": [{ "delta": { key: text } }] });
        write!(stream, "data: {chunk}\n\n")?;
        stream.flush()
    };
    // Reasoning first, as a thinking model does: it is counted by SYNTRAC, never shown as the answer.
    for word in ["Reading", "the", "evidence."] {
        send(stream, "reasoning_content", &format!("{word} "))?;
        thread::sleep(WORD_PAUSE);
    }
    for word in canned_answer(body).split_inclusive(' ') {
        send(stream, "content", word)?;
        thread::sleep(WORD_PAUSE);
    }
    write!(stream, "data: [DONE]\n\n")?;
    stream.flush()
}
