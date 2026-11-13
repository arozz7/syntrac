//! Path rules for running as a Windows service. Pure, so they are unit-tested without installing anything.

use std::path::{Path, PathBuf};

use crate::ServiceConfig;

#[derive(Debug, thiserror::Error)]
pub enum ServicePathError {
    #[error("`{key}` must be set in the service configuration")]
    Missing { key: &'static str },
    #[error("`{key}` must be an absolute path (a service starts in System32), got `{value}`")]
    NotAbsolute { key: &'static str, value: String },
    #[error("`{key}` is on drive {drive}, which is not available. Is it connected and powered on? The service is restarted automatically and starts working once the drive is back.")]
    DriveNotAvailable { key: &'static str, drive: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePaths {
    pub database_path: PathBuf,
    pub log_dir: PathBuf,
}

/// The configuration lives next to the `bin` folder, inside the install directory. That directory is
/// writable only by administrators; the service reads this file as SYSTEM, so it must not sit in the
/// user-writable data folder.
pub fn service_config_path(exe_dir: &Path) -> PathBuf {
    exe_dir.parent().unwrap_or(exe_dir).join("config").join("service.toml")
}

pub fn validate_service_paths(config: &ServiceConfig) -> Result<ServicePaths, ServicePathError> {
    Ok(ServicePaths {
        database_path: absolute("database_path", config.database_path.as_deref())?,
        log_dir: absolute("log_dir", config.log_dir.as_deref())?,
    })
}

fn absolute(key: &'static str, value: Option<&Path>) -> Result<PathBuf, ServicePathError> {
    let value = value.ok_or(ServicePathError::Missing { key })?;
    if !value.is_absolute() {
        return Err(ServicePathError::NotAbsolute { key, value: value.display().to_string() });
    }
    Ok(value.to_path_buf())
}

/// The data may live on a drive other than the system drive (it is configurable). If that drive is missing, say so
/// plainly rather than failing later with a confusing "cannot create folder". Only the DRIVE is checked: a missing
/// folder on a present drive is normal on first run, and the service creates it.
pub fn check_data_drives(paths: &ServicePaths) -> Result<(), ServicePathError> {
    for (key, path) in [("database_path", &paths.database_path), ("log_dir", &paths.log_dir)] {
        if let Some(drive) = missing_drive(path) {
            return Err(ServicePathError::DriveNotAvailable { key, drive });
        }
    }
    Ok(())
}

/// `Some("O:")` when the path starts with a drive (or share) whose root does not exist.
fn missing_drive(path: &Path) -> Option<String> {
    let Some(std::path::Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    let drive = prefix.as_os_str().to_string_lossy().into_owned();
    let root = PathBuf::from(format!("{drive}\\"));
    if root.exists() {
        None
    } else {
        Some(drive)
    }
}
