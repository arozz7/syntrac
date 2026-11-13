//! Builds the NT-device-to-drive-letter table from the live system.

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::QueryDosDeviceW;

use crate::paths::NtPathConverter;

/// Asks Windows which device each drive letter points at (for example `C:` -> `\Device\HarddiskVolume7`).
/// Drive letters that do not exist are skipped.
pub fn system_path_converter() -> NtPathConverter {
    let mut pairs = Vec::new();
    for letter in 'A'..='Z' {
        let drive = format!("{letter}:");
        let mut buffer = [0u16; 512];
        // SAFETY: the buffer is a live local slice; QueryDosDeviceW writes at most buffer.len() UTF-16 units.
        let written = unsafe { QueryDosDeviceW(&HSTRING::from(drive.as_str()), Some(&mut buffer)) };
        if written == 0 {
            continue;
        }
        // The result is a double-NUL-terminated list; the first entry is the primary target.
        let first = buffer.split(|unit| *unit == 0).next().unwrap_or_default();
        let device = String::from_utf16_lossy(first);
        if !device.is_empty() {
            pairs.push((device, drive));
        }
    }
    NtPathConverter::new(pairs)
}
