//! The backend behind the Tauri commands. It opens the database READ-ONLY on every call (so no long-lived read
//! transaction can hold back the collector''s WAL checkpoints) and turns every problem into an explicit state the UI
//! can show, never a raw error.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use syntrac_core::rollup::HOUR_MICROS;
use syntrac_core::{AddressScope, Clock, MetricsRepository, RepoError, RollupRepository, Timestamp};
use syntrac_ipc::pipe::request;
use syntrac_ipc::protocol::{Request, Response};
use syntrac_store::SqliteStore;

use crate::db_location::{DbLocation, FileMemory, NoMemory, PathMemory};
use crate::health::{assess_health, HealthView};
use crate::history::range_start;
use crate::history::{system_history, SystemHistoryView};
use crate::history_network::{
    destination_history, destination_range, new_destinations_view, program_destinations_view, rank_destination_range,
    rank_owner_range, DestinationHistoryView, DestinationOrderParam, DestinationRange, DestinationsView,
    NewDestinationsView, OwnersView, ProgramDestinationsView, DEFAULT_DESTINATION_ROWS,
    DEFAULT_PROGRAM_DESTINATION_ROWS,
};
use crate::history_programs::{
    program_history, range_totals, rank_range, Grouping, ProgramHistoryView, ProgramMetricParam, ProgramsView,
    RangeTotals, DEFAULT_PROGRAM_ROWS,
};
use crate::known_view::{known_programs, KnownProgramsView};
use crate::owner_data_view::OwnerDataStateView;
use crate::settings::AppSettings;
use crate::views::{
    connection_list, overview, process_list, ConnectionRowView, OverviewView, ProcessListParams, ProcessRowView,
};

const PIPE_TIMEOUT: Duration = Duration::from_millis(1500);
/// A write waits for the service to save it, which can take longer than a status check.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How far back to look for the newest metrics sample when judging data freshness.
const FRESHNESS_LOOKBACK_SECS: i64 = 600;

fn unexpected(response: &Response) -> String {
    format!("The collector answered something unexpected: {response:?}")
}

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

/// Sums over a long range are the expensive part of a ranking (a year is millions of rows), and they only change when the
/// collector rolls up another hour. So they are kept, keyed by the range start and the rollup watermark: a new hour changes the
/// key, and the old entries are dropped.
type TotalsKey = (i64, Option<i64>);
const MAX_CACHED_RANGES: usize = 16;

pub struct Backend<C> {
    pipe_name: String,
    location: DbLocation,
    clock: C,
    totals: Mutex<HashMap<TotalsKey, Arc<RangeTotals>>>,
    /// The same, for the sums of destinations (the Network tab).
    destinations: Mutex<HashMap<TotalsKey, Arc<DestinationRange>>>,
}

impl<C: Clock + Send + Sync> Backend<C> {
    pub fn new(settings: AppSettings, clock: C) -> Self {
        let memory: Box<dyn PathMemory> = match settings.memory_file {
            Some(file) => Box::new(FileMemory::new(file)),
            None => Box::new(NoMemory),
        };
        let location = DbLocation::new(settings.database_path, settings.database_pinned, memory);
        Self {
            pipe_name: settings.pipe_name,
            location,
            clock,
            totals: Mutex::new(HashMap::new()),
            destinations: Mutex::new(HashMap::new()),
        }
    }

    /// The summed range, from the cache when the collector has not rolled up another hour since it was read.
    fn cached_totals(
        &self,
        store: &SqliteStore,
        now: Timestamp,
        range_secs: u64,
    ) -> Result<Arc<RangeTotals>, RepoError> {
        let watermark = store.rolled_up_before()?.map(|w| w.as_micros());
        let key = (range_start(now, range_secs).as_micros() / HOUR_MICROS, watermark);
        if let Some(found) = self.totals.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
            return Ok(found);
        }
        let totals = Arc::new(range_totals(store, now, range_secs)?);
        if let Ok(mut cache) = self.totals.lock() {
            // An entry for an older watermark can never be asked for again; and the cache stays small whatever is asked.
            cache.retain(|(_, mark), _| *mark == watermark);
            if cache.len() >= MAX_CACHED_RANGES {
                cache.clear();
            }
            cache.insert(key, Arc::clone(&totals));
        }
        Ok(totals)
    }

    /// The summed destinations of a range, from the cache when the collector has not rolled up another hour since.
    fn cached_destinations(
        &self,
        store: &SqliteStore,
        now: Timestamp,
        range_secs: u64,
    ) -> Result<Arc<DestinationRange>, RepoError> {
        let watermark = store.rolled_up_before()?.map(|w| w.as_micros());
        let key = (range_start(now, range_secs).as_micros() / HOUR_MICROS, watermark);
        if let Some(found) = self.destinations.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
            return Ok(found);
        }
        let range = Arc::new(destination_range(store, now, range_secs)?);
        if let Ok(mut cache) = self.destinations.lock() {
            cache.retain(|(_, mark), _| *mark == watermark);
            if cache.len() >= MAX_CACHED_RANGES {
                cache.clear();
            }
            cache.insert(key, Arc::clone(&range));
        }
        Ok(range)
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

    pub fn system_history(&self, range_secs: u64) -> Data<SystemHistoryView> {
        self.with_store(|store, now| system_history(store, now, range_secs))
    }

    pub fn top_programs(
        &self,
        range_secs: u64,
        metric: ProgramMetricParam,
        limit: Option<usize>,
        grouping: Grouping,
    ) -> Data<ProgramsView> {
        self.with_store(|store, now| {
            let range = self.cached_totals(store, now, range_secs)?;
            rank_range(store, &range, metric, limit.unwrap_or(DEFAULT_PROGRAM_ROWS), grouping)
        })
    }

    pub fn program_history(&self, range_secs: u64, path: String) -> Data<ProgramHistoryView> {
        self.with_store(|store, now| program_history(store, now, range_secs, &path))
    }

    pub fn top_destinations(
        &self,
        range_secs: u64,
        order: DestinationOrderParam,
        scope: AddressScope,
        limit: Option<usize>,
    ) -> Data<DestinationsView> {
        self.with_store(|store, now| {
            let range = self.cached_destinations(store, now, range_secs)?;
            rank_destination_range(store, &range, order.into(), scope, limit.unwrap_or(DEFAULT_DESTINATION_ROWS))
        })
    }

    /// Where one program connected over a range (its own destinations, biggest first).
    pub fn program_destinations(
        &self,
        range_secs: u64,
        path: String,
        scope: AddressScope,
        limit: Option<usize>,
    ) -> Data<ProgramDestinationsView> {
        self.with_store(|store, now| {
            program_destinations_view(
                store,
                now,
                range_secs,
                &path,
                scope,
                limit.unwrap_or(DEFAULT_PROGRAM_DESTINATION_ROWS),
            )
        })
    }

    /// The destinations of a range grouped by the network that owns them (the sums are the ones the ranking keeps).
    pub fn top_owners(&self, range_secs: u64, scope: AddressScope, limit: Option<usize>) -> Data<OwnersView> {
        self.with_store(|store, now| {
            let range = self.cached_destinations(store, now, range_secs)?;
            rank_owner_range(store, &range, scope, limit.unwrap_or(DEFAULT_DESTINATION_ROWS))
        })
    }

    pub fn new_destinations(
        &self,
        window_secs: u64,
        scope: AddressScope,
        limit: Option<usize>,
    ) -> Data<NewDestinationsView> {
        self.with_store(|store, now| {
            new_destinations_view(store, now, window_secs, scope, limit.unwrap_or(DEFAULT_DESTINATION_ROWS))
        })
    }

    pub fn destination_history(&self, range_secs: u64, addr: String) -> Data<DestinationHistoryView> {
        self.with_store(|store, now| destination_history(store, now, range_secs, &addr))
    }

    pub fn known_programs(&self) -> Data<KnownProgramsView> {
        self.with_store(|store, _| known_programs(store))
    }

    /// Asks the service to mark the product of the program at path as known. The service decides (it checks the file's own
    /// identity); a refusal comes back in its own words.
    pub fn mark_known(&self, path: String, label: String, note: Option<String>) -> Result<i64, String> {
        match self.write(&Request::KnownProductAdd { path, label, note })? {
            Response::KnownProductSaved { id } => Ok(id),
            other => Err(unexpected(&other)),
        }
    }

    pub fn unmark_known(&self, id: i64) -> Result<(), String> {
        match self.write(&Request::KnownProductRemove { id })? {
            Response::Done => Ok(()),
            other => Err(unexpected(&other)),
        }
    }

    /// What the service has for owner data (docs/OWNER-PLAN.md) and whether a download is under way. Makes no network request.
    pub fn owner_data_status(&self) -> Result<OwnerDataStateView, String> {
        self.owner_data_request(&Request::OwnerDataStatus)
    }

    /// Asks the service to download the owner data. The app never downloads anything itself; the service answers at once and
    /// works in the background (ask for the status to see it finish).
    pub fn refresh_owner_data(&self) -> Result<OwnerDataStateView, String> {
        self.owner_data_request(&Request::OwnerDataRefresh)
    }

    fn owner_data_request(&self, wanted: &Request) -> Result<OwnerDataStateView, String> {
        let not_running = "The SYNTRAC collector is not running, so owner data cannot be checked or downloaded.";
        match self.send(wanted, not_running)? {
            Response::OwnerData(report) => Ok(report.into()),
            other => Err(unexpected(&other)),
        }
    }

    /// Sends a write request to the service. The app never writes the database itself.
    fn write(&self, wanted: &Request) -> Result<Response, String> {
        self.send(wanted, "The SYNTRAC collector is not running, so the change could not be saved.")
    }

    fn send(&self, wanted: &Request, not_running: &str) -> Result<Response, String> {
        match request(&self.pipe_name, wanted, WRITE_TIMEOUT) {
            Ok(Response::Error { message }) => Err(message),
            Ok(response) => Ok(response),
            Err(syntrac_ipc::pipe::ClientError::NotRunning) => Err(not_running.to_owned()),
            Err(syntrac_ipc::pipe::ClientError::AccessDenied) => {
                Err("This user is not allowed to change the collector's settings.".to_owned())
            }
            Err(other) => Err(format!("The collector could not be reached: {other}")),
        }
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
