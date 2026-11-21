//! The real store of start with Windows: one text value in a key of the signed-in user's own registry hive (`HKEY_CURRENT_USER`).
//! A thin adapter; the rules are in `autostart`.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE,
};

use crate::autostart::{AutostartStore, VALUE_NAME};

/// The key every program that starts at sign-in uses (Task Manager's Startup tab lists it).
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// A start-up command is never this long; a longer value is refused rather than half read.
const MAX_VALUE_BYTES: u32 = 64 * 1024;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn describe(what: &str, status: WIN32_ERROR) -> String {
    format!("{what} failed (Windows error {})", status.0)
}

/// An open key, closed when dropped.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by this program and is closed exactly once, here.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

pub struct RegistryAutostart {
    subkey: String,
}

impl RegistryAutostart {
    /// The real thing: the user's own Run key.
    pub fn new() -> Self {
        Self::at(RUN_KEY)
    }

    /// Another key of the user's hive (for tests, which must never write the Run key).
    pub fn at(subkey: &str) -> Self {
        Self { subkey: subkey.to_owned() }
    }

    /// `Ok(None)` when the key does not exist.
    fn open(&self, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<Option<Key>, String> {
        let name = wide(&self.subkey);
        let mut key = HKEY::default();
        // SAFETY: `name` is NUL-terminated and outlives the call; `key` receives the handle, closed in `Drop`.
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr()), 0, access, &mut key) };
        match status {
            ERROR_SUCCESS => Ok(Some(Key(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            other => Err(describe("Opening the start-up key", other)),
        }
    }
}

impl Default for RegistryAutostart {
    fn default() -> Self {
        Self::new()
    }
}

impl AutostartStore for RegistryAutostart {
    fn registered(&self) -> Result<Option<String>, String> {
        let Some(key) = self.open(KEY_READ)? else { return Ok(None) };
        let name = wide(VALUE_NAME);
        let mut kind = REG_VALUE_TYPE::default();
        let mut bytes = 0u32;
        // SAFETY: with no data buffer the call only reports the type and the size into the two live locals.
        let status =
            unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, Some(&mut kind), None, Some(&mut bytes)) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
            return Err(describe("Reading the start-up entry", status));
        }
        if (kind != REG_SZ && kind != REG_EXPAND_SZ) || bytes > MAX_VALUE_BYTES {
            return Err("The start-up entry is not plain text.".to_owned());
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
        let mut length = bytes;
        // SAFETY: `buffer` holds at least `length` bytes (2 per character) and outlives the call.
        let status = unsafe {
            RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut length),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(describe("Reading the start-up entry", status));
        }
        let characters = (length as usize / 2).min(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..characters]);
        Ok(Some(text.trim_end_matches('\0').to_owned()))
    }

    fn register(&self, command: &str) -> Result<(), String> {
        let subkey = wide(&self.subkey);
        let mut key = HKEY::default();
        // SAFETY: `subkey` is NUL-terminated and outlives the call; `key` receives the handle, closed in `Drop`.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(describe("Opening the start-up key for writing", status));
        }
        let key = Key(key);
        let name = wide(VALUE_NAME);
        let data = wide(command);
        // SAFETY: the byte view covers exactly the characters (and the final NUL) of `data`, which outlives the call.
        let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let status = unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), 0, REG_SZ, Some(bytes)) };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(describe("Writing the start-up entry", status))
        }
    }

    fn unregister(&self) -> Result<(), String> {
        let Some(key) = self.open(KEY_SET_VALUE)? else { return Ok(()) };
        let name = wide(VALUE_NAME);
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let status = unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(describe("Removing the start-up entry", status))
        }
    }
}

/// Removes a throwaway key a test made. Only keys the tests make (`Software\Syntrac\AutostartTest-...`) can be removed this way; the Run
/// key, or any other, is refused.
pub fn delete_key_for_tests(subkey: &str) {
    const TEST_PREFIX: &str = r"Software\Syntrac\AutostartTest-";
    if !subkey.starts_with(TEST_PREFIX) {
        return;
    }
    let name = wide(subkey);
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr())) };
}
