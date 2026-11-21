//! The background alerts (docs/ALERTS-PLAN.md, slice T1): which toasts a list of open findings calls for, and the watcher that sends them
//! with no window. Pure: the findings, the notifier and the settings file are all fakes here.

use std::sync::{Arc, Mutex};

use syntrac_app_lib::prefs::{Prefs, PrefsHandle};
use syntrac_app_lib::toast_watch::{plan_toasts, FindingSource, Notifier, OpenFinding, Toast, Watcher};

fn finding(id: i64, tier: &str) -> OpenFinding {
    OpenFinding { id, tier: tier.to_owned(), title: format!("Finding {id}"), why: format!("Why {id}.") }
}

fn titles(toasts: &[Toast]) -> Vec<&str> {
    toasts.iter().map(|toast| toast.title.as_str()).collect()
}

// ---- the plan

#[test]
fn the_first_run_ever_shows_nothing_and_remembers_the_newest_id() {
    let plan = plan_toasts(&[finding(1, "suspicious"), finding(2, "unusual")], None);

    assert!(plan.toasts.is_empty(), "what is already open is not news");
    assert_eq!(plan.last_announced, 2);
    assert_eq!(plan_toasts(&[], None).last_announced, 0, "nothing open: the next finding is above zero");
}

#[test]
fn a_new_suspicious_or_confirmed_finding_is_one_toast_with_its_title_and_why() {
    let plan = plan_toasts(&[finding(1, "suspicious"), finding(2, "confirmed")], Some(0));

    assert_eq!(plan.toasts.len(), 2);
    assert_eq!(plan.toasts[0], Toast { title: "SYNTRAC: suspicious finding".into(), body: "Finding 1. Why 1.".into() });
    assert_eq!(plan.toasts[1].title, "SYNTRAC: confirmed finding");
}

#[test]
fn the_unusual_ones_are_one_summary_never_one_each() {
    let three = plan_toasts(&[finding(1, "unusual"), finding(2, "unusual"), finding(3, "unusual")], Some(0));
    assert_eq!(
        three.toasts,
        vec![Toast {
            title: "SYNTRAC: 3 findings to look at".into(),
            body: "Open the Findings tab to see them.".into()
        }]
    );
    assert_eq!(titles(&plan_toasts(&[finding(1, "unusual")], Some(0)).toasts), ["SYNTRAC: 1 finding to look at"]);
}

#[test]
fn only_findings_above_the_last_announced_id_are_news_and_the_id_moves_up() {
    let plan = plan_toasts(&[finding(1, "suspicious"), finding(2, "suspicious")], Some(1));

    assert_eq!(plan.toasts.len(), 1);
    assert!(plan.toasts[0].body.contains("Finding 2"));
    assert_eq!(plan.last_announced, 2);
}

#[test]
fn at_most_three_suspicious_toasts_then_one_line_saying_how_many_more() {
    let many: Vec<_> = (1..=5).map(|id| finding(id, "suspicious")).collect();

    let plan = plan_toasts(&many, Some(0));

    assert_eq!(plan.toasts.len(), 4);
    assert_eq!(
        plan.toasts[3],
        Toast {
            title: "SYNTRAC: 2 more suspicious findings".into(),
            body: "Open the Findings tab to see them.".into()
        }
    );
    let one_more: Vec<_> = (1..=4).map(|id| finding(id, "suspicious")).collect();
    assert_eq!(plan_toasts(&one_more, Some(0)).toasts[3].title, "SYNTRAC: 1 more suspicious finding");
}

#[test]
fn a_long_text_is_cut_to_two_hundred_characters() {
    let long = OpenFinding { why: "x".repeat(500), ..finding(1, "suspicious") };
    let body = &plan_toasts(&[long], Some(0)).toasts[0].body;
    assert_eq!(body.chars().count(), 200);
    assert!(body.ends_with('…'));
}

#[test]
fn the_last_announced_id_never_goes_backwards() {
    assert_eq!(plan_toasts(&[], Some(7)).last_announced, 7, "a closed finding does not lower it");
    assert_eq!(plan_toasts(&[finding(3, "suspicious")], Some(7)).last_announced, 7);
}

// ---- the watcher

#[derive(Clone)]
struct FakeSource {
    open: Arc<Mutex<Result<Vec<OpenFinding>, String>>>,
    running: Arc<Mutex<bool>>,
}

impl FakeSource {
    fn new(open: Vec<OpenFinding>) -> Self {
        Self { open: Arc::new(Mutex::new(Ok(open))), running: Arc::new(Mutex::new(true)) }
    }
    fn set(&self, open: Vec<OpenFinding>) {
        *self.open.lock().unwrap() = Ok(open);
    }
}

impl FindingSource for FakeSource {
    fn open_findings(&self) -> Result<Vec<OpenFinding>, String> {
        self.open.lock().unwrap().clone()
    }
    fn collector_running(&self) -> bool {
        *self.running.lock().unwrap()
    }
}

#[derive(Clone, Default)]
struct FakeNotifier {
    shown: Arc<Mutex<Vec<Toast>>>,
}

impl Notifier for FakeNotifier {
    fn show(&self, toast: &Toast) {
        self.shown.lock().unwrap().push(toast.clone());
    }
}

fn watcher(source: &FakeSource, notifier: &FakeNotifier, prefs: &PrefsHandle) -> Watcher {
    Watcher::new(Box::new(source.clone()), Box::new(notifier.clone()), prefs.clone())
}

#[test]
fn the_first_tick_announces_nothing_a_later_new_finding_is_announced_once() {
    let source = FakeSource::new(vec![finding(1, "suspicious")]);
    let notifier = FakeNotifier::default();
    let prefs = PrefsHandle::in_memory(Prefs::default());
    let mut watcher = watcher(&source, &notifier, &prefs);

    watcher.tick();
    assert!(notifier.shown.lock().unwrap().is_empty());
    assert_eq!(prefs.last_announced(), Some(1));

    source.set(vec![finding(1, "suspicious"), finding(2, "suspicious")]);
    watcher.tick();
    watcher.tick();

    let shown = notifier.shown.lock().unwrap();
    assert_eq!(shown.len(), 1, "the same finding is not announced again on the next look");
    assert!(shown[0].body.contains("Finding 2"));
}

#[test]
fn with_notifications_off_nothing_is_shown_but_what_is_old_is_not_announced_when_they_come_back_on() {
    let source = FakeSource::new(vec![]);
    let notifier = FakeNotifier::default();
    let prefs = PrefsHandle::in_memory(Prefs { notifications: false, last_announced: Some(0) });
    let mut watcher = watcher(&source, &notifier, &prefs);

    source.set(vec![finding(1, "suspicious")]);
    watcher.tick();
    assert!(notifier.shown.lock().unwrap().is_empty());

    prefs.set_notifications_on(true);
    watcher.tick();
    assert!(notifier.shown.lock().unwrap().is_empty(), "finding 1 was seen while the toasts were off");
}

#[test]
fn a_finding_that_cannot_be_read_changes_nothing_and_says_so() {
    let source = FakeSource::new(vec![]);
    *source.open.lock().unwrap() = Err("the database is not there".into());
    let notifier = FakeNotifier::default();
    let prefs = PrefsHandle::in_memory(Prefs { notifications: true, last_announced: Some(4) });
    let mut watcher = watcher(&source, &notifier, &prefs);

    let report = watcher.tick();

    assert_eq!(report.open, None);
    assert!(notifier.shown.lock().unwrap().is_empty());
    assert_eq!(prefs.last_announced(), Some(4));
}

#[test]
fn the_report_carries_the_open_count_and_whether_the_collector_runs_for_the_tray() {
    let source = FakeSource::new(vec![finding(1, "unusual"), finding(2, "suspicious")]);
    *source.running.lock().unwrap() = false;
    let mut watcher = watcher(&source, &FakeNotifier::default(), &PrefsHandle::in_memory(Prefs::default()));

    let report = watcher.tick();

    assert_eq!((report.open, report.collector_running), (Some(2), false));
}

#[test]
fn findings_raised_while_the_program_was_not_running_are_announced_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.json");
    // One run: it has seen finding 1.
    let first = PrefsHandle::with_file(path.clone());
    let source = FakeSource::new(vec![finding(1, "suspicious")]);
    watcher(&source, &FakeNotifier::default(), &first).tick();
    drop(first);

    // The program is closed; finding 2 is raised; the program starts again.
    source.set(vec![finding(1, "suspicious"), finding(2, "suspicious")]);
    let notifier = FakeNotifier::default();
    watcher(&source, &notifier, &PrefsHandle::with_file(path)).tick();

    let shown = notifier.shown.lock().unwrap();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].body.contains("Finding 2"));
}

#[test]
fn a_settings_file_that_cannot_be_written_does_not_make_a_finding_repeat_every_look() {
    let dir = tempfile::tempdir().unwrap();
    // The parent of the settings file is a FILE, so writing it fails every time.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let prefs = PrefsHandle::with_file(blocker.join("app.json"));
    prefs.set_last_announced(0);
    let source = FakeSource::new(vec![finding(1, "suspicious")]);
    let notifier = FakeNotifier::default();
    let mut watcher = watcher(&source, &notifier, &prefs);

    watcher.tick();
    watcher.tick();
    watcher.tick();

    assert_eq!(notifier.shown.lock().unwrap().len(), 1);
}
