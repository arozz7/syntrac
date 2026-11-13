//! The real named pipe, in-process. Each test uses its own pipe name.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use syntrac_ipc::pipe::{pipe_path, request, ClientError, PipeServer, PipeServerConfig};
use syntrac_ipc::protocol::{decode_response, CollectorTotals, Request, Response, StatusReport};
use syntrac_ipc::serve::Handler;

const TIMEOUT: Duration = Duration::from_secs(5);

fn unique_name() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("syntrac-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst))
}

struct Fixed;

impl Handler for Fixed {
    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong { version: "test".into() },
            Request::Status => Response::Status(StatusReport {
                version: "test".into(),
                pid: 7,
                started_at_micros: 1,
                uptime_secs: 2,
                database_path: "db".into(),
                etw_enabled: false,
                last_tick_at_micros: None,
                totals: CollectorTotals { ticks: 3, ..CollectorTotals::default() },
                etw: None,
                storage: None,
            }),
        }
    }
}

fn start(name: &str, read_timeout: Duration) -> PipeServer {
    let mut config = PipeServerConfig::new(name);
    config.read_timeout = read_timeout;
    PipeServer::start(config, Arc::new(Fixed)).expect("server starts")
}

#[test]
fn a_ping_round_trips_through_a_real_pipe() {
    let name = unique_name();
    let _server = start(&name, Duration::from_secs(3));
    assert_eq!(request(&name, &Request::Ping, TIMEOUT).unwrap(), Response::Pong { version: "test".into() });
}

#[test]
fn a_status_request_carries_the_report() {
    let name = unique_name();
    let _server = start(&name, Duration::from_secs(3));
    match request(&name, &Request::Status, TIMEOUT).unwrap() {
        Response::Status(report) => assert_eq!((report.pid, report.totals.ticks), (7, 3)),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn many_sequential_requests_all_succeed_so_instances_are_recycled_correctly() {
    let name = unique_name();
    let _server = start(&name, Duration::from_secs(3));
    for round in 0..30 {
        assert!(request(&name, &Request::Ping, TIMEOUT).is_ok(), "request {round}");
    }
}

#[test]
fn concurrent_clients_are_all_served() {
    let name = unique_name();
    let _server = start(&name, Duration::from_secs(3));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let name = name.clone();
            thread::spawn(move || (0..5).all(|_| request(&name, &Request::Ping, TIMEOUT).is_ok()))
        })
        .collect();
    for worker in workers {
        assert!(worker.join().unwrap());
    }
}

#[test]
fn a_second_server_cannot_take_over_a_name_that_is_already_in_use() {
    // FILE_FLAG_FIRST_PIPE_INSTANCE: if something else already owns the name (a squatter, or a duplicate service),
    // starting must FAIL rather than silently join or replace it.
    let name = unique_name();
    let _first = start(&name, Duration::from_secs(3));
    let second = PipeServer::start(PipeServerConfig::new(&name), Arc::new(Fixed));
    assert!(second.is_err(), "a second server on the same name must be refused");
}

#[test]
fn a_request_to_a_pipe_nobody_serves_reports_not_running() {
    let error = request(&unique_name(), &Request::Ping, TIMEOUT).unwrap_err();
    assert!(matches!(error, ClientError::NotRunning), "{error:?}");
}

#[test]
fn a_silent_client_does_not_block_others_and_is_dropped_after_the_timeout() {
    let name = unique_name();
    let _server = start(&name, Duration::from_millis(700));

    // Connect and say nothing.
    let mut silent = OpenOptions::new().read(true).write(true).open(pipe_path(&name)).unwrap();

    // Everyone else is served immediately in the meantime.
    let began = Instant::now();
    assert!(request(&name, &Request::Ping, TIMEOUT).is_ok());
    assert!(began.elapsed() < Duration::from_secs(2), "took {:?}", began.elapsed());

    // The server gives up on the silent client: its read ends (end of stream or error) instead of hanging.
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let mut byte = [0u8; 1];
        // The read returning at all (end of stream or an error) proves the server dropped us instead of hanging.
        let _ = silent.read(&mut byte);
        let _ = done.send(());
    });
    assert!(finished.recv_timeout(Duration::from_secs(4)).is_ok(), "the silent connection was never dropped");
}

#[test]
fn an_oversized_request_gets_an_error_and_the_server_keeps_working() {
    let name = unique_name();
    let _server = start(&name, Duration::from_secs(3));

    let path = pipe_path(&name);
    let (result, received) = mpsc::channel();
    thread::spawn(move || {
        let mut pipe = OpenOptions::new().read(true).write(true).open(path).unwrap();
        // 64 KiB with no newline: far over the cap. Write it all first, then read, like any real client does:
        // reading and writing the SAME synchronous handle from two threads would serialize and deadlock on Windows.
        // The server answers after the cap, then drains the rest, so the write completes.
        let junk = vec![b'x'; 64 * 1024];
        let _ = pipe.write_all(&junk);
        let mut line = String::new();
        let _ = BufReader::new(&mut pipe).read_line(&mut line);
        let _ = result.send(line);
    });
    let line = received.recv_timeout(Duration::from_secs(6)).expect("the server must answer an oversized request");
    match decode_response(line.trim().as_bytes()).unwrap() {
        Response::Error { message } => assert!(message.contains("too large"), "{message}"),
        other => panic!("unexpected {other:?}"),
    }
    assert!(request(&name, &Request::Ping, TIMEOUT).is_ok(), "the server must keep serving afterwards");
}

#[test]
fn garbage_gets_an_error_response_and_the_server_keeps_working() {
    let name = unique_name();
    let _server = start(&name, Duration::from_secs(3));
    let mut pipe = OpenOptions::new().read(true).write(true).open(pipe_path(&name)).unwrap();
    pipe.write_all(b"not json\n").unwrap();
    let mut line = String::new();
    BufReader::new(&mut pipe).read_line(&mut line).unwrap();
    assert!(matches!(decode_response(line.trim().as_bytes()).unwrap(), Response::Error { .. }));
    assert!(request(&name, &Request::Ping, TIMEOUT).is_ok());
}

#[test]
fn stopping_the_server_is_prompt_and_frees_the_name() {
    let name = unique_name();
    let mut server = start(&name, Duration::from_secs(3));
    assert!(request(&name, &Request::Ping, TIMEOUT).is_ok());

    let began = Instant::now();
    server.stop();
    assert!(began.elapsed() < Duration::from_secs(3), "stop took {:?}", began.elapsed());

    assert!(matches!(request(&name, &Request::Ping, TIMEOUT), Err(ClientError::NotRunning)));
    // And the name can be claimed again.
    let _again = start(&name, Duration::from_secs(3));
    assert!(request(&name, &Request::Ping, TIMEOUT).is_ok());
}
