//! Where the UI looks for the collector. Defaults match the installed service; environment variables let a
//! developer point the UI at a console collector instead (`SYNTRAC_DB`, `SYNTRAC_PIPE`).

use std::path::PathBuf;

const DEFAULT_PROGRAM_DATA: &str = r"C:\ProgramData";
const DEFAULT_PIPE: &str = "syntrac-collector";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    /// The database to read: an explicit choice when `database_pinned`, otherwise only the default, which the app
    /// replaces with whatever the running collector reports (the data may have been moved to another drive).
    pub database_path: PathBuf,
    /// True when the path was chosen explicitly (`SYNTRAC_DB`); the collector's report must not override it.
    pub database_pinned: bool,
    /// Where the last known database path is remembered between runs. None: nothing is remembered.
    pub memory_file: Option<PathBuf>,
    /// Pipe name only, not the full path.
    pub pipe_name: String,
}

impl AppSettings {
    /// `get` looks up an environment variable; injected so the rules are testable without touching the real one.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let present = |key: &str| get(key).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        let program_data = present("ProgramData").unwrap_or_else(|| DEFAULT_PROGRAM_DATA.to_owned());
        let explicit = present("SYNTRAC_DB").map(PathBuf::from);
        let database_pinned = explicit.is_some();
        let database_path =
            explicit.unwrap_or_else(|| PathBuf::from(program_data).join("Syntrac").join("data").join("syntrac.db"));
        let memory_file =
            present("LOCALAPPDATA").map(|local| PathBuf::from(local).join("Syntrac").join("database-path.txt"));
        Self {
            database_path,
            database_pinned,
            memory_file,
            pipe_name: present("SYNTRAC_PIPE").unwrap_or_else(|| DEFAULT_PIPE.to_owned()),
        }
    }

    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// One known database, never replaced and never remembered (tests, tools).
    pub fn for_database(database_path: PathBuf, pipe_name: &str) -> Self {
        Self { database_path, database_pinned: true, memory_file: None, pipe_name: pipe_name.to_owned() }
    }
}
