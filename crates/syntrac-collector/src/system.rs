//! Small Windows helpers shared by the collector and the console host.

use std::mem::size_of;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use syntrac_core::{Clock, Timestamp};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, OpenProcessToken};

/// The wall clock, behind the `Clock` interface so everything else stays deterministic under test.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros());
        Timestamp::from_micros(i64::try_from(micros).unwrap_or(i64::MAX))
    }
}

/// Closes a Windows handle when dropped, so early returns cannot leak it.
pub(crate) struct OwnedHandle(pub HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the wrapped handle was returned by a successful Win32 call and is closed exactly once here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: the out-pointer references a live local; the token handle is wrapped and closed on return.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let token = OwnedHandle(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let queried = GetTokenInformation(
            token.0,
            TokenElevation,
            Some(std::ptr::from_mut(&mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        queried.is_ok() && elevation.TokenIsElevated != 0
    }
}

pub fn filetime_to_u64(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

fn filetime_to_duration(time: FILETIME) -> Duration {
    Duration::from_nanos(filetime_to_u64(time) * 100)
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
