//! Service configuration, loaded from TOML. Every field has a safe default, unknown keys are
//! rejected (so a typo cannot silently do nothing), and values that would break the run loop are refused.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not parse the configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServiceConfig {
    /// Database file. When absent the host chooses a default location.
    pub database_path: Option<PathBuf>,
    /// Directory for rolling log files (service mode). Must be an absolute path there.
    pub log_dir: Option<PathBuf>,
    /// The user (SID, `S-1-5-21-...`) allowed to talk to the collector's status pipe, in addition to SYSTEM and
    /// Administrators. The installer fills this in. Validated at load so it can never inject extra access entries.
    pub pipe_allowed_sid: Option<String>,
    /// Network flows are aggregated into windows of this length.
    pub flow_window_secs: u64,
    /// A flow window is stored this many seconds after it ends, so late-arriving events still join it.
    pub flow_flush_grace_secs: u64,
    /// How often the host drains the telemetry sources.
    pub drain_interval_ms: u64,
    pub metrics_interval_secs: u64,
    /// How many of the heaviest processes are recorded individually per metrics sample.
    pub top_processes: usize,
    pub retention_days: u32,
    pub purge_interval_minutes: u64,
    /// How often a status line (events stored, drops, own CPU) is logged.
    pub status_interval_secs: u64,
    /// If ETW delivers nothing for this long, the trace session is restarted.
    pub etw_silent_restart_secs: u64,
    /// How many restarts in a row are tried before giving up (the problem stays visible in the health banner).
    /// 0 means watch and report only.
    pub etw_max_restarts: u32,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            database_path: None,
            log_dir: None,
            pipe_allowed_sid: None,
            flow_window_secs: 10,
            flow_flush_grace_secs: 3,
            drain_interval_ms: 500,
            metrics_interval_secs: 5,
            top_processes: 25,
            retention_days: 30,
            purge_interval_minutes: 60,
            status_interval_secs: 30,
            etw_silent_restart_secs: 180,
            etw_max_restarts: 3,
        }
    }
}

impl ServiceConfig {
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let checks = [
            (self.flow_window_secs, "flow_window_secs"),
            (self.flow_flush_grace_secs, "flow_flush_grace_secs"),
            (self.drain_interval_ms, "drain_interval_ms"),
            (self.metrics_interval_secs, "metrics_interval_secs"),
            (u64::from(self.retention_days), "retention_days"),
            (self.purge_interval_minutes, "purge_interval_minutes"),
            (self.top_processes as u64, "top_processes"),
            (self.status_interval_secs, "status_interval_secs"),
            (self.etw_silent_restart_secs, "etw_silent_restart_secs"),
        ];
        if let Some(sid) = &self.pipe_allowed_sid {
            syntrac_ipc::sddl::validate_sid(sid).map_err(|e| ConfigError::Invalid(format!("pipe_allowed_sid: {e}")))?;
        }
        match checks.iter().find(|(value, _)| *value == 0) {
            Some((_, name)) => Err(ConfigError::Invalid(format!("{name} must be greater than zero"))),
            None => Ok(()),
        }
    }
}
