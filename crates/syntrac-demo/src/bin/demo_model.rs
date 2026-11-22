//! A stand-in "model server" for trying the AI help on the demo machine (docs/PHASE4-PLAN.md). It speaks the small part of the common chat API
//! that SYNTRAC uses (`/health`, `/v1/models`, a streamed `/v1/chat/completions`) on loopback, and answers with a CANNED text that says plainly
//! it is not a language model. A request that is not streamed and offers tools (the question box) gets a tool call first, and then, once a tool
//! result is in the conversation, an answer that copies the names out of that result, so the whole round trip can be checked. It exists so the whole flow (settings, Investigate, streaming, Stop, the copy box) can be exercised and
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
            if wants_whole_reply(&request.body) {
                let reply = turn_reply(&request.body).to_string();
                return plain(&mut stream, "200 OK", &reply);
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

/// A request with `"stream": false` wants one whole reply (a turn that may carry tools) instead of a stream.
fn wants_whole_reply(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body).is_ok_and(|request| request["stream"] == false)
}

/// The tool to ask for: the programs ranking when it is offered, else the first one.
fn tool_to_call(request: &serde_json::Value) -> Option<String> {
    let names: Vec<&str> =
        request["tools"].as_array()?.iter().filter_map(|tool| tool["function"]["name"].as_str()).collect();
    names.iter().find(|name| **name == "top_programs").or(names.first()).map(|name| (*name).to_owned())
}

/// The program names in a tool result (the data between the marked block's tags), as one text.
fn names_in_result(result: &str) -> String {
    let (Some(start), Some(end)) = (result.find('{'), result.rfind('}')) else {
        return "(nothing readable)".to_owned();
    };
    let names: Vec<String> = serde_json::from_str::<serde_json::Value>(&result[start..=end])
        .ok()
        .and_then(|data| data["programs"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|program| program["name"].as_str().map(str::to_owned))
        .collect();
    if names.is_empty() {
        "(no programs)".to_owned()
    } else {
        names.join(", ")
    }
}

fn message_reply(content: &str) -> serde_json::Value {
    serde_json::json!({ "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }] })
}

/// One whole reply: a tool call when tools are offered and no tool result is in the conversation yet; otherwise an answer, which copies the
/// names from the tool result when there is one.
fn turn_reply(body: &str) -> serde_json::Value {
    let request: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let tools_offered = request["tools"].as_array().is_some_and(|tools| !tools.is_empty());
    let result = request["messages"]
        .as_array()
        .and_then(|messages| messages.iter().rev().find(|message| message["role"] == "tool"))
        .and_then(|message| message["content"].as_str());
    match (tools_offered, result, tool_to_call(&request)) {
        (true, None, Some(name)) => serde_json::json!({ "choices": [{
            "message": { "role": "assistant", "content": null, "tool_calls": [{
                "id": "call_demo_1", "type": "function", "function": { "name": name, "arguments": "{}" } }] },
            "finish_reason": "tool_calls" }] }),
        (_, Some(result), _) => message_reply(&format!(
            "This is the demo stand-in model, not a language model, so it can only repeat what it was given.

             It asked for a lookup and was shown these programs: {}.

             A real model would explain them. Here, only the round trip is being tested: the tool call, its result, and this answer.",
            names_in_result(result)
        )),
        _ => message_reply("This is the demo stand-in model, not a language model. No tool result was given, so it has nothing to repeat."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn offered() -> serde_json::Value {
        json!([{ "type": "function", "function": { "name": "list_findings" } }, { "type": "function", "function": { "name": "top_programs" } }])
    }

    #[test]
    fn a_turn_with_tools_and_no_result_asks_for_the_ranking() {
        let body = json!({ "stream": false, "tools": offered(), "messages": [{ "role": "user", "content": "q" }] })
            .to_string();
        let reply = turn_reply(&body);
        let call = &reply["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(call["function"]["name"], "top_programs");
        assert_eq!(call["function"]["arguments"], "{}");
        assert_eq!(reply["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn a_turn_with_a_tool_result_answers_with_the_names_in_it() {
        let result = "<tool_result name=\"top_programs\">
{\"programs\":[{\"name\":\"alpha.exe\"},{\"name\":\"beta.exe\"}]}
</tool_result>";
        let body = json!({ "stream": false, "tools": offered(), "messages": [
            { "role": "user", "content": "q" },
            { "role": "assistant", "content": null },
            { "role": "tool", "tool_call_id": "call_demo_1", "content": result }] })
        .to_string();
        let answer = turn_reply(&body)["choices"][0]["message"]["content"].as_str().unwrap().to_owned();
        assert!(answer.contains("alpha.exe, beta.exe"), "{answer}");
        assert!(answer.contains("not a language model"));
    }

    #[test]
    fn the_last_turn_without_tools_still_answers_from_the_result() {
        let body = json!({ "stream": false, "messages": [
            { "role": "tool", "content": "{\"programs\":[{\"name\":\"gamma.exe\"}]}" }] })
        .to_string();
        assert!(turn_reply(&body)["choices"][0]["message"]["content"].as_str().unwrap().contains("gamma.exe"));
    }

    #[test]
    fn a_turn_with_nothing_to_repeat_says_so() {
        let body = json!({ "stream": false, "messages": [{ "role": "user", "content": "q" }] }).to_string();
        assert!(turn_reply(&body)["choices"][0]["message"]["content"].as_str().unwrap().contains("No tool result"));
    }

    #[test]
    fn only_a_request_that_says_it_is_not_streamed_wants_a_whole_reply() {
        assert!(wants_whole_reply(r#"{"stream":false}"#));
        assert!(!wants_whole_reply(r#"{"stream":true}"#));
        assert!(!wants_whole_reply("not json"));
    }
}
