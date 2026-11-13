//! Stable process identity that survives PID reuse.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::time::Timestamp;

/// Fixed namespace so ids are reproducible across runs and machines.
const PROCESS_NAMESPACE: Uuid = Uuid::from_u128(0x5359_4e54_5241_4300_8000_0000_0000_0001);

/// Identifies one *instance* of a process, never just a PID.
///
/// Windows reuses PIDs, so the id is derived from `(pid, creation time)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProcessInstanceId(Uuid);

impl ProcessInstanceId {
    pub fn from_pid_and_creation(pid: u32, created: Timestamp) -> Self {
        let name = format!("{pid}:{}", created.as_micros());
        Self(Uuid::new_v5(&PROCESS_NAMESPACE, name.as_bytes()))
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}
