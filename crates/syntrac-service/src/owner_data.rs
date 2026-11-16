//! The owner dataset as the service runs it: what it reports over the pipe, and the background download the user asks for
//! (docs/OWNER-PLAN.md). Nothing here runs by itself: a download starts only when a client sends `owner_data_refresh`, and the
//! address it downloads from is compiled into the adapter, not taken from the request.
//!
//! A refresh runs on its own thread with its own database connection, so the collector is never held up and the pipe answers at
//! once. Every failure is kept as one plain sentence for the screen, and leaves the stored dataset as it was.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use syntrac_core::{refresh_owner_data, Clock, DatasetFetcher, OwnerRepository, RefreshLimits, RepoError};
use syntrac_ipc::protocol::{OwnerDataReport, OwnerDatasetReport, Response};
use tracing::{info, warn};

/// What the pipe handler needs from the owner dataset.
pub trait OwnerControl: Send + Sync {
    /// What is stored and whether a download is under way. Makes no network request.
    fn report(&self) -> OwnerDataReport;
    /// Starts a download in the background and answers with the report at once.
    fn refresh(&self) -> Response;
}

/// Opens the database for one job.
type Opener<R> = Box<dyn Fn() -> Result<R, RepoError> + Send + Sync>;

#[derive(Default)]
struct State {
    dataset: Option<OwnerDatasetReport>,
    refreshing: bool,
    last_error: Option<String>,
}

struct Inner<F, R, C> {
    fetcher: F,
    open: Opener<R>,
    clock: C,
    allowed: bool,
    limits: RefreshLimits,
    state: Mutex<State>,
}

pub struct OwnerData<F, R, C> {
    inner: Arc<Inner<F, R, C>>,
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<F, R, C> OwnerData<F, R, C>
where
    F: DatasetFetcher + Send + Sync + 'static,
    R: OwnerRepository + Send + 'static,
    C: Clock + Send + Sync + 'static,
{
    /// `open` gives each job its own connection. The dataset that is already stored is read once now, so the report is right
    /// straight after a restart.
    pub fn new(fetcher: F, open: Opener<R>, clock: C, allowed: bool, limits: RefreshLimits) -> Self {
        let dataset = match open().and_then(|repo| repo.dataset_info()) {
            Ok(info) => info.map(|info| OwnerDatasetReport {
                source: info.source,
                downloaded_at_micros: info.downloaded_at.as_micros(),
                rows: info.rows,
            }),
            Err(error) => {
                warn!(%error, "Could not read the stored owner dataset");
                None
            }
        };
        let state = State { dataset, ..State::default() };
        Self { inner: Arc::new(Inner { fetcher, open, clock, allowed, limits, state: Mutex::new(state) }) }
    }
}

impl<F, R, C> Inner<F, R, C>
where
    F: DatasetFetcher,
    R: OwnerRepository,
    C: Clock,
{
    fn report(&self) -> OwnerDataReport {
        let state = lock(&self.state);
        OwnerDataReport {
            dataset: state.dataset.clone(),
            refreshing: state.refreshing,
            last_error: state.last_error.clone(),
            allowed: self.allowed,
        }
    }

    /// One download, start to finish. Never panics the caller: the outcome lands in the state.
    fn run(&self) {
        let outcome = (self.open)().map_err(|error| error.to_string()).and_then(|mut repo| {
            let done = refresh_owner_data(&self.fetcher, &mut repo, self.clock.now(), &self.limits)
                .map_err(|error| error.to_string())?;
            let info = repo.dataset_info().map_err(|error| error.to_string())?;
            Ok((done, info))
        });
        let mut state = lock(&self.state);
        match outcome {
            Ok((done, info)) => {
                info!(
                    rows = done.rows,
                    bad_lines = done.bad_lines,
                    unrouted = done.unrouted,
                    "Owner dataset downloaded"
                );
                state.dataset = info.map(|info| OwnerDatasetReport {
                    source: info.source,
                    downloaded_at_micros: info.downloaded_at.as_micros(),
                    rows: info.rows,
                });
                state.last_error = None;
            }
            Err(message) => {
                warn!(%message, "The owner dataset was not updated");
                state.last_error = Some(message);
            }
        }
    }
}

/// Clears the "refreshing" flag however the job ends, even by a panic.
struct Refreshing<'a>(&'a Mutex<State>);

impl Drop for Refreshing<'_> {
    fn drop(&mut self) {
        lock(self.0).refreshing = false;
    }
}

impl<F, R, C> OwnerControl for OwnerData<F, R, C>
where
    F: DatasetFetcher + Send + Sync + 'static,
    R: OwnerRepository + Send + 'static,
    C: Clock + Send + Sync + 'static,
{
    fn report(&self) -> OwnerDataReport {
        self.inner.report()
    }

    fn refresh(&self) -> Response {
        if !self.inner.allowed {
            return Response::Error {
                message: "Downloading owner data is switched off in the service's configuration (owner_data_allowed = false)."
                    .into(),
            };
        }
        {
            let mut state = lock(&self.inner.state);
            if state.refreshing {
                drop(state);
                return Response::OwnerData(self.inner.report());
            }
            state.refreshing = true;
            state.last_error = None;
        }
        let inner = Arc::clone(&self.inner);
        let spawned = std::thread::Builder::new().name("syntrac-owner-data".into()).spawn(move || {
            let _clear = Refreshing(&inner.state);
            inner.run();
        });
        if let Err(error) = spawned {
            lock(&self.inner.state).refreshing = false;
            warn!(%error, "The owner dataset download could not start");
            return Response::Error { message: format!("Could not start the download: {error}") };
        }
        Response::OwnerData(self.inner.report())
    }
}
