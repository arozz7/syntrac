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
    /// How long detail (events, flows, per-process samples) is kept, in days. Detail is deleted only once it is also rolled up.
    /// The old name `retention_days` is still accepted.
    #[serde(alias = "retention_days")]
    pub detail_retention_days: u32,
    pub purge_interval_minutes: u64,
    /// Storage guard: the most live data the database may hold, in GB. Over it, the oldest rolled-up detail is trimmed early.
    pub max_database_gb: u64,
    /// Storage guard: free space on the data drive, in GB, below which the oldest rolled-up detail is trimmed early.
    pub min_free_space_gb: u64,
    /// Storage guard: free space in GB below which detail collection pauses (rollups and status continue). Must be less than
    /// `min_free_space_gb`.
    pub hard_floor_free_gb: u64,
    /// How often completed hours are rolled up into the permanent summaries (a backlog is worked off sooner).
    pub rollup_interval_minutes: u64,
    /// Identity worker (docs/IDENTITY-PLAN.md): how often it looks for executables whose files have not been read yet.
    pub identity_interval_minutes: u64,
    /// Identity worker: files read per pass (a backlog is worked off at this many every identity_backlog_pause_secs).
    pub identity_files_per_pass: usize,
    /// Identity worker: seconds between two passes while a backlog remains.
    pub identity_backlog_pause_secs: u64,
    /// Identity worker: a file larger than this many MB is not hashed (its size is still recorded).
    pub identity_max_hash_mb: u64,
    /// Context worker (docs/RUNAS-PLAN.md): whether to record which account each program runs as and which services it hosts.
    pub context_enabled: bool,
    /// Context worker: seconds between two looks at the running programs.
    pub context_interval_secs: u64,
    /// Context worker: a program that keeps running has its "last seen" moved this often (minutes), not every look.
    pub context_refresh_minutes: u64,
    /// Whether the service may ever download the owner dataset (docs/OWNER-PLAN.md). The download itself still happens only when
    /// the user asks for it in the app; `false` makes the service refuse even then. The only network use SYNTRAC has.
    pub owner_data_allowed: bool,
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
            detail_retention_days: 14,
            purge_interval_minutes: 60,
            max_database_gb: 10,
            min_free_space_gb: 20,
            hard_floor_free_gb: 5,
            rollup_interval_minutes: 5,
            identity_interval_minutes: 5,
            identity_files_per_pass: 5,
            identity_backlog_pause_secs: 30,
            identity_max_hash_mb: 512,
            context_enabled: true,
            context_interval_secs: 30,
            context_refresh_minutes: 10,
            owner_data_allowed: true,
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
            (u64::from(self.detail_retention_days), "detail_retention_days"),
            (self.max_database_gb, "max_database_gb"),
            (self.min_free_space_gb, "min_free_space_gb"),
            (self.hard_floor_free_gb, "hard_floor_free_gb"),
            (self.purge_interval_minutes, "purge_interval_minutes"),
            (self.rollup_interval_minutes, "rollup_interval_minutes"),
            (self.identity_interval_minutes, "identity_interval_minutes"),
            (self.identity_files_per_pass as u64, "identity_files_per_pass"),
            (self.identity_backlog_pause_secs, "identity_backlog_pause_secs"),
            (self.identity_max_hash_mb, "identity_max_hash_mb"),
            (self.context_interval_secs, "context_interval_secs"),
            (self.context_refresh_minutes, "context_refresh_minutes"),
            (self.top_processes as u64, "top_processes"),
            (self.status_interval_secs, "status_interval_secs"),
            (self.etw_silent_restart_secs, "etw_silent_restart_secs"),
        ];
        if let Some(sid) = &self.pipe_allowed_sid {
            syntrac_ipc::sddl::validate_sid(sid).map_err(|e| ConfigError::Invalid(format!("pipe_allowed_sid: {e}")))?;
        }
        if let Some((_, name)) = checks.iter().find(|(value, _)| *value == 0) {
            return Err(ConfigError::Invalid(format!("{name} must be greater than zero")));
        }
        if self.hard_floor_free_gb >= self.min_free_space_gb {
            return Err(ConfigError::Invalid(format!(
                "hard_floor_free_gb ({}) must be less than min_free_space_gb ({})",
                self.hard_floor_free_gb, self.min_free_space_gb
            )));
        }
        Ok(())
    }
}
