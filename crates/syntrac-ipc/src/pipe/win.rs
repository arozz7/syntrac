//! Thin Win32 layer. Everything `unsafe` in this crate is here, and each block says why it is sound.

use std::io::{self, Read, Write};
use std::mem::size_of;
use std::time::{Duration, Instant};

use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, HANDLE, HLOCAL,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcessToken, ResetEvent, WaitForSingleObject,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const SDDL_REVISION_1: u32 = 1;
const PIPE_BUFFER_BYTES: u32 = 4096;
/// How often a waiting accept loop looks at the stop flag.
const ACCEPT_POLL_MS: u32 = 250;

fn is(error: &windows::core::Error, code: WIN32_ERROR) -> bool {
    error.code() == HRESULT::from_win32(code.0)
}

fn to_io(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error((error.code().0 as u32 & 0xFFFF) as i32)
}

/// Closes the handle when dropped, so early returns cannot leak it.
pub(crate) struct OwnedHandle(pub HANDLE);

// SAFETY: a kernel handle value may be used from any thread; ownership moves with the wrapper.
unsafe impl Send for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful Win32 call and is closed exactly once, here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// A manual-reset event used to wait for overlapped operations.
pub(crate) struct Event(OwnedHandle);

impl Event {
    pub fn new() -> io::Result<Self> {
        // SAFETY: no pointers are passed except a null name and null security attributes.
        let handle = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(to_io)?;
        Ok(Self(OwnedHandle(handle)))
    }

    fn raw(&self) -> HANDLE {
        self.0 .0
    }

    fn reset(&self) {
        // SAFETY: the event handle is valid for the lifetime of `self`.
        unsafe {
            let _ = ResetEvent(self.raw());
        }
    }
}

fn wait_millis(timeout: Duration) -> u32 {
    u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1).min(u32::MAX - 1)
}

/// Runs one overlapped operation to completion, or cancels it when `timeout` passes.
///
/// `start` receives the OVERLAPPED to pass to the Win32 call. The OVERLAPPED lives on this stack frame, and this
/// function never returns while the operation could still be using it: on timeout the operation is cancelled and
/// waited for first.
fn overlapped_op(
    handle: HANDLE,
    event: &Event,
    timeout: Duration,
    start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>,
) -> io::Result<u32> {
    event.reset();
    let mut overlapped = OVERLAPPED { hEvent: event.raw(), ..Default::default() };

    match start(&mut overlapped) {
        Ok(()) => {}
        // The peer is already gone: the same as end-of-stream.
        Err(error) if is(&error, ERROR_BROKEN_PIPE) || is(&error, ERROR_PIPE_NOT_CONNECTED) => return Ok(0),
        Err(error) if is(&error, ERROR_IO_PENDING) => {
            // SAFETY: the event handle is valid.
            let waited = unsafe { WaitForSingleObject(event.raw(), wait_millis(timeout)) };
            if waited != WAIT_OBJECT_0 {
                let mut ignored = 0u32;
                // SAFETY: `overlapped` is alive and was passed to the operation above. Cancelling and then waiting
                // for completion guarantees the kernel no longer touches it before this frame ends.
                unsafe {
                    let _ = CancelIoEx(handle, Some(&overlapped));
                    let _ = GetOverlappedResult(handle, &overlapped, &mut ignored, true);
                }
                return Err(io::Error::new(io::ErrorKind::TimedOut, "pipe operation timed out"));
            }
        }
        Err(error) => return Err(to_io(error)),
    }

    let mut transferred = 0u32;
    // SAFETY: the operation has completed (or completed synchronously), so the result can be read without waiting.
    match unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, false) } {
        Ok(()) => Ok(transferred),
        Err(error) if is(&error, ERROR_BROKEN_PIPE) || is(&error, ERROR_PIPE_NOT_CONNECTED) => Ok(0),
        Err(error) => Err(to_io(error)),
    }
}

/// Creates one pipe instance with the given access list. `first` must be true for the very first instance of a
/// name so that creation fails if the name is already taken.
pub(crate) fn create_instance(path: &str, sddl: &str, first: bool) -> io::Result<OwnedHandle> {
    let wide_sddl: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `wide_sddl` is a NUL-terminated UTF-16 string that outlives the call; the out pointer is a live local.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide_sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(to_io)?;

    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }

    // SAFETY: `wide_path` is NUL-terminated and outlives the call; `attributes` points at a descriptor that stays
    // allocated until the LocalFree below, after the call has copied it.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(wide_path.as_ptr()),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            Some(&attributes),
        )
    };
    let creation_error = io::Error::last_os_error();
    // SAFETY: the descriptor was allocated by the conversion call above and is freed exactly once.
    unsafe {
        windows::Win32::Foundation::LocalFree(HLOCAL(descriptor.0));
    }

    if handle == INVALID_HANDLE_VALUE {
        return Err(creation_error);
    }
    Ok(OwnedHandle(handle))
}

/// Waits for a client to connect to `instance`. Returns `Ok(false)` if `stop` was raised first.
pub(crate) fn wait_for_client(
    instance: &OwnedHandle,
    event: &Event,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<bool> {
    use std::sync::atomic::Ordering;

    event.reset();
    let mut overlapped = OVERLAPPED { hEvent: event.raw(), ..Default::default() };
    // SAFETY: `overlapped` lives until this function returns, and it never returns while the connect is pending.
    match unsafe { ConnectNamedPipe(instance.0, Some(&mut overlapped)) } {
        Ok(()) => return Ok(true),
        // A client connected between creating the instance and calling connect.
        Err(error) if is(&error, ERROR_PIPE_CONNECTED) => return Ok(true),
        Err(error) if is(&error, ERROR_IO_PENDING) => {}
        Err(error) => return Err(to_io(error)),
    }

    loop {
        // SAFETY: the event handle is valid.
        let waited = unsafe { WaitForSingleObject(event.raw(), ACCEPT_POLL_MS) };
        if waited == WAIT_OBJECT_0 {
            let mut ignored = 0u32;
            // SAFETY: the connect completed, so the result is available without waiting.
            unsafe { GetOverlappedResult(instance.0, &overlapped, &mut ignored, false) }.map_err(to_io)?;
            return Ok(true);
        }
        if stop.load(Ordering::SeqCst) {
            let mut ignored = 0u32;
            // SAFETY: cancel the pending connect and wait for it, so `overlapped` is no longer in use.
            unsafe {
                let _ = CancelIoEx(instance.0, Some(&overlapped));
                let _ = GetOverlappedResult(instance.0, &overlapped, &mut ignored, true);
            }
            return Ok(false);
        }
    }
}

/// A connected pipe instance as a `Read + Write` stream. Every operation shares ONE deadline for the whole
/// connection, so a client cannot hold the thread by trickling data.
pub(crate) struct PipeStream {
    handle: OwnedHandle,
    event: Event,
    deadline: Instant,
}

impl PipeStream {
    pub fn new(handle: OwnedHandle, lifetime: Duration) -> io::Result<Self> {
        Ok(Self { handle, event: Event::new()?, deadline: Instant::now() + lifetime })
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "connection deadline passed"))
    }

    /// Lets the client finish reading the response: reads and discards until it closes (or the deadline passes).
    pub fn wait_for_client_to_close(&mut self) {
        let mut scratch = [0u8; 256];
        while matches!(self.read(&mut scratch), Ok(n) if n > 0) {}
    }

    pub fn disconnect(&self) {
        // SAFETY: the handle is a connected pipe instance owned by `self`.
        unsafe {
            let _ = DisconnectNamedPipe(self.handle.0);
        }
    }
}

impl Read for PipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        let handle = self.handle.0;
        // SAFETY: `buffer` and the OVERLAPPED stay alive until `overlapped_op` returns, which only happens once the
        // read has completed or been cancelled and waited for.
        overlapped_op(handle, &self.event, remaining, |overlapped| unsafe {
            ReadFile(handle, Some(buffer), None, Some(overlapped))
        })
        .map(|read| read as usize)
    }
}

impl Write for PipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        let handle = self.handle.0;
        // SAFETY: as for `read`.
        overlapped_op(handle, &self.event, remaining, |overlapped| unsafe {
            WriteFile(handle, Some(buffer), None, Some(overlapped))
        })
        .map(|written| written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The SID of the user this process runs as, in string form (`S-1-5-...`).
pub fn current_user_sid() -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: the out pointer is a live local; the token handle is wrapped and closed on return.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(to_io)?;
    let token = OwnedHandle(token);

    let mut needed = 0u32;
    // SAFETY: the first call only asks how large the buffer must be; it is expected to fail with that size.
    unsafe {
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
    }
    // u64 elements keep the buffer 8-byte aligned, as TOKEN_USER requires.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer is at least `needed` bytes and lives past the call.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            (buffer.len() * size_of::<u64>()) as u32,
            &mut needed,
        )
    }
    .map_err(to_io)?;

    // SAFETY: the buffer now holds an aligned TOKEN_USER written by GetTokenInformation.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: `user.User.Sid` points into `buffer`, which is alive; `text` receives a LocalAlloc'd string.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(to_io)?;
    // SAFETY: `text` is a valid NUL-terminated string until freed just below.
    let sid = unsafe { text.to_string() }.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc; freed exactly once.
    unsafe {
        windows::Win32::Foundation::LocalFree(HLOCAL(text.0.cast()));
    }
    sid
}
