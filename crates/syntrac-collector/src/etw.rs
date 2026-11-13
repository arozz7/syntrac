//! Live ETW telemetry: Kernel-Process, Kernel-Network and DNS-Client in one trace session.
//!
//! This is a thin adapter. It pulls raw field values out of each `EventRecord` and hands them to the
//! tested functions in `etw_map`. The callbacks never block: events go through a BOUNDED channel with
//! `try_send`, and anything that does not fit is counted as dropped (blocking would make the kernel
//! drop events silently instead).

use std::net::IpAddr;
use std::sync::mpsc::{self, sync_channel, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferrisetw::parser::Parser;
use ferrisetw::provider::Provider;
use ferrisetw::schema_locator::SchemaLocator;
use ferrisetw::trace::{stop_trace_by_name, TraceTrait, UserTrace};
use ferrisetw::EventRecord;
use syntrac_core::{Clock, EventEnvelope, SourceError, TelemetrySource, Timestamp};
use tracing::{error, info, warn};

use crate::etw_map::{
    dns_resolution, is_tracked_dns_event, is_tracked_network_event, network_activity, network_field_needs,
    process_started, process_stopped, DnsFields, NetworkFields, ProcessStartFields, ProcessStopFields,
};
use crate::etw_stats::EtwStats;
use crate::paths::NtPathConverter;
use crate::system::SystemClock;

const SESSION_NAME: &str = "SyntracCollector";
const KERNEL_PROCESS: &str = "22fb2cd6-0e7b-422b-a0c7-2fad1fd0e716";
const KERNEL_NETWORK: &str = "7dd42a49-5329-4832-8dfd-43d979153a88";
const DNS_CLIENT: &str = "1c95126e-7eea-49a9-a3fe-a378b03ddb4d";
/// WINEVENT_KEYWORD_PROCESS: process start/stop only (from the provider manifest, confirmed in the spike).
const KEYWORD_PROCESS: u64 = 0x10;
const CHANNEL_CAPACITY: usize = 65_536;
const MAX_EVENTS_PER_DRAIN: usize = 200_000;
/// How long to wait for the trace session to stop before carrying on without it.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

const PROCESS_START: u16 = 1;
const PROCESS_STOP: u16 = 2;

const PROVIDER_PROCESS: &str = "process";
const PROVIDER_NETWORK: &str = "network";
const PROVIDER_DNS: &str = "dns";

#[derive(Clone)]
struct Sink {
    sender: SyncSender<EventEnvelope>,
    stats: Arc<EtwStats>,
}

impl Sink {
    fn send(&self, event: EventEnvelope) {
        match self.sender.try_send(event) {
            Ok(()) => self.stats.record_received(),
            Err(_) => self.stats.record_dropped(),
        }
    }

    fn parse_error(&self, provider: &'static str, event_id: u16) {
        self.stats.record_parse_error(provider, event_id);
    }

    /// Notes how the event's own timestamp compares with the wall clock (FILETIME vs QPC check).
    fn observe(&self, event_ts: Timestamp) {
        self.stats.observe_event_time(event_ts, SystemClock.now());
    }
}

pub struct EtwSource {
    receiver: Receiver<EventEnvelope>,
    stats: Arc<EtwStats>,
    trace: Option<UserTrace>,
}

impl EtwSource {
    /// Starts the trace. Needs an elevated process. A session left behind by a crashed run is stopped first.
    pub fn start(paths: NtPathConverter) -> Result<Self, SourceError> {
        // Fails harmlessly when no stale session exists.
        if stop_trace_by_name(SESSION_NAME).is_ok() {
            warn!(session = SESSION_NAME, "Stopped a trace session left over from a previous run");
        }

        let (sender, receiver) = sync_channel(CHANNEL_CAPACITY);
        let stats = Arc::new(EtwStats::default());
        let sink = Sink { sender, stats: Arc::clone(&stats) };
        let paths = Arc::new(paths);

        let process_provider = {
            let (sink, paths) = (sink.clone(), Arc::clone(&paths));
            Provider::by_guid(KERNEL_PROCESS)
                .any(KEYWORD_PROCESS)
                .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
                    handle_process(record, locator, &sink, &paths);
                })
                .build()
        };
        let network_provider = {
            let sink = sink.clone();
            Provider::by_guid(KERNEL_NETWORK)
                .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
                    handle_network(record, locator, &sink);
                })
                .build()
        };
        let dns_provider = Provider::by_guid(DNS_CLIENT)
            .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
                handle_dns(record, locator, &sink);
            })
            .build();

        let (trace, handle) = UserTrace::new()
            .named(SESSION_NAME.to_owned())
            .enable(process_provider)
            .enable(network_provider)
            .enable(dns_provider)
            .start()
            .map_err(|error| {
                SourceError::Failed(format!("could not start the ETW trace (is the process elevated?): {error:?}"))
            })?;

        std::thread::spawn(move || {
            let status = UserTrace::process_from_handle(handle);
            info!(?status, "ETW processing thread ended");
        });

        info!(session = SESSION_NAME, "ETW trace started");
        Ok(Self { receiver, stats, trace: Some(trace) })
    }

    pub fn stats(&self) -> Arc<EtwStats> {
        Arc::clone(&self.stats)
    }

    /// Stops the trace session. Idempotent, and it cannot hang the caller.
    ///
    /// The session is stopped BY NAME on a helper thread with a timeout. `ferrisetw`'s own stop calls
    /// `CloseTrace` first, which can block while the processing thread is still inside ProcessTrace; a
    /// stalled stop kept the console collector alive after its run finished. The trace object is then
    /// deliberately leaked (not dropped): dropping would repeat that blocking sequence, the session is
    /// already stopped, and the operating system releases the rest when the process exits.
    fn stop_trace(&mut self) {
        let Some(trace) = self.trace.take() else { return };
        let began = Instant::now();
        info!(session = SESSION_NAME, "Stopping the ETW trace session");

        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(stop_trace_by_name(SESSION_NAME));
        });
        match finished.recv_timeout(STOP_TIMEOUT) {
            Ok(Ok(())) => info!(elapsed_ms = began.elapsed().as_millis() as u64, "ETW trace session stopped"),
            Ok(Err(error)) => warn!(?error, "Stopping the ETW trace session reported an error"),
            Err(_) => {
                error!(timeout_secs = STOP_TIMEOUT.as_secs(), "The ETW trace session did not stop in time; continuing")
            }
        }
        std::mem::forget(trace);
    }
}

impl Drop for EtwSource {
    fn drop(&mut self) {
        self.stop_trace();
    }
}

impl TelemetrySource for EtwSource {
    fn name(&self) -> &str {
        "etw"
    }

    fn stop(&mut self) {
        self.stop_trace();
    }

    fn drain(&mut self) -> Result<Vec<EventEnvelope>, SourceError> {
        let mut events = Vec::new();
        while events.len() < MAX_EVENTS_PER_DRAIN {
            match self.receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(SourceError::Failed("ETW event channel closed".into()));
                }
            }
        }
        Ok(events)
    }
}

/// ETW header timestamps are system time (FILETIME) because the trace is not opened in raw-timestamp mode.
/// The run summary reports the observed lag and any "future" events, which settles that question with data.
fn event_time(record: &EventRecord) -> Timestamp {
    Timestamp::from_filetime(u64::try_from(record.raw_timestamp()).unwrap_or(0))
}

fn handle_process(record: &EventRecord, locator: &SchemaLocator, sink: &Sink, paths: &NtPathConverter) {
    let event_id = record.event_id();
    if event_id != PROCESS_START && event_id != PROCESS_STOP {
        return;
    }
    let Ok(schema) = locator.event_schema(record) else {
        sink.parse_error(PROVIDER_PROCESS, event_id);
        return;
    };
    let parser = Parser::create(record, &schema);
    let ts = event_time(record);
    sink.observe(ts);

    let pid: Result<u32, _> = parser.try_parse("ProcessID");
    let create_time: Result<u64, _> = parser.try_parse("CreateTime");

    if event_id == PROCESS_START {
        let parent_pid: Result<u32, _> = parser.try_parse("ParentProcessID");
        let image_name: Result<String, _> = parser.try_parse("ImageName");
        match (pid, parent_pid, create_time, image_name) {
            (Ok(pid), Ok(parent_pid), Ok(create_time), Ok(image_name)) => {
                sink.send(process_started(ts, ProcessStartFields { pid, parent_pid, create_time, image_name }, paths));
            }
            _ => sink.parse_error(PROVIDER_PROCESS, event_id),
        }
    } else {
        let exit_code: Option<u32> = parser.try_parse::<u32>("ExitCode").ok();
        match (pid, create_time) {
            (Ok(pid), Ok(create_time)) => {
                sink.send(process_stopped(ts, ProcessStopFields { pid, create_time, exit_code }));
            }
            _ => sink.parse_error(PROVIDER_PROCESS, event_id),
        }
    }
}

fn handle_network(record: &EventRecord, locator: &SchemaLocator, sink: &Sink) {
    let event_id = record.event_id();
    if !is_tracked_network_event(event_id) {
        return;
    }
    // Failure events carry only an error code (per the manifest), so missing addresses are expected for
    // them and are not a parse error. Data events must provide a byte count.
    let needs = network_field_needs(event_id);
    let Ok(schema) = locator.event_schema(record) else {
        if needs.endpoint {
            sink.parse_error(PROVIDER_NETWORK, event_id);
        }
        return;
    };
    let parser = Parser::create(record, &schema);
    let ts = event_time(record);
    sink.observe(ts);

    let pid: Result<u32, _> = parser.try_parse("PID");
    let daddr: Result<IpAddr, _> = parser.try_parse("daddr");
    let saddr: Result<IpAddr, _> = parser.try_parse("saddr");
    let dport: Result<u16, _> = parser.try_parse("dport");
    let sport: Result<u16, _> = parser.try_parse("sport");
    let size: Result<u32, _> = parser.try_parse("size");

    let (Ok(pid), Ok(daddr), Ok(saddr), Ok(dport), Ok(sport)) = (pid, daddr, saddr, dport, sport) else {
        if needs.endpoint {
            sink.parse_error(PROVIDER_NETWORK, event_id);
        }
        return;
    };
    let size = match size {
        Ok(size) => size,
        Err(_) if needs.size => {
            sink.parse_error(PROVIDER_NETWORK, event_id);
            return;
        }
        Err(_) => 0,
    };

    if let Some(event) = network_activity(ts, NetworkFields { event_id, pid, size, daddr, saddr, dport, sport }) {
        sink.send(event);
    }
}

fn handle_dns(record: &EventRecord, locator: &SchemaLocator, sink: &Sink) {
    let event_id = record.event_id();
    if !is_tracked_dns_event(event_id) {
        return;
    }
    let Ok(schema) = locator.event_schema(record) else {
        sink.parse_error(PROVIDER_DNS, event_id);
        return;
    };
    let parser = Parser::create(record, &schema);
    let ts = event_time(record);
    sink.observe(ts);

    let query_name: Result<String, _> = parser.try_parse("QueryName");
    let results: Result<String, _> = parser.try_parse("QueryResults");
    match query_name {
        Ok(query_name) => {
            let fields = DnsFields { event_id, caller_pid: record.process_id(), query_name, results: results.ok() };
            if let Some(event) = dns_resolution(ts, fields) {
                sink.send(event);
            }
        }
        Err(_) => sink.parse_error(PROVIDER_DNS, event_id),
    }
}
