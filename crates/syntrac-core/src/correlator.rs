//! Resolves raw PIDs to stable process instances and maintains process lifetimes.

use crate::event::{EventEnvelope, EventPayload};
use crate::identity::ProcessInstanceId;
use crate::ports::{ProcessRecord, ProcessRepository, RepoError};

/// Attributes `event` to a process instance, updating the process table as needed.
///
/// Events must be fed in timestamp order. An event whose PID is unknown (for example a
/// connection that predates monitoring) is returned unattributed, never guessed.
pub fn correlate<R: ProcessRepository + ?Sized>(
    repo: &mut R,
    mut event: EventEnvelope,
) -> Result<EventEnvelope, RepoError> {
    let owner = match &event.payload {
        EventPayload::ProcessStarted { pid, parent_pid, image_path, command_line } => {
            // The start event's timestamp stands in for the creation time.
            let id = ProcessInstanceId::from_pid_and_creation(*pid, event.ts);
            let parent_id = repo.resolve(*parent_pid, event.ts)?;
            repo.record_start(ProcessRecord {
                id,
                pid: *pid,
                parent_pid: *parent_pid,
                parent_id,
                image_path: image_path.clone(),
                command_line: command_line.clone(),
                started: event.ts,
                ended: None,
                exit_code: None,
            })?;
            Some(id)
        }
        EventPayload::ProcessExited { pid, exit_code } => {
            let owner = repo.resolve(*pid, event.ts)?;
            if let Some(id) = owner {
                repo.record_exit(id, event.ts, *exit_code)?;
            }
            owner
        }
        other => repo.resolve(other.pid(), event.ts)?,
    };
    event.process_instance_id = owner;
    Ok(event)
}
