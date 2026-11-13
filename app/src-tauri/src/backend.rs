//! The backend behind the Tauri commands. It opens the database READ-ONLY on every call (so no long-lived read
//! transaction can hold back the collector''s WAL checkpoints) and turns every problem into an explicit state the UI
//! can show, never a raw error.

use std::time::Duration;

use serde::Serialize;
use syntrac_core::{Clock, MetricsRepository, RepoError, Timestamp};
use syntrac_ipc::pipe::request;
use syntrac_ipc::protocol::{Request, Response};
use syntrac_store::SqliteStore;

use crate::db_location::{DbLocation, FileMemory, NoMemory, PathMemory};
use crate::health::{assess_health, HealthView};
use crate::settings::AppSettings;
use crate::views::{
    connection_list, overview, process_list, ConnectionRowView, OverviewView, ProcessListParams, ProcessRowView,
};

const PIPE_TIMEOUT: Duration = Duration::from_millis(1500);
/// How far back to look for the newest metrics sample when judging data freshness.
const FRESHNESS_LOOKBACK_SECS: i64 = 600;

/// What a data call produced. `state` tells the UI which screen to show.
#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Data<T> {
    Ok {
        data: T,
    },
    /// No database file: the collector is probably not installed or has never run.
    NoDatabase {
        message: String,
    },
    /// The database was written by a different version of SYNTRAC.
    SchemaMismatch {
        message: String,
    },
    Error {
        message: String,
    },
}

pub struct Backend<C> {
    pipe_name: String,
    location: DbLocation,
    clock: C,
}

impl<C: Clock + Send + Sync> Backend<C> {
    pub fn new(settings: AppSettings, clock: C) -> Self {
        let memory: Box<dyn PathMemory> = match settings.memory_file {
            Some(file) => Box::new(FileMemory::new(file)),
            None => Box::new(NoMemory),
        };
        let location = DbLocation::new(settings.database_path, settings.database_pinned, memory);
        Self { pipe_name: settings.pipe_name, location, clock }
    }

    /// Asks the collector how it is doing. Also keeps the database location up to date: the data may have been moved.
    fn ask_collector(&self) -> Result<syntrac_ipc::protocol::StatusReport, syntrac_ipc::pipe::ClientError> {
        let report = request(&self.pipe_name, &Request::Status, PIPE_TIMEOUT).and_then(|response| match response {
            Response::Status(report) => Ok(report),
            other => Err(syntrac_ipc::pipe::ClientError::Protocol(format!("{other:?}"))),
        })?;
        self.location.learn(&report.database_path);
        Ok(report)
    }

    fn open(&self) -> Result<SqliteStore, Data<()>> {
        let mut path = self.location.current();
        if !path.exists() {
            // Not where we expected. The data may have been moved: ask the collector where it is now.
            let _ = self.ask_collector();
            path = self.location.current();
        }
        if !path.exists() {
            return Err(Data::NoDatabase {
                message: format!(
                    "There is no database at {}. Is the collector installed and has it run? \
                     (If the data lives on another drive, is that drive connected?)",
                    path.display()
                ),
            });
        }
        SqliteStore::open_read_only(&path).map_err(|error| match error {
            RepoError::SchemaMismatch { found, expected } => Data::SchemaMismatch {
                message: format!(
                    "The database is schema v{found} but this app needs v{expected}. Start the matching collector once so it can migrate the database."
                ),
            },
            other => Data::Error { message: format!("Could not open the database: {other}") },
        })
    }

    fn with_store<T>(&self, work: impl FnOnce(&SqliteStore, Timestamp) -> Result<T, RepoError>) -> Data<T> {
        let store = match self.open() {
            Ok(store) => store,
            Err(Data::NoDatabase { message }) => return Data::NoDatabase { message },
            Err(Data::SchemaMismatch { message }) => return Data::SchemaMismatch { message },
            Err(Data::Error { message }) => return Data::Error { message },
            Err(Data::Ok { .. }) => unreachable!("open() only returns problem states"),
        };
        match work(&store, self.clock.now()) {
            Ok(data) => Data::Ok { data },
            Err(error) => Data::Error { message: format!("Could not read the data: {error}") },
        }
    }

    pub fn overview(&self, range_secs: u64) -> Data<OverviewView> {
        self.with_store(|store, now| overview(store, now, range_secs))
    }

    pub fn processes(&self, params: ProcessListParams) -> Data<Vec<ProcessRowView>> {
        self.with_store(|store, _| process_list(store, &params))
    }

    pub fn connections(&self, range_secs: u64, limit: Option<usize>) -> Data<Vec<ConnectionRowView>> {
        self.with_store(|store, now| connection_list(store, now, range_secs, limit))
    }

    /// Never fails: every problem is a state.
    pub fn health(&self) -> HealthView {
        let now = self.clock.now();
        let pipe = self.ask_collector();
        let from = Timestamp::from_micros(now.as_micros().saturating_sub(FRESHNESS_LOOKBACK_SECS * 1_000_000));
        let newest = self
            .open()
            .ok()
            .and_then(|store| store.system_series(from, now).ok())
            .and_then(|series| series.last().map(|point| point.ts));
        assess_health(pipe, newest, now)
    }
}
