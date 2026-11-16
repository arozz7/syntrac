//! The one request SYNTRAC makes to the internet: downloading the IP-to-owner dataset, only when the user asks (docs/OWNER-PLAN.md).
//!
//! Uses Windows' own WinHTTP, so the system's certificate store and proxy settings apply and no HTTP crate is added. What keeps
//! it safe: the address is compiled in (the service never takes one from a client or a file), a redirect is refused rather than
//! followed, the download and the unpacked text have size caps (no zip bomb), every wait has a timeout, and the result is only a
//! text file that the pure parser reads: nothing downloaded is ever run or opened by anything but the parser.

use std::ffi::c_void;
use std::io::Read;

use sha2::{Digest, Sha256};
use syntrac_core::{DatasetFetcher, FetchError, FetchedFile};
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_QUERY_CONTENT_LENGTH,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

/// The dataset file: IPv4 and IPv6 together, public domain (PDDL v1.0) as the site states.
pub const OWNER_DATA_URL: &str = "https://iptoasn.com/data/ip2asn-combined.tsv.gz";

/// Transfer limits. Defaults are far above the real file and far below anything that could hurt.
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    /// The file as downloaded.
    pub max_download_bytes: u64,
    /// The text after unpacking.
    pub max_text_bytes: u64,
    /// How long to wait for the next piece of the answer.
    pub receive_timeout_ms: i32,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self { max_download_bytes: 64 * 1024 * 1024, max_text_bytes: 256 * 1024 * 1024, receive_timeout_ms: 60_000 }
    }
}

const CONNECT_TIMEOUT_MS: i32 = 15_000;
const SEND_TIMEOUT_MS: i32 = 30_000;
const READ_CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// Path and query, starting with `/`.
    pub path: String,
}

fn refuse(message: impl Into<String>) -> FetchError {
    FetchError(message.into())
}

/// Only `http` and `https`, with a host, no login in the address, no fragment. (The service only ever uses the compiled-in
/// `https` address; `http` exists for tests against the loopback interface.)
pub fn parse_url(url: &str) -> Result<ParsedUrl, FetchError> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(refuse("the address must start with http:// or https://"));
    };
    let rest = rest.split('#').next().unwrap_or("");
    let (authority, path) = match rest.find(['/', '?']) {
        Some(at) if rest[at..].starts_with('?') => (&rest[..at], format!("/{}", &rest[at..])),
        Some(at) => (&rest[..at], rest[at..].to_owned()),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() || authority.contains('@') || authority.contains('[') {
        return Err(refuse("the address has no usable host"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| refuse("the address has a bad port"))?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(refuse("the address has no usable host"));
    }
    Ok(ParsedUrl { secure, host: host.to_owned(), port, path })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void, what: &str) -> Result<Self, FetchError> {
        if raw.is_null() {
            Err(refuse(format!("could not {what}: {}", windows::core::Error::from_win32().message())))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from WinHTTP and is closed exactly once, here.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn step(what: &str, result: windows::core::Result<()>) -> Result<(), FetchError> {
    result.map_err(|error| refuse(format!("{what}: {}", error.message())))
}

pub struct WinHttpFetcher {
    url: ParsedUrl,
    limits: FetchLimits,
}

impl WinHttpFetcher {
    pub fn new(url: &str, limits: FetchLimits) -> Result<Self, FetchError> {
        Ok(Self { url: parse_url(url)?, limits })
    }

    /// The fetcher for the compiled-in dataset address with the default limits.
    pub fn for_owner_data() -> Self {
        Self::new(OWNER_DATA_URL, FetchLimits::default()).expect("the compiled-in address is valid")
    }

    /// Sends the request and reads the answer into memory, stopping at the download cap.
    fn download(&self) -> Result<Vec<u8>, FetchError> {
        let url = &self.url;
        let agent = wide("SYNTRAC");
        let host = wide(&url.host);
        let path = wide(&url.path);
        let verb = wide("GET");
        let access = if url.secure { WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY } else { WINHTTP_ACCESS_TYPE_NO_PROXY };
        let flags = if url.secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };

        // SAFETY: every pointer passed points at a live, NUL-terminated buffer owned by this function for the whole call, and
        // each handle is checked and closed by `Handle`.
        unsafe {
            let session = Handle::new(
                WinHttpOpen(PCWSTR(agent.as_ptr()), access, PCWSTR::null(), PCWSTR::null(), 0),
                "start a download",
            )?;
            step(
                "set the timeouts",
                WinHttpSetTimeouts(
                    session.0,
                    CONNECT_TIMEOUT_MS,
                    CONNECT_TIMEOUT_MS,
                    SEND_TIMEOUT_MS,
                    self.limits.receive_timeout_ms,
                ),
            )?;
            let connection =
                Handle::new(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), url.port, 0), "reach the server")?;
            let request = Handle::new(
                WinHttpOpenRequest(
                    connection.0,
                    PCWSTR(verb.as_ptr()),
                    PCWSTR(path.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    flags,
                ),
                "prepare the request",
            )?;
            step(
                "refuse redirects",
                WinHttpSetOption(
                    Some(request.0 as *const c_void),
                    WINHTTP_OPTION_REDIRECT_POLICY,
                    Some(&WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes()),
                ),
            )?;
            step("send the request", WinHttpSendRequest(request.0, None, None, 0, 0, 0))?;
            step("read the answer", WinHttpReceiveResponse(request.0, std::ptr::null_mut()))?;

            let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE)
                .ok_or_else(|| refuse("the server's answer had no status"))?;
            match status {
                200 => {}
                300..=399 => return Err(refuse(format!("the server answered {status}; a redirect is refused"))),
                other => return Err(refuse(format!("the server answered {other}"))),
            }
            if let Some(length) = query_number(&request, WINHTTP_QUERY_CONTENT_LENGTH) {
                if u64::from(length) > self.limits.max_download_bytes {
                    return Err(refuse(format!(
                        "the file is {length} bytes, larger than the {} allowed",
                        self.limits.max_download_bytes
                    )));
                }
            }

            let mut bytes = Vec::new();
            let mut chunk = vec![0u8; READ_CHUNK];
            loop {
                let mut read = 0u32;
                step(
                    "read the file",
                    WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), READ_CHUNK as u32, &mut read),
                )?;
                if read == 0 {
                    return Ok(bytes);
                }
                bytes.extend_from_slice(&chunk[..read as usize]);
                if bytes.len() as u64 > self.limits.max_download_bytes {
                    return Err(refuse(format!(
                        "the file is larger than the {} bytes allowed",
                        self.limits.max_download_bytes
                    )));
                }
            }
        }
    }
}

/// A numeric header (the status code, the content length), `None` when the server did not send it.
///
/// SAFETY (caller): `request` is a live request handle whose response has been received.
unsafe fn query_number(request: &Handle, header: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut length = std::mem::size_of::<u32>() as u32;
    // SAFETY: `value` and `length` outlive the call and `length` is the size of `value`.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            header | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut value as *mut u32).cast()),
            &mut length,
            std::ptr::null_mut(),
        )
    }
    .ok()
    .map(|()| value)
}

/// Unpacks a gzip file into text, never producing more than `max_text_bytes`.
fn unpack(compressed: &[u8], max_text_bytes: u64) -> Result<String, FetchError> {
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(compressed)
        .take(max_text_bytes + 1)
        .read_to_end(&mut text)
        .map_err(|error| refuse(format!("the file is not a complete gzip file ({error})")))?;
    if text.len() as u64 > max_text_bytes {
        return Err(refuse(format!(
            "the unpacked text is larger than the {max_text_bytes} bytes allowed; not unpacked"
        )));
    }
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// Lower-case hex, as `file_hash::sha256_hex` writes a hash.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl DatasetFetcher for WinHttpFetcher {
    fn fetch(&self) -> Result<FetchedFile, FetchError> {
        let bytes = self.download()?;
        let text = unpack(&bytes, self.limits.max_text_bytes)?;
        Ok(FetchedFile { text, source: self.url.host.clone(), sha256: hex(&Sha256::digest(&bytes)) })
    }
}
