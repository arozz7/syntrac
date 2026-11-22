//! A tiny scripted HTTP server on loopback for the adapter tests. Each connection gets one request and one scripted reply, then is closed.
#![allow(dead_code)] // Shared by several test files, each of which uses a different part.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Seen {
    pub request_line: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

pub struct Reply {
    pub raw: String,
    pub delay: Duration,
}

pub fn reply(status_line: &str, extra_headers: &str, body: &str) -> Reply {
    Reply {
        raw: format!("HTTP/1.1 {status_line}\r\n{extra_headers}Connection: close\r\n\r\n{body}"),
        delay: Duration::ZERO,
    }
}

pub fn sse(lines: &[&str]) -> Reply {
    let body: String = lines.iter().map(|line| format!("{line}\n\n")).collect();
    reply("200 OK", "Content-Type: text/event-stream\r\n", &body)
}

pub struct FakeServer {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeServer {
    pub fn start(script: impl Fn(&Seen) -> Reply + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let Some(request) = read_request(&stream) else { continue };
                let reply = script(&request);
                record.lock().unwrap().push(request);
                thread::sleep(reply.delay);
                let mut stream = stream;
                // The client may have gone away already (a cancelled answer); that is not an error here.
                let _ = stream.write_all(reply.raw.as_bytes());
                let _ = stream.flush();
            }
        });
        Self { port, seen }
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// A loopback port nothing listens on.
pub fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn read_request(stream: &TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (key, value) = line.split_once(':')?;
        headers.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Seen {
        request_line: request_line.trim_end().to_owned(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}
