//! Phase 4, slice L4c (docs/PHASE4-PLAN.md): starts and stops the model server the user set a launch command for, and reports its state.
//! It only ever stops a process it started itself, and it never starts a second server while one already answers. Starting a process sits
//! behind `ProcessSpawner`, so the logic runs in tests with a fake and no real program.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::llm::{LlmService, ServerState};
use crate::server_launch::{LaunchCommand, LaunchProblem};

/// A process the program started. Dropping the handle does not stop the process.
pub trait ChildHandle: Send {
    /// `None` while it runs, else its exit code (`-1` when Windows gave none).
    fn try_exit(&mut self) -> Result<Option<i32>, LaunchError>;
    /// Ends the process and everything it started.
    fn kill(&mut self) -> Result<(), LaunchError>;
}

/// Starts a program directly (no shell). The command has already passed `LaunchCommand::check`.
pub trait ProcessSpawner: Send + Sync {
    fn spawn(&self, command: &LaunchCommand) -> Result<Box<dyn ChildHandle>, LaunchError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LaunchError {
    #[error("the launch command is not allowed: {0}")]
    Refused(LaunchProblem),
    #[error("the program file was not found")]
    ProgramMissing,
    #[error("the working folder was not found")]
    FolderMissing,
    #[error("the program could not be started: {0}")]
    Spawn(String),
    #[error("a model server already answers; it was not started again")]
    AlreadyRunning,
    #[error("the server this app started is still running")]
    AlreadyStarted,
    #[error("this app did not start a server, so it will not stop one")]
    NotStartedByThisApp,
    #[error("the server could not be stopped: {0}")]
    Kill(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// Nothing answers and this app has not started anything.
    NotRunning,
    /// This app started it and it does not answer yet (a large model can take minutes to load).
    Starting {
        seconds: u64,
    },
    /// Still not answering after the startup limit; the process is still there and can be stopped.
    SlowToStart {
        seconds: u64,
    },
    /// A server answers. `started_by_this_app` tells whether Stop is available.
    Ready {
        started_by_this_app: bool,
    },
    /// Something answers that it is loading, and this app did not start it.
    Loading,
    /// The server this app started has ended.
    Exited {
        code: Option<i32>,
    },
    Failed(String),
}

struct Owned {
    handle: Box<dyn ChildHandle>,
    started: Instant,
}

#[derive(Default)]
struct Inner {
    child: Option<Owned>,
    /// How the last server this app started ended, until the next start or stop. `Some(None)` means no exit code.
    ended: Option<Option<i32>>,
}

pub struct ServerManager {
    spawner: Box<dyn ProcessSpawner>,
    startup_limit: Duration,
    inner: Mutex<Inner>,
}

impl ServerManager {
    pub fn new(spawner: Box<dyn ProcessSpawner>, startup_limit: Duration) -> Self {
        Self { spawner, startup_limit, inner: Mutex::new(Inner::default()) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts the server. Refused when the command breaks the rules, when this app's own server is still running, or when a server already
    /// answers (a second one would fight over the same port and memory).
    pub fn start(&self, command: &LaunchCommand, server: &dyn LlmService, now: Instant) -> Result<(), LaunchError> {
        command.check().map_err(LaunchError::Refused)?;
        // The probe is a network call that can be slow: it is made before the lock is taken, so a Start or a Stop never waits behind it.
        let answering = matches!(server.state(), ServerState::Ready | ServerState::Loading);
        let mut inner = self.lock();
        refresh(&mut inner);
        if inner.child.is_some() {
            return Err(LaunchError::AlreadyStarted);
        }
        if answering {
            return Err(LaunchError::AlreadyRunning);
        }
        let handle = self.spawner.spawn(command)?;
        inner.ended = None;
        inner.child = Some(Owned { handle, started: now });
        Ok(())
    }

    pub fn status(&self, server: &dyn LlmService, now: Instant) -> ServerStatus {
        // Probed before the lock is taken (see `start`).
        let probe = server.state();
        let mut inner = self.lock();
        refresh(&mut inner);
        if let Some(owned) = &inner.child {
            let seconds = now.saturating_duration_since(owned.started).as_secs();
            return match probe {
                ServerState::Ready => ServerStatus::Ready { started_by_this_app: true },
                ServerState::Failed(message) => ServerStatus::Failed(message),
                _ if Duration::from_secs(seconds) > self.startup_limit => ServerStatus::SlowToStart { seconds },
                _ => ServerStatus::Starting { seconds },
            };
        }
        match (probe, inner.ended) {
            (ServerState::Ready, _) => ServerStatus::Ready { started_by_this_app: false },
            (ServerState::Loading, _) => ServerStatus::Loading,
            (ServerState::Failed(message), _) => ServerStatus::Failed(message),
            (ServerState::NotRunning, Some(code)) => ServerStatus::Exited { code },
            (ServerState::NotRunning, None) => ServerStatus::NotRunning,
        }
    }

    /// Stops the server this app started, with everything it started. A server that was already running is never touched.
    pub fn stop(&self) -> Result<(), LaunchError> {
        let mut inner = self.lock();
        let Some(mut owned) = inner.child.take() else {
            return Err(LaunchError::NotStartedByThisApp);
        };
        inner.ended = None;
        owned.handle.kill()
    }

    /// For the end of the program: stops the server this app started when `stop_it` is set, otherwise leaves everything running.
    pub fn shutdown(&self, stop_it: bool) {
        if stop_it {
            // At the end of the program there is nobody left to tell about a failure.
            let _ = self.stop();
        }
    }
}

/// Notices that the server this app started has ended.
fn refresh(inner: &mut Inner) {
    let Some(owned) = &mut inner.child else { return };
    if let Ok(Some(code)) = owned.handle.try_exit() {
        inner.child = None;
        inner.ended = Some(Some(code).filter(|code| *code != -1));
    }
}
