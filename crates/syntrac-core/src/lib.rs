//! SYNTRAC core: domain types, interfaces and pure logic. No I/O, no vendor imports.

pub mod correlator;
pub mod event;
pub mod exe_identity;
pub mod exe_location;
pub mod flow;
pub mod identity;
pub mod identity_run;
pub mod metrics;
pub mod pipeline;
pub mod ports;
pub mod retention;
pub mod rollup;
pub mod rollup_ledger;
pub mod rollup_run;
pub mod silence;
pub mod storage_guard;
pub mod time;
pub mod trend;

pub use correlator::correlate;
pub use event::{EventCategory, EventEnvelope, EventPayload, NetAction, NetProtocol, Severity};
pub use exe_identity::{
    FileFacts, FileInspector, FileStamp, IdentityEntry, IdentityFailure, IdentityRecord, IdentityRepository,
    InspectError, InspectErrorKind, Signature, SignatureKind, SignatureStatus, VersionInfo,
};
pub use exe_location::{classify_location, install_root, product_key, LocationClass, ProductKey};
pub use flow::{Destination, FlowAggregator, FlowRow};
pub use identity::ProcessInstanceId;
pub use identity_run::{inspect_due, retry_after, IdentityPass, IdentitySettings};
pub use metrics::{
    CpuBreakdown, CpuContribution, MetricsSample, OtherProcesses, ProcessMetric, PurgeCounts, SystemMetrics,
    SystemPoint,
};
pub use pipeline::{ingest_metrics, IngestError, Pipeline};
pub use ports::{
    EventQuery, EventRepository, FlowQueries, MetricsRepository, MetricsSource, ProcessFilter, ProcessQueries,
    ProcessRecord, ProcessRepository, RepoError, Retention, SourceError, TelemetrySource,
};
pub use rollup::{Executable, HourInput, HourRollup, LedgerEntry, ProcessHour, SystemHour, TrafficHour};
pub use rollup_ledger::LedgerObservations;
pub use rollup_run::{roll_up_due, RollupPass, RollupRepository, RollupSettings};
pub use time::{Clock, SystemClock, Timestamp};
pub use trend::{
    collect_totals, rank_totals, top_programs, ProcessTotals, ProgramMetric, ProgramSummary, ProgramTotals,
    TrafficTotals, TrendRepository,
};
