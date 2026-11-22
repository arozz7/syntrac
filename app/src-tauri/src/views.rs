//! View models: exactly what the UI renders, built from the repository traits so they are tested against the
//! in-memory store. Field names are camelCase on the wire; times are milliseconds since the Unix epoch.

use serde::{Deserialize, Serialize};
use syntrac_core::{
    FlowQueries, MetricsRepository, NetProtocol, ProcessFilter, ProcessInstanceId, ProcessQueries, ProcessRepository,
    RepoError, Timestamp,
};

use crate::names::display_name;
use crate::series::downsample;

pub const MIN_RANGE_SECS: u64 = 60;
pub const MAX_RANGE_SECS: u64 = 7 * 24 * 3600;
pub const MAX_SERIES_POINTS: usize = 240;
const TOP_PROCESSES: usize = 10;
const TOP_DESTINATIONS: usize = 8;
/// How many times more destinations are fetched than shown, so filtering loopback still leaves a full list.
const DESTINATION_OVERFETCH: usize = 8;
const DEFAULT_PROCESS_ROWS: usize = 500;
const MAX_PROCESS_ROWS: usize = 2000;
const DEFAULT_CONNECTION_ROWS: usize = 200;
const MAX_CONNECTION_ROWS: usize = 1000;

pub fn clamp_range_secs(secs: u64) -> u64 {
    secs.clamp(MIN_RANGE_SECS, MAX_RANGE_SECS)
}

pub fn clamp_limit(requested: Option<usize>, default: usize, max: usize) -> usize {
    requested.map_or(default, |n| n.clamp(1, max))
}

fn ms(ts: Timestamp) -> i64 {
    ts.as_micros() / 1000
}

fn id_text(id: ProcessInstanceId) -> String {
    id.as_uuid().to_string()
}

fn range_start(now: Timestamp, range_secs: u64) -> Timestamp {
    let micros = i64::try_from(range_secs.saturating_mul(1_000_000)).unwrap_or(i64::MAX);
    Timestamp::from_micros(now.as_micros().saturating_sub(micros))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemPointView {
    pub ts_ms: i64,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub net_received_bytes: u64,
    pub net_sent_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    /// The busiest GPU engine (a share of ONE engine, not of the machine); `None` when the GPU counters were not available.
    pub gpu_percent: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessCpuView {
    pub instance: Option<String>,
    pub pid: u32,
    pub name: String,
    pub image_path: Option<String>,
    pub avg_percent: f32,
    pub peak_percent: f32,
    pub samples_present: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuView {
    pub window_samples: u32,
    pub system_avg_percent: f32,
    pub others_avg_percent: f32,
    /// System CPU that no measured process explains (interrupt/driver time, unreadable processes).
    pub unaccounted_avg_percent: f32,
    pub processes: Vec<ProcessCpuView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessGpuView {
    pub instance: Option<String>,
    pub pid: u32,
    pub name: String,
    pub image_path: Option<String>,
    /// A share of the program's busiest GPU engine (not of the machine), averaged over the samples that had a GPU reading.
    pub avg_percent: f32,
    pub peak_percent: f32,
    pub memory_peak_bytes: u64,
}

/// Who used the GPU in the period. The figures are shares of ONE engine, so they do not add up to the system figure.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuView {
    /// How many samples of the period had a GPU reading; 0 means the counters were not available (say so, never "idle").
    pub reading_samples: u32,
    pub system_avg_percent: f32,
    pub system_peak_percent: f32,
    /// The largest single share of any program that is not listed.
    pub others_peak_percent: f32,
    pub processes: Vec<ProcessGpuView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationView {
    pub remote_addr: String,
    pub remote_port: u16,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub flow_count: u32,
    pub process_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewView {
    pub from_ms: i64,
    pub to_ms: i64,
    pub system: Vec<SystemPointView>,
    pub cpu: CpuView,
    pub gpu: GpuView,
    pub destinations: Vec<DestinationView>,
}

/// Everything the overview page shows for the last `range_secs` seconds.
pub fn overview<R>(repo: &R, now: Timestamp, range_secs: u64) -> Result<OverviewView, RepoError>
where
    R: MetricsRepository + FlowQueries + ProcessRepository,
{
    let range_secs = clamp_range_secs(range_secs);
    let from = range_start(now, range_secs);

    let system = downsample(
        repo.system_series(from, now)?
            .into_iter()
            .map(|point| SystemPointView {
                ts_ms: ms(point.ts),
                cpu_percent: point.system.cpu_percent,
                memory_percent: percent(point.system.memory_used_bytes, point.system.memory_total_bytes),
                net_received_bytes: point.system.net_received_bytes,
                net_sent_bytes: point.system.net_sent_bytes,
                disk_read_bytes: point.system.disk_read_bytes,
                disk_write_bytes: point.system.disk_write_bytes,
                gpu_percent: point.system.gpu_percent,
            })
            .collect(),
        MAX_SERIES_POINTS,
    );

    let breakdown = repo.cpu_breakdown(from, now, TOP_PROCESSES)?;
    let mut processes = Vec::with_capacity(breakdown.processes.len());
    for contribution in &breakdown.processes {
        let image_path = image_path_of(repo, contribution.instance)?;
        processes.push(ProcessCpuView {
            instance: contribution.instance.map(id_text),
            pid: contribution.pid,
            name: display_name(image_path.as_deref().unwrap_or_default(), contribution.pid),
            image_path,
            avg_percent: contribution.avg_cpu_percent,
            peak_percent: contribution.peak_cpu_percent,
            samples_present: contribution.samples_present,
        });
    }

    let gpu_breakdown = repo.gpu_breakdown(from, now, TOP_PROCESSES)?;
    let mut gpu_processes = Vec::with_capacity(gpu_breakdown.processes.len());
    for contribution in &gpu_breakdown.processes {
        let image_path = image_path_of(repo, contribution.instance)?;
        gpu_processes.push(ProcessGpuView {
            instance: contribution.instance.map(id_text),
            pid: contribution.pid,
            name: display_name(image_path.as_deref().unwrap_or_default(), contribution.pid),
            image_path,
            avg_percent: contribution.avg_percent,
            peak_percent: contribution.peak_percent,
            memory_peak_bytes: contribution.memory_peak_bytes,
        });
    }

    // Ask for more than will be shown: loopback (processes talking to this machine itself) is filtered out AFTER
    // ranking, and must not crowd real destinations out of a fixed-size list.
    let destinations = repo
        .top_destinations(from, now, TOP_DESTINATIONS * DESTINATION_OVERFETCH)?
        .into_iter()
        .filter(|d| !d.remote_addr.is_loopback())
        .take(TOP_DESTINATIONS)
        .map(|d| DestinationView {
            remote_addr: d.remote_addr.to_string(),
            remote_port: d.remote_port,
            bytes_sent: d.bytes_sent,
            bytes_received: d.bytes_received,
            flow_count: d.flow_count,
            process_count: d.process_count,
        })
        .collect();

    Ok(OverviewView {
        from_ms: ms(from),
        to_ms: ms(now),
        system,
        cpu: CpuView {
            window_samples: breakdown.window_samples,
            system_avg_percent: breakdown.system_avg_cpu_percent,
            others_avg_percent: breakdown.others_avg_cpu_percent,
            unaccounted_avg_percent: breakdown.unaccounted_avg_cpu_percent,
            processes,
        },
        gpu: GpuView {
            reading_samples: gpu_breakdown.reading_samples,
            system_avg_percent: gpu_breakdown.system_avg_percent,
            system_peak_percent: gpu_breakdown.system_peak_percent,
            others_peak_percent: gpu_breakdown.others_peak_percent,
            processes: gpu_processes,
        },
        destinations,
    })
}

fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 100.0) as f32
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProcessListParams {
    pub running_only: bool,
    pub search: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRowView {
    pub id: String,
    pub pid: u32,
    pub parent_id: Option<String>,
    pub name: String,
    pub image_path: String,
    pub command_line: Option<String>,
    pub started_ms: i64,
    pub ended_ms: Option<i64>,
    /// The exit time is only an upper bound (the process exited while the collector was not running).
    pub ended_approximate: bool,
    pub running: bool,
}

pub fn process_list<R: ProcessQueries>(repo: &R, params: &ProcessListParams) -> Result<Vec<ProcessRowView>, RepoError> {
    let filter = ProcessFilter {
        running_only: params.running_only,
        name_contains: params.search.clone(),
        alive_between: None,
        limit: clamp_limit(params.limit, DEFAULT_PROCESS_ROWS, MAX_PROCESS_ROWS),
    };
    Ok(repo
        .list_processes(&filter)?
        .into_iter()
        .map(|p| ProcessRowView {
            id: id_text(p.id),
            pid: p.pid,
            parent_id: p.parent_id.map(id_text),
            name: display_name(&p.image_path, p.pid),
            image_path: p.image_path,
            command_line: p.command_line,
            started_ms: ms(p.started),
            ended_ms: p.ended.map(ms),
            ended_approximate: p.ended_approximate,
            running: p.ended.is_none(),
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRowView {
    pub start_ms: i64,
    pub end_ms: i64,
    pub process_id: Option<String>,
    pub pid: u32,
    pub name: String,
    pub protocol: String,
    pub remote_addr: String,
    pub remote_port: u16,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub opened: u32,
    pub closed: u32,
    pub failed: u32,
}

pub fn connection_list<R: FlowQueries>(
    repo: &R,
    now: Timestamp,
    range_secs: u64,
    limit: Option<usize>,
) -> Result<Vec<ConnectionRowView>, RepoError> {
    let from = range_start(now, clamp_range_secs(range_secs));
    let limit = clamp_limit(limit, DEFAULT_CONNECTION_ROWS, MAX_CONNECTION_ROWS);
    Ok(repo
        .recent_flows(from, now, limit)?
        .into_iter()
        .map(|flow| ConnectionRowView {
            start_ms: ms(flow.window_start),
            end_ms: ms(flow.window_end),
            process_id: flow.process.map(id_text),
            pid: flow.pid,
            name: display_name(flow.image_path.as_deref().unwrap_or_default(), flow.pid),
            protocol: match flow.protocol {
                NetProtocol::Tcp => "tcp",
                NetProtocol::Udp => "udp",
            }
            .to_owned(),
            remote_addr: flow.remote_addr.to_string(),
            remote_port: flow.remote_port,
            bytes_sent: flow.bytes_sent,
            bytes_received: flow.bytes_received,
            opened: flow.opened,
            closed: flow.closed,
            failed: flow.failed,
        })
        .collect())
}

/// The path of the program a process instance belongs to, when it is known.
fn image_path_of<R: ProcessRepository>(
    repo: &R,
    instance: Option<ProcessInstanceId>,
) -> Result<Option<String>, RepoError> {
    let record = match instance {
        Some(id) => repo.get(id)?,
        None => None,
    };
    Ok(record.map(|r| r.image_path).filter(|path| !path.is_empty()))
}
