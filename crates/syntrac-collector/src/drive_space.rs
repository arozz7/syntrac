//! Free space on the drive that holds the database, for the storage guard.
//!
//! Asks Windows about the data FOLDER itself (`GetDiskFreeSpaceExW`), not a drive letter, so a drive mounted as a folder or
//! a path on another volume is measured correctly. The guard's rules are pure and tested against a fake volume; this is only
//! the thin adapter.

use std::path::PathBuf;

use syntrac_core::storage_guard::FreeSpace;
use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

pub struct DriveFreeSpace {
    folder: PathBuf,
}

impl DriveFreeSpace {
    /// `folder` must exist (the database's own folder does).
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }
}

impl FreeSpace for DriveFreeSpace {
    fn free_bytes(&self) -> Result<u64, String> {
        let mut available = 0_u64;
        // SAFETY: `available` is a live local; the other two out-parameters are optional and passed as None.
        let result =
            unsafe { GetDiskFreeSpaceExW(&HSTRING::from(self.folder.as_os_str()), Some(&mut available), None, None) };
        match result {
            Ok(()) => Ok(available),
            Err(error) => Err(format!("could not read the free space of {}: {error}", self.folder.display())),
        }
    }
}
