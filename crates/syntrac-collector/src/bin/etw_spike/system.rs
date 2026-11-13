//! Small Windows helpers for the spike: elevation check and own-process CPU time.

use std::mem::size_of;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, OpenProcessToken};

pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: out-pointers reference live locals; the token handle is closed before returning.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(std::ptr::from_mut(&mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        queried.is_ok() && elevation.TokenIsElevated != 0
    }
}

fn filetime_to_duration(time: FILETIME) -> Duration {
    let ticks_100ns = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Duration::from_nanos(ticks_100ns * 100)
}

/// Total CPU time (kernel + user) consumed by this process so far.
pub fn own_cpu_time() -> Duration {
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: all four out-pointers reference live locals; the pseudo-handle needs no closing.
    let result = unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) };
    match result {
        Ok(()) => filetime_to_duration(kernel) + filetime_to_duration(user),
        Err(_) => Duration::ZERO,
    }
}
