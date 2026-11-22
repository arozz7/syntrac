//! Phase 4, slice L4d (docs/PHASE4-PLAN.md): one investigation. The page names a finding or a program; this builds the prompt at the chosen
//! detail level, sends it to the user's model server on this machine, and keeps the streamed answer for the page to read. The answer is plain
//! text and never decides or changes anything. Nothing here runs unless the user presses Investigate, and the server is started only when the
//! user chose that.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use syntrac_core::investigate::{build_prompt, Evidence, PromptConfig, PromptLevel};
use syntrac_core::llm::{ChatMessage, ChatRequest, Flow, LlmError, LlmService, Role, ServerState, StreamEvent};

use crate::ai_control::{AiControl, ServiceFactory};
use crate::investigation_store::{InvestigationStore, JsonInvestigationStore, SavedInvestigation, SavedMark};
use crate::prefs::PrefsHandle;

/// How many investigations are kept for the page to read; older ones are dropped.
const KEPT_JOBS: usize = 8;
/// How long a server that is starting may take before the investigation gives up waiting.
const SERVER_WAIT: Duration = Duration::from_secs(15 * 60);
/// The longest answer asked for. A model that thinks first spends part of this on its thinking, so it is generous; Stop ends it sooner.
const MAX_ANSWER_TOKENS: u32 = 3000;
const TEMPERATURE: f64 = 0.2;

/// What an investigation is about, as the page names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubjectRef {
    Finding { id: i64 },
    Program { path: String },
}

/// Reads the facts for a subject (from the database, in the real program).
pub type EvidenceProvider = Arc<dyn Fn(&SubjectRef) -> Result<Evidence, String> + Send + Sync>;

/// The prompt as it would be sent, for the page to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptPreview {
    /// `summary`, `full` or `external`.
    pub level: String,
    pub system: String,
    pub user: String,
    /// Everything in one text, for pasting into an assistant that has no separate instruction field.
    pub text: String,
}

/// The state of one investigation, for the page to poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvestigationView {
    pub id: u64,
    /// `waiting` (for the server), `running`, `done`, `failed` or `cancelled`.
    pub phase: String,
    /// The answer so far, as plain text.
    pub answer: String,
    /// How much reasoning the model has produced (it is not shown, only counted).
    pub reasoning_chars: usize,
    /// The answer stopped at the length limit and is cut off, not finished.
    pub truncated: bool,
    pub error: Option<String>,
    /// The evidence block that was sent, so the answer can be checked against it.
    pub sent: String,
    pub level: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Waiting,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl Phase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

struct JobState {
    phase: Phase,
    answer: String,
    reasoning_chars: usize,
    /// The server stopped at the length limit: the answer is cut off.
    truncated: bool,
    error: Option<String>,
}

struct Job {
    id: u64,
    sent: String,
    level: &'static str,
    cancel: AtomicBool,
    state: Mutex<JobState>,
}

impl Job {
    fn lock(&self) -> MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn finish(&self, phase: Phase, error: Option<String>) {
        let mut state = self.lock();
        state.phase = phase;
        state.error = error;
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Keeps a finished answer. Runs on the job's thread, before the job reads as done, so a page that sees "done" finds the answer kept.
type Keeper = Box<dyn Fn(&str, bool) + Send>;

/// The time now, in milliseconds since 1970 (replaced in tests).
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

pub struct Investigator {
    prefs: PrefsHandle,
    control: Arc<AiControl>,
    service: ServiceFactory,
    evidence: EvidenceProvider,
    config: PromptConfig,
    store: Arc<dyn InvestigationStore>,
    clock: Clock,
    wait_step: Duration,
    next_id: AtomicU64,
    jobs: Mutex<VecDeque<Arc<Job>>>,
}

impl Investigator {
    pub fn new(
        prefs: PrefsHandle,
        control: Arc<AiControl>,
        service: ServiceFactory,
        evidence: EvidenceProvider,
        config: PromptConfig,
    ) -> Self {
        Self {
            prefs,
            control,
            service,
            evidence,
            config,
            // Answers are kept only in memory until the real program gives a file to keep them in.
            store: Arc::new(JsonInvestigationStore::in_memory()),
            clock: Arc::new(system_now_ms),
            wait_step: Duration::from_secs(2),
            next_id: AtomicU64::new(1),
            jobs: Mutex::new(VecDeque::new()),
        }
    }

    /// How often a job waiting for the server looks again (short in tests).
    #[must_use]
    pub fn with_wait_step(mut self, step: Duration) -> Self {
        self.wait_step = step;
        self
    }

    /// Where finished answers are kept so the same subject need not be asked about again.
    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn InvestigationStore>) -> Self {
        self.store = store;
        self
    }

    /// The clock that dates kept answers (fixed in tests).
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The answer kept for a subject, if an earlier investigation finished.
    pub fn saved(&self, subject: &SubjectRef) -> Option<SavedInvestigation> {
        self.store.get(subject)
    }

    /// Which subjects have a kept answer, and when it was kept (no answers, so a list can be marked cheaply).
    pub fn saved_list(&self) -> Vec<SavedMark> {
        self.store.list()
    }

    /// Throws the kept answer for a subject away.
    pub fn forget(&self, subject: &SubjectRef) {
        self.store.forget(subject);
    }

    /// The prompt for a subject: at the level of the user's setting, or the reduced `external` text that a person may paste elsewhere. It
    /// asks no server and does not need the help to be turned on.
    pub fn preview(&self, subject: &SubjectRef, external: bool) -> Result<PromptPreview, String> {
        let level =
            if external { PromptLevel::External } else { PromptLevel::from(self.prefs.ai_settings().detail_level) };
        let evidence = (self.evidence)(subject)?;
        let prompt = build_prompt(&evidence, level, &self.config);
        Ok(PromptPreview {
            level: level_name(level).to_owned(),
            text: prompt.as_single_text(),
            system: prompt.system,
            user: prompt.user,
        })
    }

    /// Begins an investigation and returns its number at once; the answer arrives while the page polls.
    pub fn start(&self, subject: &SubjectRef) -> Result<u64, String> {
        let settings = self.prefs.ai_settings();
        let endpoint = settings.ready_to_use().map_err(|problem| format!("AI help is not ready: {problem}."))?;
        let level = PromptLevel::from(settings.detail_level);
        let evidence = (self.evidence)(subject)?;
        let prompt = build_prompt(&evidence, level, &self.config);
        let server = (self.service)(endpoint);

        let must_wait = match server.state() {
            ServerState::Ready => false,
            ServerState::Failed(reason) => return Err(format!("The model server answered badly: {reason}.")),
            ServerState::NotRunning | ServerState::Loading if !settings.start_on_investigate => {
                return Err(not_ready_message(&server.state()));
            }
            ServerState::NotRunning | ServerState::Loading => {
                self.control.ensure_started()?;
                true
            }
        };

        let request = ChatRequest {
            model: settings.model.trim().to_owned(),
            messages: vec![
                ChatMessage { role: Role::System, content: prompt.system.clone() },
                ChatMessage { role: Role::User, content: prompt.user.clone() },
            ],
            max_tokens: Some(MAX_ANSWER_TOKENS),
            temperature: TEMPERATURE,
        };
        let job = Arc::new(Job {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            sent: prompt.user,
            level: level_name(level),
            cancel: AtomicBool::new(false),
            state: Mutex::new(JobState {
                phase: if must_wait { Phase::Waiting } else { Phase::Running },
                answer: String::new(),
                reasoning_chars: 0,
                truncated: false,
                error: None,
            }),
        });
        self.keep(&job);
        let keeper = self.keeper(subject, &settings.model, level_name(level), &job.sent);
        let (control, wait_step) = (Arc::clone(&self.control), self.wait_step);
        let worker = Arc::clone(&job);
        std::thread::spawn(move || run(&worker, server.as_ref(), &request, must_wait, &control, wait_step, &keeper));
        Ok(job.id)
    }

    pub fn poll(&self, id: u64) -> Option<InvestigationView> {
        let job = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|job| job.id == id)
            .cloned()?;
        let state = job.lock();
        Some(InvestigationView {
            id,
            phase: state.phase.name().to_owned(),
            answer: state.answer.clone(),
            reasoning_chars: state.reasoning_chars,
            truncated: state.truncated,
            error: state.error.clone(),
            sent: job.sent.clone(),
            level: job.level.to_owned(),
        })
    }

    /// Asks a running investigation to stop. What arrived so far is kept.
    pub fn cancel(&self, id: u64) {
        if let Some(job) =
            self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|job| job.id == id)
        {
            job.cancel.store(true, Ordering::SeqCst);
        }
    }

    fn keeper(&self, subject: &SubjectRef, model: &str, level: &str, sent: &str) -> Keeper {
        let (store, clock) = (Arc::clone(&self.store), Arc::clone(&self.clock));
        let template = SavedInvestigation {
            subject: subject.clone(),
            answer: String::new(),
            truncated: false,
            model: model.trim().to_owned(),
            level: level.to_owned(),
            sent: sent.to_owned(),
            saved_ms: 0,
        };
        Box::new(move |answer, truncated| {
            store.put(SavedInvestigation {
                answer: answer.to_owned(),
                truncated,
                saved_ms: clock(),
                ..template.clone()
            });
        })
    }

    fn keep(&self, job: &Arc<Job>) {
        let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        jobs.push_back(Arc::clone(job));
        while jobs.len() > KEPT_JOBS {
            // A dropped job that is still running is told to stop: nobody can read its answer any more.
            if let Some(old) = jobs.pop_front() {
                old.cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}

pub(crate) fn level_name(level: PromptLevel) -> &'static str {
    match level {
        PromptLevel::Summary => "summary",
        PromptLevel::Full => "full",
        PromptLevel::External => "external",
    }
}

pub(crate) fn not_ready_message(state: &ServerState) -> String {
    match state {
        ServerState::Loading => "The model server is still loading. Try again when it is ready.".to_owned(),
        _ => "The model server is not running. Start it in the AI help settings, or turn on \"start it when I press Investigate\".".to_owned(),
    }
}

fn run(
    job: &Job,
    server: &dyn LlmService,
    request: &ChatRequest,
    must_wait: bool,
    control: &AiControl,
    wait_step: Duration,
    keeper: &Keeper,
) {
    if must_wait && !wait_for_server(job, server, control, wait_step) {
        return;
    }
    job.lock().phase = Phase::Running;
    let result = server.stream_chat(request, &mut |event| {
        if job.cancelled() {
            return Flow::Stop;
        }
        let mut state = job.lock();
        match event {
            StreamEvent::Answer(text) => state.answer.push_str(&text),
            StreamEvent::Reasoning(text) => state.reasoning_chars += text.chars().count(),
            StreamEvent::CutOff => state.truncated = true,
        }
        Flow::Continue
    });
    match result {
        _ if job.cancelled() => job.finish(Phase::Cancelled, None),
        Ok(()) => {
            let (answer, truncated) = {
                let state = job.lock();
                (state.answer.clone(), state.truncated)
            };
            // Kept before the job reads as done: the page asks for the kept answer the moment it sees "done".
            if !answer.trim().is_empty() {
                keeper(&answer, truncated);
            }
            job.finish(Phase::Done, None);
        }
        Err(error) => job.finish(Phase::Failed, Some(describe(&error))),
    }
}

/// Waits until the server answers. Returns whether to go on (false: the job was finished here).
fn wait_for_server(job: &Job, server: &dyn LlmService, control: &AiControl, wait_step: Duration) -> bool {
    match wait_until_ready(server, control, wait_step, &|| job.cancelled()) {
        Ok(()) => true,
        Err(WaitEnd::Cancelled) => {
            job.finish(Phase::Cancelled, None);
            false
        }
        Err(WaitEnd::Failed(message)) => {
            job.finish(Phase::Failed, Some(message));
            false
        }
    }
}

/// Why waiting for a server that is starting ended without it being ready.
pub(crate) enum WaitEnd {
    Cancelled,
    Failed(String),
}

/// Waits until the server answers, giving up when cancelled, when the server reports a fault or ends, or after `SERVER_WAIT`.
pub(crate) fn wait_until_ready(
    server: &dyn LlmService,
    control: &AiControl,
    wait_step: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), WaitEnd> {
    let deadline = Instant::now() + SERVER_WAIT;
    loop {
        if cancelled() {
            return Err(WaitEnd::Cancelled);
        }
        match server.state() {
            ServerState::Ready => return Ok(()),
            ServerState::Failed(reason) => {
                return Err(WaitEnd::Failed(format!("The model server answered badly: {reason}.")));
            }
            ServerState::NotRunning | ServerState::Loading => {}
        }
        if control.status().state == "exited" {
            return Err(WaitEnd::Failed("The model server ended before it was ready.".to_owned()));
        }
        if Instant::now() > deadline {
            return Err(WaitEnd::Failed("The model server did not become ready in time.".to_owned()));
        }
        std::thread::sleep(wait_step);
    }
}

pub(crate) fn describe(error: &LlmError) -> String {
    match error {
        LlmError::NotRunning => "The model server is not running.".to_owned(),
        LlmError::Loading { retry_after_secs: Some(secs) } => {
            format!("The model server is loading a model; try again in about {secs} seconds.")
        }
        LlmError::Loading { retry_after_secs: None } => {
            "The model server is loading a model; try again shortly.".to_owned()
        }
        LlmError::TimedOut => "The model server did not answer in time.".to_owned(),
        other => format!("{other}."),
    }
}
