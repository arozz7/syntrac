//! The Windows side of the orphan guard: lists and stops ETW trace sessions, and looks for other collector
//! processes. Thin by design; every decision lives in the tested `orphan_guard` module.

use std::mem::size_of;
use std::sync::mpsc;
use std::time::Duration;

use ferrisetw::trace::stop_trace_by_name;
use sysinfo::{ProcessesToUpdate, System};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Diagnostics::Etw::{QueryAllTracesW, EVENT_TRACE_PROPERTIES};

use crate::orphan_guard::SessionHost;

/// Windows allows at most 64 concurrent trace sessions.
const MAX_SESSIONS: usize = 64;
/// Capacity, in UTF-16 units, reserved for each session's logger name and log file name.
const NAME_UNITS: usize = 1024;
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const COLLECTOR_PROCESS: &str = "syntrac-service.exe";

pub struct WindowsSessions;

impl SessionHost for WindowsSessions {
    fn list_sessions(&self) -> Result<Vec<String>, String> {
        query_session_names()
    }

    fn stop_session(&self, name: &str) -> Result<(), String> {
        // Stopped on a helper thread so a stalled stop cannot hold up starting the collector.
        let (done, finished) = mpsc::channel();
        let name = name.to_owned();
        std::thread::spawn(move || {
            let _ = done.send(stop_trace_by_name(&name).map_err(|error| format!("{error:?}")));
        });
        finished.recv_timeout(STOP_TIMEOUT).unwrap_or_else(|_| Err("stopping the session timed out".to_owned()))
    }

    fn other_collector_running(&self) -> bool {
        let Ok(own_pid) = sysinfo::get_current_pid() else {
            // Cannot tell, so assume another collector exists: the safe answer is "stop nothing".
            return true;
        };
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        system.processes().iter().any(|(pid, process)| {
            *pid != own_pid && process.name().to_string_lossy().eq_ignore_ascii_case(COLLECTOR_PROCESS)
        })
    }
}

/// Asks Windows for every running trace session and returns their names.
fn query_session_names() -> Result<Vec<String>, String> {
    let header = size_of::<EVENT_TRACE_PROPERTIES>();
    let logger_name_offset = header;
    let log_file_name_offset = header + NAME_UNITS * 2;
    // Each slot is the properties header followed by room for the two names, rounded up to whole u64s so every
    // slot starts suitably aligned.
    let slot_words = (header + 2 * NAME_UNITS * 2).div_ceil(8);
    let mut buffer = vec![0u64; slot_words * MAX_SESSIONS];

    let base = buffer.as_mut_ptr().cast::<u8>();
    let mut slots: Vec<*mut EVENT_TRACE_PROPERTIES> = (0..MAX_SESSIONS)
        .map(|index| {
            // SAFETY: `index * slot_words` words stays inside `buffer`, which has MAX_SESSIONS slots.
            let slot = unsafe { base.add(index * slot_words * 8) }.cast::<EVENT_TRACE_PROPERTIES>();
            // SAFETY: the slot is zeroed, aligned and large enough for the header.
            unsafe {
                (*slot).Wnode.BufferSize = (slot_words * 8) as u32;
                (*slot).LoggerNameOffset = logger_name_offset as u32;
                (*slot).LogFileNameOffset = log_file_name_offset as u32;
            }
            slot
        })
        .collect();

    let mut count = 0u32;
    // SAFETY: `slots` holds MAX_SESSIONS valid, initialised property blocks, each with room for its names.
    let status = unsafe { QueryAllTracesW(&mut slots, &mut count) };
    if status != ERROR_SUCCESS {
        return Err(format!("QueryAllTracesW failed: {status:?}"));
    }

    let mut names = Vec::new();
    for slot in slots.iter().take((count as usize).min(MAX_SESSIONS)) {
        // SAFETY: Windows wrote a nul-terminated UTF-16 name at LoggerNameOffset inside this slot, whose name area
        // holds NAME_UNITS units; the read below never goes past that area.
        let name = unsafe {
            let first = slot.cast::<u8>().add(logger_name_offset).cast::<u16>();
            let units = std::slice::from_raw_parts(first, NAME_UNITS);
            let length = units.iter().position(|&unit| unit == 0).unwrap_or(NAME_UNITS);
            String::from_utf16_lossy(&units[..length])
        };
        if !name.is_empty() {
            names.push(name);
        }
    }
    Ok(names)
}
