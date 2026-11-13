//! The pipe server: an accept loop on one thread, one short-lived thread per connection.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tracing::{debug, error, warn};

use super::pipe_path;
use super::win::{create_instance, current_user_sid, wait_for_client, Event, OwnedHandle, PipeStream};
use crate::sddl::build_pipe_sddl;
use crate::serve::{serve_connection, Handler};

/// Retry delay when a fresh pipe instance cannot be created.
const RECREATE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct PipeServerConfig {
    /// Name only, not the full path (`syntrac-collector`, not `\\.\pipe\...`).
    pub name: String,
    /// Users (as SID strings) allowed to connect, in addition to SYSTEM, Administrators and the user this process
    /// runs as.
    pub allowed_sids: Vec<String>,
    /// Total time one connection may take, request to close. A silent or slow client is dropped after this.
    pub read_timeout: Duration,
    /// Connections served at once. Extra connections are closed immediately rather than queued.
    pub max_concurrent: usize,
}

impl PipeServerConfig {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            allowed_sids: Vec::new(),
            read_timeout: Duration::from_secs(3),
            max_concurrent: 8,
        }
    }
}

pub struct PipeServer {
    stop: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl PipeServer {
    /// Creates the first pipe instance synchronously, so a name that is already taken fails HERE with an error.
    pub fn start(config: PipeServerConfig, handler: Arc<dyn Handler>) -> io::Result<Self> {
        let own_sid = current_user_sid()?;
        let mut sids: Vec<&str> = config.allowed_sids.iter().map(String::as_str).collect();
        sids.push(&own_sid);
        let sddl = build_pipe_sddl(&sids).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;

        let path = pipe_path(&config.name);
        let first = create_instance(&path, &sddl, true)?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let accept_thread = thread::Builder::new()
            .name("syntrac-pipe-accept".into())
            .spawn(move || accept_loop(&config, &path, &sddl, first, &thread_stop, &handler))?;
        Ok(Self { stop, accept_thread: Some(accept_thread) })
    }

    /// Stops accepting and waits for the accept thread (within a quarter of a second). Safe to call twice.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Counts a connection as active until dropped.
struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn accept_loop(
    config: &PipeServerConfig,
    path: &str,
    sddl: &str,
    first: OwnedHandle,
    stop: &AtomicBool,
    handler: &Arc<dyn Handler>,
) {
    let active = Arc::new(AtomicUsize::new(0));
    let event = match Event::new() {
        Ok(event) => event,
        Err(error) => {
            error!(%error, "Pipe server could not create its wait event; no clients will be served");
            return;
        }
    };

    let mut instance = Some(first);
    while !stop.load(Ordering::SeqCst) {
        let current = match instance.take() {
            Some(current) => current,
            None => match create_instance(path, sddl, false) {
                Ok(next) => next,
                Err(error) => {
                    warn!(%error, "Pipe server could not create a new instance; retrying");
                    thread::sleep(RECREATE_DELAY);
                    continue;
                }
            },
        };

        match wait_for_client(&current, &event, stop) {
            Ok(true) => {
                if active.load(Ordering::SeqCst) >= config.max_concurrent {
                    warn!("Pipe server at its connection limit; refusing a client");
                    continue; // `current` is dropped, which closes the connection
                }
                active.fetch_add(1, Ordering::SeqCst);
                let guard = ActiveGuard(Arc::clone(&active));
                let handler = Arc::clone(handler);
                let timeout = config.read_timeout;
                let spawned = thread::Builder::new().name("syntrac-pipe-conn".into()).spawn(move || {
                    let _guard = guard;
                    handle_connection(current, timeout, handler.as_ref());
                });
                if let Err(error) = spawned {
                    warn!(%error, "Pipe server could not start a connection thread");
                }
            }
            Ok(false) => break, // stop requested
            Err(error) => {
                warn!(%error, "Waiting for a pipe client failed; recreating the instance");
                thread::sleep(RECREATE_DELAY);
            }
        }
    }
}

fn handle_connection(instance: OwnedHandle, lifetime: Duration, handler: &dyn Handler) {
    let mut stream = match PipeStream::new(instance, lifetime) {
        Ok(stream) => stream,
        Err(error) => {
            warn!(%error, "Could not set up a pipe connection");
            return;
        }
    };
    if let Err(error) = serve_connection(&mut stream, handler) {
        debug!(%error, "Dropped a pipe connection");
    }
    // Give the client time to read the answer before the connection is torn down.
    stream.wait_for_client_to_close();
    stream.disconnect();
}
