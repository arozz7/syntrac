//! The pipe's findings commands: what the user decides about a finding, and removing something they said is expected
//! (docs/ALERTS-PLAN.md, F1).
//!
//! Everything a client sends is checked here. An expectation is always built from the finding THIS service stored (its own kind
//! and subject), never from anything the client claims, so a client cannot silence a rule for a subject it names. Each change is
//! logged.

use std::sync::{Mutex, PoisonError};

use syntrac_core::findings::{apply_decision, Decision, ExpectationScope, NewExpectation, FOLDER_SCOPE_PREFIX};
use syntrac_core::review::review_group_keys;
use syntrac_core::{
    install_root, is_protected_folder_on_system_drive, Clock, DetectionStateRepository, ExpectationRepository,
    FindingRepository, IdentityRepository, KnownProductRepository,
};
use syntrac_ipc::protocol::Response;
use tracing::{info, warn};

/// A path longer than this is not an install folder.
const MAX_ROOT_CHARS: usize = 600;

/// A publisher longer than this is not one the review showed.
const MAX_PUBLISHER_CHARS: usize = 300;

/// The most programs one "mark these programs as expected" may store. A bigger group is refused whole.
pub const MAX_REVIEW_PROGRAMS: usize = 500;

const REVIEW_NOTE: &str = "Looked at in the review of what was here at switch-on";

const WRITABLE_FOLDER: &str =
    "That folder can be written to by ordinary programs, so a program copied into it would never be \
called new. Mark the programs of the group one by one instead.";

/// What the pipe handler needs from the findings.
pub trait FindingsControl: Send + Sync {
    /// Applies the user's decision to a finding. `decision` is one of the names in `Request::FindingDecide`.
    fn decide(&self, id: i64, decision: &str, days: Option<u32>, note: Option<&str>, scope: Option<&str>) -> Response;
    fn remove_expectation(&self, id: i64) -> Response;
    /// Expect new programs below an install folder the user has looked at in the review of what is here today. Only for a
    /// protected folder (Windows or Program Files on the Windows drive).
    fn confirm_review(&self, root: &str) -> Response;
    /// Expect the programs of one group of the review, one by one: exactly the programs the review shows for that folder and
    /// publisher, never the folder itself. For a folder ordinary programs can write to.
    fn confirm_review_programs(&self, root: &str, publisher: Option<&str>) -> Response;
}

pub struct FindingsAdmin<R, C> {
    repo: Mutex<R>,
    clock: C,
    /// The Windows drive: only its Windows and Program Files folders can be expected as a whole.
    system_drive: String,
    review_limit: usize,
}

fn refuse(message: impl Into<String>) -> Response {
    Response::Error { message: message.into() }
}

/// The program keys of one group of the review, read the way the review screen reads them.
fn review_set<R>(repo: &R, root: &str, publisher: Option<&str>) -> Result<Vec<String>, String>
where
    R: ExpectationRepository + IdentityRepository + KnownProductRepository + DetectionStateRepository,
{
    let read = |error: syntrac_core::RepoError| format!("Could not read the review: {error}");
    let entries = repo.identity_entries().map_err(read)?;
    let known = repo.known_products().map_err(read)?;
    let expectations = repo.expectations().map_err(read)?;
    let switched_on = repo.detection_state().map_err(read)?.map(|state| state.switched_on_at);
    Ok(review_group_keys(&entries, &known, &expectations, switched_on, root, publisher))
}

fn parse(decision: &str, days: Option<u32>, note: Option<&str>, scope: Option<&str>) -> Result<Decision, String> {
    let scope = match scope {
        None | Some("this_item") => ExpectationScope::ThisItem,
        Some("this_folder") => ExpectationScope::ThisFolder,
        Some("build_folder") => ExpectationScope::BuildFolder,
        Some(other) => return Err(format!("Unknown scope: {other}.")),
    };
    match decision {
        "expected_this_time" => Ok(Decision::ExpectedThisTime),
        "always_expected" => Ok(Decision::AlwaysExpected { note: note.map(str::to_owned), scope }),
        "not_expected" => Ok(Decision::NotExpected),
        "reopen" => Ok(Decision::Reopen),
        "raise_limit" => Ok(Decision::RaiseLimit),
        "snooze" => {
            days.map(|days| Decision::Snooze { days }).ok_or_else(|| "Say how many days to snooze it for.".to_owned())
        }
        other => Err(format!("Unknown decision: {other}.")),
    }
}

impl<R: FindingRepository + ExpectationRepository + IdentityRepository, C: Clock> FindingsAdmin<R, C> {
    /// The Windows drive defaults to `c:`; the service sets the real one with `with_system_drive`.
    pub fn new(repo: R, clock: C) -> Self {
        Self { repo: Mutex::new(repo), clock, system_drive: "c:".to_owned(), review_limit: MAX_REVIEW_PROGRAMS }
    }

    /// The Windows drive of this machine (`folder_protection::system_drive`), decided by the service, never by a client.
    #[must_use]
    pub fn with_system_drive(mut self, drive: &str) -> Self {
        drive.clone_into(&mut self.system_drive);
        self
    }

    /// A lower limit for "mark these programs as expected" (for tests).
    #[must_use]
    pub fn with_review_limit(mut self, limit: usize) -> Self {
        self.review_limit = limit;
        self
    }

    /// Runs `work` on the repository (for tests).
    pub fn with_repository<T>(&self, work: impl FnOnce(&R) -> T) -> T {
        work(&self.repo.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl<R, C> FindingsControl for FindingsAdmin<R, C>
where
    R: FindingRepository
        + ExpectationRepository
        + IdentityRepository
        + KnownProductRepository
        + DetectionStateRepository
        + Send,
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
        let effect = match apply_decision(&finding, &decision, now, &self.system_drive) {
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

    fn confirm_review(&self, root: &str) -> Response {
        let root = root.trim();
        if root.is_empty() || root.chars().count() > MAX_ROOT_CHARS || root.ends_with('\\') || !root.contains('\\') {
            return refuse("That folder is too general to be expected as a whole.");
        }
        if !is_protected_folder_on_system_drive(root, &self.system_drive) {
            return refuse(WRITABLE_FOLDER);
        }
        let mut repo = self.repo.lock().unwrap_or_else(PoisonError::into_inner);
        let known = match repo.identity_entries() {
            Ok(entries) => entries
                .iter()
                .any(|entry| install_root(&entry.exe.path).is_some_and(|found| found.eq_ignore_ascii_case(root))),
            Err(error) => return refuse(format!("Could not read the programs: {error}")),
        };
        if !known {
            return refuse("No program known to SYNTRAC is installed in that folder.");
        }
        let expectation = NewExpectation {
            kind: "new_program".into(),
            subject: format!("{FOLDER_SCOPE_PREFIX}{}", root.to_lowercase()),
            note: Some(REVIEW_NOTE.into()),
            source_finding: None,
            ceiling: None,
        };
        match repo.add_expectation(&expectation, self.clock.now()) {
            Ok(id) => {
                info!(id, root, "A group of the review was marked as expected");
                Response::Done
            }
            Err(error) => {
                warn!(%error, "Could not save an expectation");
                refuse(format!("Could not save it: {error}"))
            }
        }
    }

    fn confirm_review_programs(&self, root: &str, publisher: Option<&str>) -> Response {
        let root = root.trim().to_lowercase();
        if root.is_empty() || root.chars().count() > MAX_ROOT_CHARS || root.ends_with('\\') || !root.contains('\\') {
            return refuse("That folder is too general to be expected.");
        }
        let publisher = publisher.map(|text| text.trim().to_lowercase()).filter(|text| !text.is_empty());
        if publisher.as_deref().is_some_and(|text| text.chars().count() > MAX_PUBLISHER_CHARS) {
            return refuse("That publisher is not one the review shows.");
        }
        let mut repo = self.repo.lock().unwrap_or_else(PoisonError::into_inner);
        // The same grouping the review screen used: seen before the switch-on, no known product, nothing expected yet.
        let keys = match review_set(&*repo, &root, publisher.as_deref()) {
            Ok(keys) => keys,
            Err(message) => return refuse(message),
        };
        if keys.is_empty() {
            return refuse("No program of the review is left in that folder.");
        }
        if keys.len() > self.review_limit {
            return refuse(format!(
                "There are too many programs in that group ({}) to mark one by one; the limit is {}.",
                keys.len(),
                self.review_limit
            ));
        }
        let now = self.clock.now();
        for key in &keys {
            let expectation = NewExpectation {
                kind: "new_program".into(),
                subject: key.clone(),
                note: Some(REVIEW_NOTE.into()),
                source_finding: None,
                ceiling: None,
            };
            if let Err(error) = repo.add_expectation(&expectation, now) {
                warn!(%error, "Could not save an expectation");
                return refuse(format!("Could not save all of them ({error}); say it again to finish."));
            }
        }
        info!(
            root = root.as_str(),
            programs = keys.len(),
            "The programs of a group of the review were marked as expected"
        );
        Response::Done
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
