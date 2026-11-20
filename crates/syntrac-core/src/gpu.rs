//! GPU figures per process (docs/ALERTS-PLAN.md, slice G1). PURE: this module only turns the rows of the Windows GPU performance
//! counters into numbers; reading the counters is the job of a [`GpuSource`] outside the core.
//!
//! The unit, used everywhere GPU is stored or shown: a process's GPU figure is the share (0-100) of its BUSIEST ENGINE over all
//! adapters, the way Task Manager reports it. It is a share of one engine, NOT of "the whole machine", so it is never divided by a
//! core count. GPU memory is the process's DEDICATED GPU memory added up over adapters (shared system memory is not counted).

use std::collections::BTreeMap;

/// One row of a counter: the instance name the counter service gave it and its value (a percentage, or bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct CounterRow {
    pub instance: String,
    pub value: f64,
}

/// Everything one read of the two counters returned.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuReading {
    /// `GPU Engine(*)\Utilization Percentage`
    pub engines: Vec<CounterRow>,
    /// `GPU Process Memory(*)\Dedicated Usage`
    pub memory: Vec<CounterRow>,
}

/// Where a reading comes from. `None` means the counters are not available on this machine (an old driver, a virtual machine, a
/// failed read): the caller records "not available", never zero.
pub trait GpuSource {
    fn read(&mut self) -> Option<GpuReading>;
}

/// One engine instance: which process, and which engine (adapter, physical GPU and engine number, as one text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineInstance {
    pub pid: u32,
    pub engine: String,
}

/// A process's GPU use in one reading.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProcessGpu {
    /// Share (0-100) of the process's busiest engine.
    pub percent: f32,
    pub memory_bytes: u64,
}

/// All processes in one reading, plus the busiest engine of the whole machine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuFigures {
    pub by_pid: BTreeMap<u32, ProcessGpu>,
    /// The busiest engine over all adapters, all processes added up per engine (0-100).
    pub busiest_percent: f32,
}

/// The counter service numbers instances that would otherwise have the same name (`...#1`); that suffix is not part of the name.
fn without_suffix(name: &str) -> &str {
    match name.rsplit_once('#') {
        Some((base, digits)) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => name,
    }
}

/// Splits `pid_<n>_<rest>` into the process id and the rest.
fn split_pid(name: &str) -> Option<(u32, &str)> {
    let after = name.strip_prefix("pid_")?;
    let (digits, rest) = after.split_once('_')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, rest))
}

/// `pid_<n>_luid_<hi>_<lo>_phys_<p>_eng_<e>_engtype_<kind>` gives the process and the engine `luid_<hi>_<lo>_phys_<p>_eng_<e>`.
pub fn parse_engine_instance(name: &str) -> Option<EngineInstance> {
    let (pid, rest) = split_pid(without_suffix(name.trim()))?;
    let (engine, _kind) = rest.split_once("_engtype_")?;
    let valid = engine.starts_with("luid_") && engine.contains("_phys_") && engine.contains("_eng_");
    valid.then(|| EngineInstance { pid, engine: engine.to_owned() })
}

/// `pid_<n>_luid_<hi>_<lo>_phys_<p>` gives the process.
pub fn parse_memory_instance(name: &str) -> Option<u32> {
    let (pid, rest) = split_pid(without_suffix(name.trim()))?;
    (rest.starts_with("luid_") && rest.contains("_phys_")).then_some(pid)
}

/// A reading that is not a number, or is negative, counts as zero; an engine cannot be more than fully busy.
fn percent_of(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn bytes_of(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value as u64
    } else {
        0
    }
}

/// Turns one reading into per-process figures. A row whose name does not parse is skipped and never hides the rest.
pub fn gpu_figures(reading: &GpuReading) -> GpuFigures {
    // Per (process, engine): the same engine listed twice (the `#1` instances) is added up.
    let mut per_engine: BTreeMap<(u32, String), f64> = BTreeMap::new();
    for row in &reading.engines {
        if let Some(instance) = parse_engine_instance(&row.instance) {
            *per_engine.entry((instance.pid, instance.engine)).or_default() += percent_of(row.value);
        }
    }

    let mut by_pid: BTreeMap<u32, ProcessGpu> = BTreeMap::new();
    let mut per_machine_engine: BTreeMap<&str, f64> = BTreeMap::new();
    for ((pid, engine), sum) in &per_engine {
        let share = sum.min(100.0);
        let slot = by_pid.entry(*pid).or_default();
        slot.percent = slot.percent.max(share as f32);
        *per_machine_engine.entry(engine.as_str()).or_default() += share;
    }
    for row in &reading.memory {
        if let Some(pid) = parse_memory_instance(&row.instance) {
            let slot = by_pid.entry(pid).or_default();
            slot.memory_bytes = slot.memory_bytes.saturating_add(bytes_of(row.value));
        }
    }

    let busiest = per_machine_engine.values().fold(0.0_f64, |most, sum| most.max(sum.min(100.0)));
    GpuFigures { by_pid, busiest_percent: busiest as f32 }
}
