//! A trace session outlives the process that started it. A leftover SYNTRAC session can starve every other session on
//! the machine (observed: an orphaned console session made the installed service receive no events at all), so
//! SYNTRAC cleans up its own leftovers - and only its own, and only when it is certain they are leftovers.

use syntrac_collector::orphan_guard::{plan_orphan_cleanup, OrphanAction};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_machine_with_no_other_syntrac_sessions_needs_nothing() {
    let sessions = names(&["NetCore", "EventLog-System", "SyntracCollector"]);
    assert_eq!(plan_orphan_cleanup("SyntracCollector", &sessions, false), vec![]);
}

#[test]
fn our_own_session_is_never_touched_whatever_its_letter_case() {
    let sessions = names(&["syntraccollector"]);
    assert_eq!(plan_orphan_cleanup("SyntracCollector", &sessions, false), vec![]);
}

#[test]
fn a_leftover_console_session_is_stopped_when_no_other_collector_is_running() {
    // The case that actually happened.
    let sessions = names(&["NetCore", "SyntracCollector", "SyntracConsole"]);
    assert_eq!(
        plan_orphan_cleanup("SyntracCollector", &sessions, false),
        vec![OrphanAction::Stop("SyntracConsole".into())]
    );
}

#[test]
fn a_leftover_service_session_is_stopped_by_a_console_run_when_the_service_is_not_running() {
    let sessions = names(&["SyntracCollector"]);
    assert_eq!(
        plan_orphan_cleanup("SyntracConsole", &sessions, false),
        vec![OrphanAction::Stop("SyntracCollector".into())]
    );
}

#[test]
fn a_session_belonging_to_a_collector_that_is_running_is_left_alone() {
    // The service and a console run may legitimately run side by side.
    let sessions = names(&["SyntracCollector", "SyntracConsole"]);
    assert_eq!(plan_orphan_cleanup("SyntracCollector", &sessions, true), vec![]);
    assert_eq!(plan_orphan_cleanup("SyntracConsole", &sessions, true), vec![]);
}

#[test]
fn an_unfamiliar_syntrac_session_is_reported_but_never_stopped() {
    // e.g. the diagnostic probe, or a session from a future version: not ours to kill on a guess.
    let sessions = names(&["SyntracProbe", "SyntracCollector"]);
    assert_eq!(
        plan_orphan_cleanup("SyntracCollector", &sessions, false),
        vec![OrphanAction::Warn("SyntracProbe".into())]
    );
    assert_eq!(
        plan_orphan_cleanup("SyntracCollector", &sessions, true),
        vec![OrphanAction::Warn("SyntracProbe".into())],
        "an unfamiliar session is worth a warning even while a collector runs"
    );
}

#[test]
fn sessions_of_other_programs_are_never_touched_even_with_similar_names() {
    let sessions = names(&["TraceEventSession", "Syntrace-Other-Product", "MpWppTracing", "NetCore"]);
    let plan = plan_orphan_cleanup("SyntracCollector", &sessions, false);
    // "Syntrace-Other-Product" starts with the prefix, so it is only warned about, never stopped.
    assert_eq!(plan, vec![OrphanAction::Warn("Syntrace-Other-Product".into())]);
}

#[test]
fn several_leftovers_are_planned_in_the_order_they_were_listed() {
    let sessions = names(&["SyntracConsole", "SyntracProbe", "SyntracCollector"]);
    assert_eq!(
        plan_orphan_cleanup("SyntracCollector", &sessions, false),
        vec![OrphanAction::Stop("SyntracConsole".into()), OrphanAction::Warn("SyntracProbe".into())]
    );
}

// ---- carrying the plan out, against a fake machine

use std::cell::RefCell;
use syntrac_collector::orphan_guard::{clean_up_orphans, SessionHost};

struct FakeHost {
    sessions: Result<Vec<String>, String>,
    other_collector_running: bool,
    failing_stops: Vec<String>,
    stopped: RefCell<Vec<String>>,
}

impl FakeHost {
    fn new(sessions: &[&str], other_collector_running: bool) -> Self {
        Self {
            sessions: Ok(names(sessions)),
            other_collector_running,
            failing_stops: Vec::new(),
            stopped: RefCell::new(Vec::new()),
        }
    }
}

impl SessionHost for FakeHost {
    fn list_sessions(&self) -> Result<Vec<String>, String> {
        self.sessions.clone()
    }
    fn stop_session(&self, name: &str) -> Result<(), String> {
        self.stopped.borrow_mut().push(name.to_owned());
        if self.failing_stops.iter().any(|f| f == name) {
            Err("access denied".into())
        } else {
            Ok(())
        }
    }
    fn other_collector_running(&self) -> bool {
        self.other_collector_running
    }
}

#[test]
fn the_cleanup_stops_exactly_the_planned_sessions_and_reports_them() {
    let host = FakeHost::new(&["NetCore", "SyntracCollector", "SyntracConsole", "SyntracProbe"], false);
    let report = clean_up_orphans("SyntracCollector", &host);
    assert_eq!(*host.stopped.borrow(), vec!["SyntracConsole".to_owned()]);
    assert_eq!(report.stopped, vec!["SyntracConsole".to_owned()]);
    assert_eq!(report.warned, vec!["SyntracProbe".to_owned()]);
    assert!(report.failed.is_empty());
}

#[test]
fn a_session_that_cannot_be_stopped_is_reported_and_the_rest_are_still_handled() {
    let mut host = FakeHost::new(&["SyntracConsole", "SyntracCollector"], false);
    host.failing_stops.push("SyntracConsole".into());
    // Neither session is the caller's own, and both are stoppable leftovers.
    let report = clean_up_orphans("SomeOtherCollector", &host);
    assert_eq!(*host.stopped.borrow(), vec!["SyntracConsole".to_owned(), "SyntracCollector".to_owned()]);
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].0 == "SyntracConsole" && report.failed[0].1.contains("access denied"));
    assert_eq!(report.stopped, vec!["SyntracCollector".to_owned()]);
}

#[test]
fn if_the_sessions_cannot_even_be_listed_nothing_is_stopped_and_starting_up_is_not_blocked() {
    let mut host = FakeHost::new(&[], false);
    host.sessions = Err("QueryAllTraces failed".into());
    let report = clean_up_orphans("SyntracCollector", &host);
    assert!(host.stopped.borrow().is_empty());
    assert!(report.stopped.is_empty() && report.failed.is_empty());
    assert!(report.list_error.is_some_and(|e| e.contains("QueryAllTraces")));
}

#[test]
fn nothing_is_stopped_while_another_collector_is_running() {
    let host = FakeHost::new(&["SyntracCollector", "SyntracConsole"], true);
    let report = clean_up_orphans("SyntracCollector", &host);
    assert!(host.stopped.borrow().is_empty());
    assert!(report.stopped.is_empty());
}
