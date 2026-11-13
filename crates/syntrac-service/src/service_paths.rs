//! Path rules for running as a Windows service. Pure, so they are unit-tested without installing anything.

use std::path::{Path, PathBuf};

use crate::ServiceConfig;

#[derive(Debug, thiserror::Error)]
pub enum ServicePathError {
    #[error("`{key}` must be set in the service configuration")]
    Missing { key: &'static str },
    #[error("`{key}` must be an absolute path (a service starts in System32), got `{value}`")]
    NotAbsolute { key: &'static str, value: String },
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
