//! Reading the Windows registry, read only. A small wrapper over the Win32 calls: open a key, list its values (text values only)
//! and its subkeys. Used by the launch-record sources (Run keys) and, later, the install records.
//!
//! Nothing here writes, creates or deletes a key. Values are returned as they are stored: a `REG_EXPAND_SZ` keeps its `%NAME%`
//! parts, because expanding them is the caller's decision (an entry that belongs to one user must use that user's folders).

use syntrac_core::clean_text;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS,
    KEY_READ, KEY_WOW64_64KEY, REG_DWORD, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};

/// Longest value name the registry allows is 16,383 characters; a key name 255.
const MAX_NAME_CHARS: usize = 16_384;
/// A value larger than this is skipped (a startup command is never this long).
const MAX_VALUE_BYTES: usize = 64 * 1024;
/// A key with more entries than this is read only that far.
const MAX_ENTRIES: u32 = 10_000;
/// Longest text kept from a value.
const MAX_TEXT_CHARS: usize = 2_048;

/// The two roots the SYNTRAC sources read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    LocalMachine,
    Users,
}

impl Root {
    fn handle(self) -> HKEY {
        match self {
            Self::LocalMachine => HKEY_LOCAL_MACHINE,
            Self::Users => HKEY_USERS,
        }
    }
}

/// An open key, closed when dropped.
pub struct RegKey(HKEY);

impl RegKey {
    /// Opens `subkey` of `root` for reading, in the 64-bit view (the 32-bit programs' entries are under `WOW6432Node` there).
    /// `None` when it does not exist or may not be read.
    pub fn open(root: Root, subkey: &str) -> Option<Self> {
        let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = HKEY::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `key` receives the handle, closed in `Drop`.
        let status =
            unsafe { RegOpenKeyExW(root.handle(), PCWSTR(wide.as_ptr()), 0, KEY_READ | KEY_WOW64_64KEY, &mut key) };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    /// The names of the subkeys (at most `MAX_ENTRIES`).
    pub fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        for index in 0..MAX_ENTRIES {
            let mut buffer = vec![0u16; 512];
            let mut length = buffer.len() as u32;
            // SAFETY: `buffer` holds `length` characters and outlives the call.
            let status = unsafe {
                RegEnumKeyExW(self.0, index, PWSTR(buffer.as_mut_ptr()), &mut length, None, PWSTR::null(), None, None)
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != ERROR_SUCCESS {
                continue;
            }
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
        names
    }

    /// The text values (`REG_SZ` and `REG_EXPAND_SZ`) as (name, value), the value as stored (variables not expanded) and
    /// cleaned of control characters. Values of other types, and values that are too large, are left out.
    pub fn text_values(&self) -> Vec<(String, String)> {
        let mut values = Vec::new();
        // The buffers are made once, not per value: a key such as an Uninstall entry has a dozen values and a machine hundreds of keys.
        let mut name = vec![0u16; MAX_NAME_CHARS];
        let mut data = vec![0u8; MAX_VALUE_BYTES];
        for index in 0..MAX_ENTRIES {
            let mut name_length = name.len() as u32;
            let mut data_length = data.len() as u32;
            let mut kind = 0u32;
            // SAFETY: both buffers hold the lengths passed and outlive the call.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    PWSTR(name.as_mut_ptr()),
                    &mut name_length,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_length),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status == ERROR_MORE_DATA || status != ERROR_SUCCESS {
                continue;
            }
            if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
                continue;
            }
            let units: Vec<u16> =
                data[..data_length as usize].chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
            let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
            let text = String::from_utf16_lossy(&units[..end]);
            values.push((
                String::from_utf16_lossy(&name[..name_length as usize]),
                clean_text(&text, MAX_TEXT_CHARS).unwrap_or_default(),
            ));
        }
        values
    }

    /// A number value (`REG_DWORD`) by name; `None` when it is not there or is not a number.
    pub fn dword(&self, name: &str) -> Option<u32> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let (mut kind, mut value, mut length) = (REG_VALUE_TYPE(0), 0u32, 4u32);
        // SAFETY: `wide` is NUL-terminated; `value` holds the four bytes passed as the length.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(wide.as_ptr()),
                None,
                Some(&mut kind),
                Some((&mut value as *mut u32).cast()),
                Some(&mut length),
            )
        };
        (status == ERROR_SUCCESS && kind == REG_DWORD && length == 4).then_some(value)
    }

    /// One text value by name (`""` is the key's default value), as stored.
    pub fn text(&self, name: &str) -> Option<String> {
        self.text_values().into_iter().find(|(found, _)| found.eq_ignore_ascii_case(name)).map(|(_, value)| value)
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `open` and is closed exactly once, here.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}
