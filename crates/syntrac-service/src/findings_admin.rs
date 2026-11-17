//! The pipe's findings commands: what the user decides about a finding, and removing something they said is expected
//! (docs/ALERTS-PLAN.md, F1).
//!
//! Everything a client sends is checked here. An expectation is always built from the finding THIS service stored (its own kind
//! and subject), never from anything the client claims, so a client cannot silence a rule for a subject it names. Each change is
//! logged.

use std::sync::{Mutex, PoisonError};

use syntrac_core::findings::{apply_decision, Decision, ExpectationScope};
use syntrac_core::{Clock, ExpectationRepository, FindingRepository};
use syntrac_ipc::protocol::Response;
use tracing::{info, warn};

/// What the pipe handler needs from the findings.
pub trait FindingsControl: Send + Sync {
    /// Applies the user's decision to a finding. `decision` is one of the names in `Request::FindingDecide`.
    fn decide(&self, id: i64, decision: &str, days: Option<u32>, note: Option<&str>, scope: Option<&str>) -> Response;
    fn remove_expectation(&self, id: i64) -> Response;
}

pub struct FindingsAdmin<R, C> {
    repo: Mutex<R>,
    clock: C,
}

fn refuse(message: impl Into<String>) -> Response {
    Response::Error { message: message.into() }
}

fn parse(decision: &str, days: Option<u32>, note: Option<&str>, scope: Option<&str>) -> Result<Decision, String> {
    let scope = match scope {
        None | Some("this_item") => ExpectationScope::ThisItem,
        Some("this_folder") => ExpectationScope::ThisFolder,
        Some(other) => return Err(format!("Unknown scope: {other}.")),
    };
    match decision {
        "expected_this_time" => Ok(Decision::ExpectedThisTime),
        "always_expected" => Ok(Decision::AlwaysExpected { note: note.map(str::to_owned), scope }),
        "not_expected" => Ok(Decision::NotExpected),
        "reopen" => Ok(Decision::Reopen),
        "snooze" => {
            days.map(|days| Decision::Snooze { days }).ok_or_else(|| "Say how many days to snooze it for.".to_owned())
        }
        other => Err(format!("Unknown decision: {other}.")),
    }
}

impl<R: FindingRepository + ExpectationRepository, C: Clock> FindingsAdmin<R, C> {
    pub fn new(repo: R, clock: C) -> Self {
        Self { repo: Mutex::new(repo), clock }
    }

    /// Runs `work` on the repository (for tests).
    pub fn with_repository<T>(&self, work: impl FnOnce(&R) -> T) -> T {
        work(&self.repo.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl<R, C> FindingsControl for FindingsAdmin<R, C>
where
    R: FindingRepository + ExpectationRepository + Send,
    C: Clock + Send + Sync,
{
    fn decide(&self, id: i64, decision: &str, days: Option<u32>, note: Option<&str>, scope: Option<&str>) -> Response {
        let decision = match parse(decision, days, note, scope) {
            Ok(decision) => decision,
            Err(message) => return refuse(message),
        };
        let now = self.clock.now();
        let mut repo = self.repo.lock().unwrap_or_else(PoisonError::into_inner);
        let finding = match repo.finding(id) {
            Ok(Some(finding)) => finding,
            Ok(None) => return refuse("There is no such finding."),
            Err(error) => return refuse(format!("Could not read that finding: {error}")),
        };
        let effect = match apply_decision(&finding, &decision, now) {
            Ok(effect) => effect,
            Err(error) => return refuse(error.to_string()),
        };
        // The expectation first: if it cannot be stored the finding is not marked, so nothing is half done.
        if let Some(expectation) = &effect.new_expectation {
            if let Err(error) = repo.add_expectation(expectation, now) {
                warn!(%error, "Could not save an expectation");
                return refuse(format!("Could not save it: {error}"));
            }
        }
        match repo.set_status(id, effect.status, now, effect.snoozed_until) {
            Ok(true) => {
                info!(id, status = effect.status.as_str(), "Finding decided");
                Response::Done
            }
            Ok(false) => refuse("There is no such finding."),
            Err(error) => {
                warn!(%error, "Could not save a decision about a finding");
                refuse(format!("Could not save it: {error}"))
            }
        }
    }

    fn remove_expectation(&self, id: i64) -> Response {
        let mut repo = self.repo.lock().unwrap_or_else(PoisonError::into_inner);
        match repo.remove_expectation(id) {
            Ok(true) => {
                info!(id, "Expectation removed");
                Response::Done
            }
            Ok(false) => refuse("There is no such expectation."),
            Err(error) => {
                warn!(%error, "Could not remove an expectation");
                refuse(format!("Could not remove it: {error}"))
            }
        }
    }
}
