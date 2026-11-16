//! The Windows services, as launch records: each service's start type and the command that starts it (I4b). Read only: the
//! service manager is asked to enumerate and to report each service's configuration, nothing is started, stopped or changed.

use syntrac_core::{clean_text, LaunchSource, LaunchSourceError, LauncherKind, RawEntry, SourceRead};
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_MORE_DATA;
use windows::Win32::Security::SC_HANDLE;
use windows::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW, QueryServiceConfigW,
    ENUM_SERVICE_STATUS_PROCESSW, QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SC_MANAGER_ENUMERATE_SERVICE,
    SERVICE_AUTO_START, SERVICE_DISABLED, SERVICE_QUERY_CONFIG, SERVICE_STATE_ALL, SERVICE_WIN32,
};

/// The listing is read in batches; a machine has a few hundred services.
const MAX_BATCHES: usize = 16;
/// Longest command kept.
const MAX_COMMAND_CHARS: usize = 1_024;

pub struct ServiceLaunchSource;

fn close(handle: SC_HANDLE) {
    // SAFETY: the handle came from the service manager and is closed once.
    let _ = unsafe { CloseServiceHandle(handle) };
}

/// The names of every Win32 service, running or not.
fn service_names() -> Result<Vec<String>, LaunchSourceError> {
    let mut names = Vec::new();
    // SAFETY: the manager handle is closed below; the buffer is 8-byte aligned storage that outlives every pointer read from it,
    // and `returned` entries are only read after the call reports them.
    unsafe {
        let manager = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE)
            .map_err(|error| LaunchSourceError(format!("could not open the service manager: {error}")))?;
        let mut buffer = vec![0u64; 32 * 1024];
        let mut resume = 0u32;
        for _ in 0..MAX_BATCHES {
            let (mut needed, mut returned) = (0u32, 0u32);
            let bytes = std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), buffer.len() * 8);
            let result = EnumServicesStatusExW(
                manager,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(bytes),
                &mut needed,
                &mut returned,
                Some(&mut resume),
                PCWSTR::null(),
            );
            let entries =
                std::slice::from_raw_parts(buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(), returned as usize);
            names.extend(entries.iter().filter_map(|entry| entry.lpServiceName.to_string().ok()));
            match result {
                Ok(()) => break,
                Err(error) if error.code() == windows::core::HRESULT::from_win32(ERROR_MORE_DATA.0) && returned > 0 => {
                    continue
                }
                Err(error) => {
                    close(manager);
                    return Err(LaunchSourceError(format!("could not list the services: {error}")));
                }
            }
        }
        close(manager);
    }
    Ok(names)
}

/// One service's start type and the command that starts it; `None` when it may not be read.
fn configuration(manager: SC_HANDLE, name: &str) -> Option<(u32, String)> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the service handle is closed before returning; the buffer is 8-byte aligned, as large as the call asked for, and
    // the strings in it are read only while it is alive.
    unsafe {
        let service = OpenServiceW(manager, PCWSTR(wide.as_ptr()), SERVICE_QUERY_CONFIG).ok()?;
        let mut needed = 0u32;
        let first = QueryServiceConfigW(service, None, 0, &mut needed);
        let wanted = match first {
            Err(error) if error.code() == windows::core::HRESULT::from_win32(122) => needed as usize, // insufficient buffer
            _ => 0,
        };
        let result = if wanted == 0 || wanted > 64 * 1024 {
            None
        } else {
            let mut buffer = vec![0u64; wanted.div_ceil(8)];
            let config = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
            QueryServiceConfigW(service, Some(config), wanted as u32, &mut needed)
                .ok()
                .map(|()| ((*config).dwStartType.0, (*config).lpBinaryPathName.to_string().unwrap_or_default()))
        };
        close(service);
        result
    }
}

impl LaunchSource for ServiceLaunchSource {
    fn name(&self) -> &'static str {
        "services"
    }

    fn kinds(&self) -> &'static [LauncherKind] {
        &[LauncherKind::ServiceAuto, LauncherKind::ServiceOther]
    }

    fn read(&self) -> Result<SourceRead, LaunchSourceError> {
        let names = service_names()?;
        let mut read = SourceRead::default();
        // SAFETY: the manager handle is closed below.
        let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE) }
            .map_err(|error| LaunchSourceError(format!("could not open the service manager: {error}")))?;
        for name in names {
            let Some((start_type, command)) = configuration(manager, &name) else {
                read.unreadable += 1;
                continue;
            };
            let Some(command) = clean_text(&command, MAX_COMMAND_CHARS) else { continue };
            let Some(location) = clean_text(&name, 256) else { continue };
            let (kind, detail) = match start_type {
                t if t == SERVICE_AUTO_START.0 => (LauncherKind::ServiceAuto, "automatic"),
                t if t == SERVICE_DISABLED.0 => (LauncherKind::ServiceOther, "disabled"),
                _ => (LauncherKind::ServiceOther, "on demand"),
            };
            read.entries.push(RawEntry { kind, location, command, detail: detail.to_owned(), profile: None });
        }
        close(manager);
        Ok(read)
    }
}
