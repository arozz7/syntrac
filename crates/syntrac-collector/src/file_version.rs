//! The version resource of a file (company, product, description, original file name, version), read with the operating
//! system's own loader (`GetFileVersionInfoExW`), not a parser of ours. It is a label anyone can write, never proof.
//!
//! The neutral (language independent) resource is asked for, so the answer comes from the file itself and not from a
//! translated `.mui` beside it. Every problem gives an EMPTY result: a file without a version resource is normal.

use syntrac_core::VersionInfo;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoExW, GetFileVersionInfoSizeExW, VerQueryValueW, FILE_VER_GET_NEUTRAL,
};

use crate::file_parse::{sanitize, translation_pairs, version_value_path};

/// Tried after the languages the file declares: English with Unicode, English with Windows-1252, and no language.
const FALLBACK_TRANSLATIONS: [(u16, u16); 3] = [(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)];

/// A resource bigger than this is not a version resource worth reading.
const MAX_RESOURCE_BYTES: u32 = 1024 * 1024;

pub fn read_version(path: &str) -> VersionInfo {
    let name = HSTRING::from(path);
    let mut ignored = 0_u32;
    // SAFETY: `name` outlives the call and `ignored` is a live local.
    let size = unsafe { GetFileVersionInfoSizeExW(FILE_VER_GET_NEUTRAL, &name, &mut ignored) };
    if size == 0 || size > MAX_RESOURCE_BYTES {
        return VersionInfo::default();
    }
    let mut block = vec![0_u8; size as usize];
    // SAFETY: `block` has exactly `size` bytes, as the call requires.
    if unsafe { GetFileVersionInfoExW(FILE_VER_GET_NEUTRAL, &name, 0, size, block.as_mut_ptr().cast()) }.is_err() {
        return VersionInfo::default();
    }

    let mut translations =
        query_bytes(&block, r"\VarFileInfo\Translation").map(|b| translation_pairs(&b)).unwrap_or_default();
    translations.extend(FALLBACK_TRANSLATIONS);

    let text = |key: &str| {
        translations.iter().find_map(|(language, code_page)| {
            query_text(&block, &version_value_path(*language, *code_page, key)).and_then(|value| sanitize(&value))
        })
    };
    VersionInfo {
        company: text("CompanyName"),
        product: text("ProductName"),
        description: text("FileDescription"),
        original_filename: text("OriginalFilename"),
        file_version: text("FileVersion"),
    }
}

/// The bytes of one sub-block, copied out of the resource.
fn query_bytes(block: &[u8], sub_block: &str) -> Option<Vec<u8>> {
    let (pointer, length) = query(block, sub_block)?;
    // SAFETY: `VerQueryValueW` returned a pointer to `length` bytes inside `block`, which is still alive and unchanged.
    Some(unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length as usize) }.to_vec())
}

/// One string sub-block (zero-terminated UTF-16; the length counts characters).
fn query_text(block: &[u8], sub_block: &str) -> Option<String> {
    let (pointer, length) = query(block, sub_block)?;
    // SAFETY: as above; the string is `length` UTF-16 units, which is `2 * length` bytes. Bytes are read, not u16s, because
    // the resource buffer is not guaranteed to be 2-byte aligned.
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), (length as usize).saturating_mul(2)) };
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn query(block: &[u8], sub_block: &str) -> Option<(*mut core::ffi::c_void, u32)> {
    let wide = HSTRING::from(sub_block);
    let mut pointer: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut length = 0_u32;
    // SAFETY: `block` is a complete version resource; the out-parameters are live locals.
    let found = unsafe { VerQueryValueW(block.as_ptr().cast(), PCWSTR(wide.as_ptr()), &mut pointer, &mut length) };
    (found.as_bool() && !pointer.is_null() && length > 0).then_some((pointer, length))
}
