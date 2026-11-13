//! The Windows named pipe: a server for the collector service and a client for everyone else.
//!
//! Security properties (each exercised in `tests/pipe.rs` where it can be):
//! * an explicit access list: SYSTEM, Administrators and the named users only (`sddl`)
//! * `FILE_FLAG_FIRST_PIPE_INSTANCE`: starting fails if anything already owns the name, so the name cannot be squatted
//! * `PIPE_REJECT_REMOTE_CLIENTS`: local connections only
//! * a request size cap and a total per-connection deadline, so a silent or hostile client cannot hold resources
//! * a cap on concurrent connections

mod client;
mod server;
mod win;

pub use client::{request, ClientError};
pub use server::{PipeServer, PipeServerConfig};
pub use win::current_user_sid;

/// The full path of a pipe called `name`.
pub fn pipe_path(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}
