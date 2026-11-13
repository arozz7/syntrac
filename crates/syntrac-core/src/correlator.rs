//! Resolves raw PIDs to stable process instances and maintains process lifetimes.

use crate::event::{EventEnvelope, EventPayload};
use crate::identity::ProcessInstanceId;
use crate::ports::{ProcessRecord, ProcessRepository, RepoError};
use crate::time::Timestamp;

/// Attributes `event` to a process instance, updating the process table as needed.
///
/// Events must be fed in timestamp order. An event whose process cannot be identified (a
/// connection that predates monitoring, or the exit of a process we never saw) is returned
/// unattributed and does NOT fail the batch.
pub fn correlate<R: ProcessRepository + ?Sized>(
    repo: &mut R,
    mut event: EventEnvelope,
) -> Result<EventEnvelope, RepoError> {
    let owner = match &event.payload {
        EventPayload::ProcessStarted { pid, parent_pid, created, image_path, command_line }
        | EventPayload::ProcessObserved { pid, parent_pid, created, image_path, command_line } => {
            Some(register(repo, *pid, *parent_pid, *created, image_path, command_line.as_deref())?)
        }
        EventPayload::ProcessExited { pid, created, exit_code } => {
            let id = ProcessInstanceId::from_pid_and_creation(*pid, *created);
            match repo.record_exit(id, event.ts, *exit_code) {
                Ok(()) => Some(id),
                Err(RepoError::NotFound) => None,
                Err(other) => return Err(other),
            }
        }
        other => repo.resolve(other.pid(), event.ts)?,
    };
    event.process_instance_id = owner;
    Ok(event)
}

fn register<R: ProcessRepository + ?Sized>(
    repo: &mut R,
    pid: u32,
    parent_pid: u32,
    created: Timestamp,
    image_path: &str,
    command_line: Option<&str>,
) -> Result<ProcessInstanceId, RepoError> {
    let id = ProcessInstanceId::from_pid_and_creation(pid, created);
    // The parent is whoever owned parent_pid when THIS process was created, not when we heard about it.
    let parent_id = repo.resolve(parent_pid, created)?;
    repo.record_start(ProcessRecord {
        id,
        pid,
        parent_pid,
        parent_id,
        image_path: image_path.to_owned(),
        command_line: command_line.map(str::to_owned),
        started: created,
        ended: None,
        ended_approximate: false,
        exit_code: None,
    })?;
    Ok(id)
}
