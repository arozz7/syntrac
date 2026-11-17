//! SYNTRAC core: domain types, interfaces and pure logic. No I/O, no vendor imports.

pub mod addr_class;
pub mod clean_text;
pub mod context_run;
pub mod correlator;
pub mod destination_trend;
pub mod dns_names;
pub mod event;
pub mod exe_context;
pub mod exe_identity;
pub mod exe_location;
pub mod findings;
pub mod findings_run;
pub mod flow;
pub mod identity;
pub mod identity_run;
pub mod install_records;
pub mod known;
pub mod launch_run;
pub mod launch_target;
pub mod launchers;
pub mod metrics;
pub mod mini_xml;
pub mod new_things;
pub mod owner_dataset;
pub mod owner_refresh;
pub mod owner_totals;
pub mod pipeline;
pub mod ports;
pub mod program_group;
pub mod retention;
pub mod rollup;
pub mod rollup_ledger;
pub mod rollup_run;
pub mod silence;
pub mod storage_guard;
pub mod task_xml;
pub mod time;
pub mod trend;
pub mod usage;

pub use addr_class::{classify_addr, AddrClass};
pub use clean_text::clean_text;
pub use context_run::{observe_context, ContextFailure, ContextPass, ContextSettings};
pub use correlator::correlate;
pub use destination_trend::{
    new_destination_cutoff, rank_destination_programs, rank_destinations, select_new, top_destination_programs,
    AddressScope, DestinationOrder, DestinationProgram, DestinationTotals, DestinationTrendRepository, NewDestination,
    ProgramDestination,
};
pub use dns_names::{backfill_names, AddressName, DnsAnswerRow, NamesBackfill};
pub use event::{EventCategory, EventEnvelope, EventPayload, NetAction, NetProtocol, Severity};
pub use exe_context::{
    classify_sid, merge_snapshot, AccountKind, AccountSeen, ContextObservation, ContextRepository, ContextSnapshot,
    ContextSourceError, ContextTracker, ContextWrite, ExecutableContext, MergedSnapshot, ProcessContext,
    ProcessContextSource, ServiceInfo, ServiceSeen,
};
pub use exe_identity::{
    FileFacts, FileInspector, FileStamp, IdentityEntry, IdentityFailure, IdentityRecord, IdentityRepository,
    InspectError, InspectErrorKind, Signature, SignatureKind, SignatureStatus, VersionInfo,
};
pub use exe_location::{classify_location, install_root, product_key, LocationClass, ProductKey};
pub use findings::{
    apply_decision, folder_scope_of, reconcile, Candidate, Decision, DecisionEffect, DecisionError, Expectation,
    ExpectationScope, Finding, FindingStatus, FindingWrite, NewExpectation, ReconcileSettings, Reconciled, Tier,
    FOLDER_SCOPE_PREFIX,
};
pub use findings_run::{
    clean_candidate, run_detection, DetectError, DetectionState, DetectionStateRepository, Detector,
    ExpectationRepository, FindingRepository, RunReport,
};
pub use flow::{Destination, FlowAggregator, FlowRow};
pub use identity::ProcessInstanceId;
pub use identity_run::{inspect_due, retry_after, IdentityPass, IdentitySettings};
pub use install_records::{
    registered_for, sweep_installs, to_record, InstallRecord, InstallRepository, InstallSeen, InstallSource,
    InstallSourceError, InstallSweep, InstallSweepError, InstallWrite, RawInstall, SourceInstalls,
};
pub use known::{
    KnownError, KnownIndex, KnownProduct, KnownProductRepository, LookalikeReason, NewKnownProduct, Standing,
};
pub use launch_run::{sweep_launchers, SweepReport};
pub use launch_target::{launch_target, LaunchTarget};
pub use launchers::{
    LaunchSource, LaunchSourceError, LauncherKind, LauncherObservation, LauncherRepository, LauncherSeen,
    LauncherWrite, RawEntry, SourceRead,
};
pub use metrics::{
    CpuBreakdown, CpuContribution, MetricsSample, OtherProcesses, ProcessMetric, PurgeCounts, SystemMetrics,
    SystemPoint,
};
pub use new_things::{
    new_thing_candidates, ChangeFacts, IdentityChange, IdentityChangeRepository, NewThingsDetector, NewThingsFacts,
    MAX_NEW_PER_RULE,
};
pub use owner_dataset::{addr_key, lookup_in, parse_ip2asn, Owner, OwnerRange, ParsedDataset};
pub use owner_refresh::{
    refresh_owner_data, DatasetFetcher, DatasetInfo, FetchError, FetchedFile, OwnerRefreshError, OwnerRepository,
    RefreshLimits, RefreshOutcome,
};
pub use owner_totals::{group_by_owner, OwnerGroup, DESTINATIONS_PER_OWNER};
pub use pipeline::{ingest_metrics, IngestError, Pipeline};
pub use ports::{
    EventQuery, EventRepository, FlowQueries, MetricsRepository, MetricsSource, ProcessFilter, ProcessQueries,
    ProcessRecord, ProcessRepository, RepoError, Retention, SourceError, TelemetrySource,
};
pub use program_group::{group_programs, rank_groups, GroupKey, ProgramGroup};
pub use rollup::{Executable, HourInput, HourRollup, LedgerEntry, ProcessHour, SystemHour, TrafficHour};
pub use rollup_ledger::LedgerObservations;
pub use rollup_run::{roll_up_due, RollupPass, RollupRepository, RollupSettings};
pub use time::{Clock, SystemClock, Timestamp};
pub use trend::{
    all_programs, collect_totals, rank_totals, top_programs, ProcessTotals, ProgramMetric, ProgramSummary,
    ProgramTotals, TrafficTotals, TrendRepository,
};
pub use usage::{
    cpu_floor_for_cores, usage_candidates, FlaggedSpan, ProcessHourRow, TrafficHourRow, UsageDetector, UsageFacts,
    UsageRepository, UsageSettings, MAX_USAGE_PER_RUN,
};
