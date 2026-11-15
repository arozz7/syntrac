//! What account each running program runs as, and which Windows services it hosts (docs/RUNAS-PLAN.md, I4a).
//!
//! A thin Windows adapter. Per pass: one Toolhelp snapshot of the processes, one token read per process, one service listing.
//! Turning a SID into a name is the only call that can be slow (a domain account may be asked of a domain controller), so each
//! SID is resolved once and kept. Everything is a QUERY: no process is touched beyond opening it with the least right that
//! works, nothing is written, nothing is injected. A process that cannot be opened (protected system processes) is counted, never
//! an error.
//!
//! Text from Windows (account and service names) goes through the same sanitiser as text from files.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::size_of;

use syntrac_core::{ContextSnapshot, ContextSourceError, ProcessContext, ProcessContextSource, ServiceInfo};
use tracing::warn;
use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL, PSID};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, TokenUser, SC_HANDLE, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, ENUM_SERVICE_STATUS_PROCESSW, SC_ENUM_PROCESS_INFO,
    SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_WIN32,
};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::file_parse::sanitize;
use crate::system::OwnedHandle;

/// The most SIDs kept in the name cache. A machine has a handful; the bound only stops a pathological one from growing it.
const MAX_CACHED_NAMES: usize = 1024;
/// The most times the service listing is asked for the next batch (it normally fits in one).
const MAX_SERVICE_BATCHES: usize = 16;
const ERROR_MORE_DATA: u32 = 234;

pub struct WindowsProcessContextSource {
    /// SID text -> account name (or none when Windows cannot name it).
    names: RefCell<HashMap<String, Option<String>>>,
}

impl WindowsProcessContextSource {
    pub fn new() -> Self {
        Self { names: RefCell::new(HashMap::new()) }
    }

    fn account_name(&self, sid_text: &str, sid: PSID) -> Option<String> {
        if let Some(known) = self.names.borrow().get(sid_text) {
            return known.clone();
        }
        let resolved = lookup_account_name(sid);
        let mut cache = self.names.borrow_mut();
        if cache.len() < MAX_CACHED_NAMES {
            cache.insert(sid_text.to_owned(), resolved.clone());
        }
        resolved
    }
}

impl Default for WindowsProcessContextSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessContextSource for WindowsProcessContextSource {
    fn observe(&self) -> Result<ContextSnapshot, ContextSourceError> {
        let pids = list_pids().map_err(ContextSourceError)?;
        let services = running_services();
        let mut snapshot = ContextSnapshot::default();
        for pid in pids {
            // The idle process and the system process have no image to name.
            if pid == 0 || pid == 4 {
                continue;
            }
            match self.read_process(pid, &services) {
                Some(process) => snapshot.processes.push(process),
                None => snapshot.unreadable += 1,
            }
        }
        Ok(snapshot)
    }
}

impl WindowsProcessContextSource {
    /// The image path, account and services of one process, or `None` when it cannot be opened or read.
    fn read_process(&self, pid: u32, services: &HashMap<u32, Vec<ServiceInfo>>) -> Option<ProcessContext> {
        // SAFETY: the process handle is closed by OwnedHandle; every out-pointer references a live local.
        unsafe {
            let process = OwnedHandle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
            let path = image_path(process.0)?;
            let mut token = HANDLE::default();
            OpenProcessToken(process.0, TOKEN_QUERY, &mut token).ok()?;
            let token = OwnedHandle(token);
            let (sid_text, account_name) = self.token_user(token.0)?;
            Some(ProcessContext {
                path,
                sid: sid_text,
                account_name,
                services: services.get(&pid).cloned().unwrap_or_default(),
            })
        }
    }

    /// The SID (as text) and the name of the account a token belongs to.
    ///
    /// # Safety
    /// `token` must be a live token handle opened with `TOKEN_QUERY`.
    unsafe fn token_user(&self, token: HANDLE) -> Option<(String, Option<String>)> {
        let mut needed = 0u32;
        // SAFETY: the caller guarantees a live TOKEN_QUERY handle. The first call only learns the size and is expected to fail
        // with "insufficient buffer". The second fills 8-byte aligned storage of the size Windows asked for, which holds a
        // TOKEN_USER followed by its SID; the SID pointer stays valid while `buffer` lives, which is until this function returns.
        unsafe {
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
            if needed == 0 || needed > 4096 {
                return None;
            }
            let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
            GetTokenInformation(token, TokenUser, Some(buffer.as_mut_ptr().cast()), needed, &mut needed).ok()?;
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let sid = user.User.Sid;
            let text = sid_to_text(sid)?;
            let name = self.account_name(&text, sid);
            Some((text, name))
        }
    }
}

/// The ids of every process, from one snapshot.
fn list_pids() -> Result<Vec<u32>, String> {
    // SAFETY: standard Toolhelp enumeration; the snapshot handle is closed by OwnedHandle and `entry` is a live local with
    // dwSize set as the API requires.
    unsafe {
        let snapshot = OwnedHandle(
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|error| format!("process snapshot: {error}"))?,
        );
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut pids = Vec::new();
        let mut next = Process32FirstW(snapshot.0, &mut entry);
        while next.is_ok() {
            pids.push(entry.th32ProcessID);
            next = Process32NextW(snapshot.0, &mut entry);
        }
        Ok(pids)
    }
}

/// The Win32 image path of a process.
///
/// # Safety
/// `process` must be a live process handle opened with at least `PROCESS_QUERY_LIMITED_INFORMATION`.
unsafe fn image_path(process: HANDLE) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    // SAFETY: the caller guarantees a live process handle; the buffer and its length are live locals.
    unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length) }.ok()?;
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    if path.trim().is_empty() {
        None
    } else {
        Some(path)
    }
}

fn sid_to_text(sid: PSID) -> Option<String> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` points into a live token buffer; on success Windows allocates `text`, which is freed exactly once below.
    unsafe {
        ConvertSidToStringSidW(sid, &mut text).ok()?;
        let result = text.to_string().ok();
        let _ = LocalFree(HLOCAL(text.0.cast()));
        result
    }
}

/// `DOMAIN\name` for a SID, as Windows names it, or `None` when it cannot (a SID that maps to no account).
fn lookup_account_name(sid: PSID) -> Option<String> {
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut name_length, mut domain_length) = (name.len() as u32, domain.len() as u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: both buffers are live locals with their lengths passed alongside; `sid` points into a live token buffer.
    let result = unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            PWSTR(name.as_mut_ptr()),
            &mut name_length,
            PWSTR(domain.as_mut_ptr()),
            &mut domain_length,
            &mut kind,
        )
    };
    result.ok()?;
    let name = String::from_utf16_lossy(&name[..name_length as usize]);
    let domain = String::from_utf16_lossy(&domain[..domain_length as usize]);
    sanitize(&if domain.is_empty() { name } else { format!("{domain}\\{name}") })
}

/// Every running Windows service, by the id of the process that hosts it. An empty map (never an error) when the service
/// manager cannot be asked: services are evidence, not the telemetry itself.
pub fn running_services() -> HashMap<u32, Vec<ServiceInfo>> {
    let mut by_pid: HashMap<u32, Vec<ServiceInfo>> = HashMap::new();
    // SAFETY: the service manager handle is closed below; the buffer is 8-byte aligned storage that outlives every pointer
    // read from it, and `returned` entries are only read after the call reports them.
    unsafe {
        let Ok(manager) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE) else {
            warn!("Could not open the service manager; services are not recorded this pass");
            return by_pid;
        };
        let mut buffer = vec![0u64; 16 * 1024];
        let mut resume = 0u32;
        for _ in 0..MAX_SERVICE_BATCHES {
            let (mut needed, mut returned) = (0u32, 0u32);
            let bytes = std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), buffer.len() * 8);
            let result = EnumServicesStatusExW(
                manager,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_ACTIVE,
                Some(bytes),
                &mut needed,
                &mut returned,
                Some(&mut resume),
                PCWSTR::null(),
            );
            let entries =
                std::slice::from_raw_parts(buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(), returned as usize);
            for entry in entries {
                let (Ok(name), Ok(display)) = (entry.lpServiceName.to_string(), entry.lpDisplayName.to_string()) else {
                    continue;
                };
                let Some(name) = sanitize(&name) else { continue };
                by_pid
                    .entry(entry.ServiceStatusProcess.dwProcessId)
                    .or_default()
                    .push(ServiceInfo { name, display_name: sanitize(&display) });
            }
            match result {
                Ok(()) => break,
                Err(error) if error.code() == HRESULT::from_win32(ERROR_MORE_DATA) && returned > 0 => continue,
                Err(_) => break,
            }
        }
        close_service_handle(manager);
    }
    by_pid
}

/// # Safety
/// `handle` must be a live service-manager handle that is not used afterwards.
unsafe fn close_service_handle(handle: SC_HANDLE) {
    // SAFETY: the caller guarantees a live handle that is not used again.
    let _ = unsafe { CloseServiceHandle(handle) };
}
