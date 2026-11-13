//! SYNTRAC core: domain types, interfaces and pure logic. No I/O, no vendor imports.

pub mod correlator;
pub mod event;
pub mod identity;
pub mod ports;
pub mod time;

mod pipeline;

pub use correlator::correlate;
pub use event::{EventCategory, EventEnvelope, EventPayload, NetProtocol, Severity};
pub use identity::ProcessInstanceId;
pub use pipeline::{ingest, IngestError};
pub use ports::{
    EventQuery, EventRepository, ProcessRecord, ProcessRepository, RepoError, SourceError, TelemetrySource,
};
pub use time::{Clock, Timestamp};
