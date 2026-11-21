//! Alerts with no window (docs/ALERTS-PLAN.md, slice T1). `plan_toasts` is the pure rule (which toasts a list of open findings calls
//! for); the [`Watcher`] looks at the findings now and then and sends them, through traits so it is tested with fakes. The same rules the
//! page used to apply, now in the program itself, so closing the window does not stop them.
//!
//! "New" means an id above the last one announced, kept in the settings file: a finding raised while the program was not running is
//! announced at the next start (under the same caps), and the very first run announces nothing (what is open is not news).

use crate::prefs::PrefsHandle;

/// At most this many individual toasts; the rest is one line saying how many more.
const MAX_INDIVIDUAL: usize = 3;
/// A notification body is cut to this many characters.
const MAX_BODY: usize = 200;
const SEE_THEM: &str = "Open the Findings tab to see them.";

/// What a toast needs to know about an open finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFinding {
    pub id: i64,
    /// `unusual`, `suspicious` or `confirmed`.
    pub tier: String,
    pub title: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastPlan {
    pub toasts: Vec<Toast>,
    /// What the next plan compares with: the highest id announced or seen.
    pub last_announced: i64,
}

fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_BODY {
        text.to_owned()
    } else {
        let mut short: String = text.chars().take(MAX_BODY - 1).collect();
        short.push('…');
        short
    }
}

fn findings(count: usize) -> String {
    if count == 1 {
        "1 finding".to_owned()
    } else {
        format!("{count} findings")
    }
}

/// Which toasts a fresh list of OPEN findings calls for, given the highest id already announced (`None`: the very first run).
/// - the first run shows nothing, it only remembers;
/// - one toast for each new suspicious or confirmed finding, at most three, then one line saying how many more;
/// - the unusual ones are one summary, never one each.
pub fn plan_toasts(open: &[OpenFinding], last_announced: Option<i64>) -> ToastPlan {
    let newest = open.iter().map(|finding| finding.id).max().unwrap_or(0);
    let Some(last) = last_announced else {
        return ToastPlan { toasts: Vec::new(), last_announced: newest };
    };
    let fresh: Vec<&OpenFinding> = open.iter().filter(|finding| finding.id > last).collect();
    let (serious, unusual): (Vec<&OpenFinding>, Vec<&OpenFinding>) =
        fresh.into_iter().partition(|finding| finding.tier != "unusual");

    let mut toasts: Vec<Toast> = serious
        .iter()
        .take(MAX_INDIVIDUAL)
        .map(|finding| Toast {
            title: format!("SYNTRAC: {} finding", finding.tier),
            body: cut(&format!("{}. {}", finding.title, finding.why)),
        })
        .collect();
    if serious.len() > MAX_INDIVIDUAL {
        let more = serious.len() - MAX_INDIVIDUAL;
        let noun = if more == 1 { "finding" } else { "findings" };
        toasts.push(Toast { title: format!("SYNTRAC: {more} more suspicious {noun}"), body: SEE_THEM.to_owned() });
    }
    if !unusual.is_empty() {
        toasts.push(Toast {
            title: format!("SYNTRAC: {} to look at", findings(unusual.len())),
            body: SEE_THEM.to_owned(),
        });
    }
    ToastPlan { toasts, last_announced: last.max(newest) }
}

/// Where the open findings and the collector's state come from.
pub trait FindingSource {
    /// The OPEN findings, or why they cannot be read.
    fn open_findings(&self) -> Result<Vec<OpenFinding>, String>;
    fn collector_running(&self) -> bool;
}

/// Where a toast goes.
pub trait Notifier {
    fn show(&self, toast: &Toast);
}

/// What one look found, for the tray.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// How many findings are open; `None` when they could not be read.
    pub open: Option<u32>,
    pub collector_running: bool,
}

pub struct Watcher {
    source: Box<dyn FindingSource + Send>,
    notifier: Box<dyn Notifier + Send>,
    prefs: PrefsHandle,
}

impl Watcher {
    pub fn new(source: Box<dyn FindingSource + Send>, notifier: Box<dyn Notifier + Send>, prefs: PrefsHandle) -> Self {
        Self { source, notifier, prefs }
    }

    /// One look: sends the toasts that are due (when notifications are on) and moves the last announced id up. What was seen while
    /// notifications were off counts as seen, so switching them on later does not announce old findings. A finding that cannot be read
    /// changes nothing.
    pub fn tick(&mut self) -> TickReport {
        let collector_running = self.source.collector_running();
        let Ok(open) = self.source.open_findings() else { return TickReport { open: None, collector_running } };
        let plan = plan_toasts(&open, self.prefs.last_announced());
        if self.prefs.notifications_on() {
            for toast in &plan.toasts {
                self.notifier.show(toast);
            }
        }
        if self.prefs.last_announced() != Some(plan.last_announced) {
            self.prefs.set_last_announced(plan.last_announced);
        }
        TickReport { open: u32::try_from(open.len()).ok(), collector_running }
    }
}
