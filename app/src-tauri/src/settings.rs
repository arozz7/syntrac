//! Where the UI looks for the collector. Defaults match the installed service; environment variables let a
//! developer point the UI at a console collector instead (`SYNTRAC_DB`, `SYNTRAC_PIPE`).

use std::path::PathBuf;

const DEFAULT_PROGRAM_DATA: &str = r"C:\ProgramData";
const DEFAULT_PIPE: &str = "syntrac-collector";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub database_path: PathBuf,
    /// Pipe name only, not the full path.
    pub pipe_name: String,
}

impl AppSettings {
    /// `get` looks up an environment variable; injected so the rules are testable without touching the real one.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let present = |key: &str| get(key).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        let program_data = present("ProgramData").unwrap_or_else(|| DEFAULT_PROGRAM_DATA.to_owned());
        let database_path = present("SYNTRAC_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(program_data).join("Syntrac").join("data").join("syntrac.db"));
        Self { database_path, pipe_name: present("SYNTRAC_PIPE").unwrap_or_else(|| DEFAULT_PIPE.to_owned()) }
    }

    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}
