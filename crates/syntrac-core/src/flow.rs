//! Folds raw network activity into per-window flows.
//!
//! Flows are keyed by process INSTANCE (not bare PID), so a reused PID never inherits
//! another process's traffic. Windows are fixed, aligned buckets of the event timestamps.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use uuid::Uuid;

use crate::event::{EventEnvelope, EventPayload, NetAction, NetProtocol};
use crate::identity::ProcessInstanceId;
use crate::time::Timestamp;

/// A remote endpoint and how much it was talked to, summed over a time range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Destination {
    pub remote_addr: IpAddr,
    pub remote_port: u16,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flow_count: u32,
    /// Distinct process instances (or unattributed PIDs) that talked to it.
    pub process_count: u32,
}

/// Who a flow belongs to. Unattributed activity falls back to the PID the source reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Instance(ProcessInstanceId),
    Unattributed(u32),
}

/// `(window start in micros, owner, protocol, remote address, remote port)`; sorted by window first.
type FlowKey = (i64, Owner, NetProtocol, IpAddr, u16);

#[derive(Debug, Default)]
struct Totals {
    pid: u32,
    bytes_sent: u64,
    bytes_received: u64,
    opened: u32,
    closed: u32,
    failed: u32,
}

#[derive(Debug)]
pub struct FlowAggregator {
    window_micros: i64,
    open: BTreeMap<FlowKey, Totals>,
}

impl FlowAggregator {
    pub fn new(window: Duration) -> Self {
        let window_micros = i64::try_from(window.as_micros()).unwrap_or(i64::MAX).max(1);
        Self { window_micros, open: BTreeMap::new() }
    }

    /// Adds one correlated event. Anything that is not `NetworkActivity` is ignored.
    pub fn add(&mut self, event: &EventEnvelope) {
        let EventPayload::NetworkActivity { pid, protocol, action, remote_addr, remote_port, bytes } = &event.payload
        else {
            return;
        };

        let owner = event.process_instance_id.map_or(Owner::Unattributed(*pid), Owner::Instance);
        let bucket = event.ts.as_micros().div_euclid(self.window_micros) * self.window_micros;
        let totals = self.open.entry((bucket, owner, *protocol, *remote_addr, *remote_port)).or_default();

        totals.pid = *pid;
        let bytes = u64::from(*bytes);
        match action {
            NetAction::Send => totals.bytes_sent += bytes,
            NetAction::Receive => totals.bytes_received += bytes,
            NetAction::Connect | NetAction::Accept => totals.opened += 1,
            NetAction::Close => totals.closed += 1,
            NetAction::Fail => totals.failed += 1,
        }
    }

    /// Emits every window that has fully ended by `now` (driven by the injected clock).
    pub fn flush_due(&mut self, now: Timestamp) -> Vec<EventEnvelope> {
        let window = self.window_micros;
        let due: Vec<FlowKey> =
            self.open.keys().take_while(|key| key.0.saturating_add(window) <= now.as_micros()).copied().collect();
        self.take(due)
    }

    /// Emits all open windows. Call at shutdown so nothing is lost.
    pub fn flush_all(&mut self) -> Vec<EventEnvelope> {
        let all: Vec<FlowKey> = self.open.keys().copied().collect();
        self.take(all)
    }

    fn take(&mut self, keys: Vec<FlowKey>) -> Vec<EventEnvelope> {
        keys.into_iter().filter_map(|key| self.open.remove(&key).map(|totals| self.to_event(key, totals))).collect()
    }

    fn to_event(&self, key: FlowKey, totals: Totals) -> EventEnvelope {
        let (start, owner, protocol, remote_addr, remote_port) = key;
        let window_start = Timestamp::from_micros(start);
        EventEnvelope {
            id: Uuid::new_v4(),
            ts: window_start,
            process_instance_id: match owner {
                Owner::Instance(id) => Some(id),
                Owner::Unattributed(_) => None,
            },
            severity: None,
            payload: EventPayload::NetworkFlow {
                pid: totals.pid,
                protocol,
                remote_addr,
                remote_port,
                bytes_sent: totals.bytes_sent,
                bytes_received: totals.bytes_received,
                opened: totals.opened,
                closed: totals.closed,
                failed: totals.failed,
                window_start,
                window_end: Timestamp::from_micros(start.saturating_add(self.window_micros)),
            },
        }
    }
}
