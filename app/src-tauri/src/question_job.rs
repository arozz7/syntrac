//! Phase 5, slice B3 (docs/PHASE5-PLAN.md): one question in the question box. The page asks; this builds the read-only tools at the chosen
//! detail level, runs the bounded reasoning loop against the user's model server on this machine, and keeps the progress for the page to
//! read: the answer, and every tool call that was made. Nothing here runs unless the person asks, and no tool can change anything.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use syntrac_core::clean_text::clean_text;
use syntrac_core::investigate::{redact_user_name, PromptLevel};
use syntrac_core::llm::{LlmError, LlmService, ServerState};
use syntrac_core::question_config::QuestionConfig;
use syntrac_core::question_loop::{
    run_question_loop, CallRecord, CallStatus, LoopLimits, LoopObserver, LoopOutcome, LoopSetup, StopReason,
    ToolRegistry,
};

use crate::ai_control::{AiControl, ServiceFactory};
use crate::ai_investigate::{describe, level_name, not_ready_message, wait_until_ready, Phase, WaitEnd};
use crate::prefs::PrefsHandle;
use crate::question_tools::{question_registry, QuestionData};

/// How many questions are kept for the page to read; older ones are dropped.
const KEPT_JOBS: usize = 8;
/// The longest question taken, in characters.
const MAX_QUESTION_CHARS: usize = 500;
/// The longest one turn may answer. A model that thinks first spends a good part of this on thinking (a trace of 1,500 tokens is ordinary), and
/// a turn that runs out of room inside its thinking says nothing at all.
const MAX_TURN_TOKENS: u32 = 6000;
const TEMPERATURE: f64 = 0.2;

/// What the loop may use. A small local model has a small window, so a result and the whole conversation are kept short.
fn default_limits() -> LoopLimits {
    LoopLimits {
        max_calls: 6,
        time_budget: Duration::from_secs(15 * 60),
        result_bytes: 3000,
        conversation_bytes: 16_000,
        arguments_bytes: 1000,
    }
}

const TOOLS_UNSUPPORTED_MESSAGE: &str = "The model server did not accept tool calls, which the question box needs. \
    Use a server and a model that support tool calling (for llama-server, start it with the --jinja option and use a model whose chat \
    template supports tools), or use Investigate, which needs no tools.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    pub name: String,
    pub arguments: String,
    /// `done`, `refused` (not run) or `failed`.
    pub status: String,
    pub note: Option<String>,
    pub result_bytes: usize,
    pub cut: bool,
}

/// The state of one question, for the page to poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub id: u64,
    /// `waiting` (for the server), `running`, `done`, `failed` or `cancelled`.
    pub phase: String,
    pub question: String,
    pub answer: String,
    /// How much reasoning the model produced (it is not shown, only counted).
    pub reasoning_chars: usize,
    /// The answer stopped at the length limit and is cut off, not finished.
    pub truncated: bool,
    pub error: Option<String>,
    /// The server cannot do tool calls; `error` explains what to do.
    pub tools_unsupported: bool,
    /// Every tool call so far, oldest first, so the answer can be checked against what was looked up.
    pub calls: Vec<CallView>,
    /// Why the loop ended: `answered`, `call_limit`, `time_budget`, `context_full` or `cancelled`.
    pub stopped: Option<String>,
    pub level: String,
}

struct JobState {
    phase: Phase,
    answer: String,
    reasoning_chars: usize,
    truncated: bool,
    error: Option<String>,
    tools_unsupported: bool,
    calls: Vec<CallView>,
    stopped: Option<&'static str>,
}

struct Job {
    id: u64,
    question: String,
    level: &'static str,
    cancel: AtomicBool,
    state: Mutex<JobState>,
}

impl Job {
    fn lock(&self) -> MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Ends the job in `phase`, unless Stop already did: a cancelled job stays cancelled whatever the worker finds out later.
    fn finish(&self, phase: Phase, error: Option<String>) {
        let mut state = self.lock();
        if state.phase != Phase::Cancelled {
            state.phase = phase;
            state.error = error;
        }
    }
}

impl LoopObserver for Job {
    fn cancelled(&self) -> bool {
        Job::cancelled(self)
    }

    fn call_made(&self, record: &CallRecord) {
        self.lock().calls.push(call_view(record));
    }
}

fn call_view(record: &CallRecord) -> CallView {
    CallView {
        name: record.name.clone(),
        arguments: record.arguments.clone(),
        status: match record.status {
            CallStatus::Done => "done",
            CallStatus::Refused => "refused",
            CallStatus::Failed => "failed",
        }
        .to_owned(),
        note: record.note.clone(),
        result_bytes: record.result_bytes,
        cut: record.cut,
    }
}

fn stop_name(reason: StopReason) -> &'static str {
    match reason {
        StopReason::Answered => "answered",
        StopReason::CallLimit => "call_limit",
        StopReason::TimeBudget => "time_budget",
        StopReason::ContextFull => "context_full",
        StopReason::Cancelled => "cancelled",
    }
}

pub struct Questioner {
    prefs: PrefsHandle,
    control: Arc<AiControl>,
    service: ServiceFactory,
    data: Arc<dyn QuestionData>,
    config: QuestionConfig,
    user_name: Option<String>,
    limits: LoopLimits,
    wait_step: Duration,
    next_id: AtomicU64,
    jobs: Mutex<VecDeque<Arc<Job>>>,
}

impl Questioner {
    pub fn new(
        prefs: PrefsHandle,
        control: Arc<AiControl>,
        service: ServiceFactory,
        data: Arc<dyn QuestionData>,
        config: QuestionConfig,
    ) -> Self {
        Self {
            prefs,
            control,
            service,
            data,
            config,
            user_name: None,
            limits: default_limits(),
            wait_step: Duration::from_secs(2),
            next_id: AtomicU64::new(1),
            jobs: Mutex::new(VecDeque::new()),
        }
    }

    /// The Windows user name, replaced by `<user>` in the question, the results and the page (only used to be left out).
    #[must_use]
    pub fn with_user_name(mut self, user_name: Option<String>) -> Self {
        self.user_name = user_name.filter(|name| !name.trim().is_empty());
        self
    }

    /// How often a question waiting for the server looks again (short in tests).
    #[must_use]
    pub fn with_wait_step(mut self, step: Duration) -> Self {
        self.wait_step = step;
        self
    }

    #[must_use]
    pub fn with_limits(mut self, limits: LoopLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Begins a question and returns its number at once; the answer arrives while the page polls.
    pub fn start(&self, question: &str) -> Result<u64, String> {
        let user = self.user_name.as_deref().unwrap_or("");
        let question = clean_text(&redact_user_name(question, user), MAX_QUESTION_CHARS)
            .ok_or_else(|| "Type a question first.".to_owned())?;
        let settings = self.prefs.ai_settings();
        let endpoint = settings.ready_to_use().map_err(|problem| format!("AI help is not ready: {problem}."))?;
        let level = PromptLevel::from(settings.detail_level);
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

        let registry = question_registry(Arc::clone(&self.data), &self.config, level, self.user_name.clone());
        let setup = LoopSetup {
            model: settings.model.trim().to_owned(),
            system: self.config.system.trim().to_owned(),
            final_request: self.config.final_request.trim().to_owned(),
            bad_call_nudge: self.config.bad_call_nudge.trim().to_owned(),
            user_name: self.user_name.clone(),
            limits: self.limits,
            max_tokens: Some(MAX_TURN_TOKENS),
            temperature: TEMPERATURE,
        };
        let job = Arc::new(Job {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            question: question.clone(),
            level: level_name(level),
            cancel: AtomicBool::new(false),
            state: Mutex::new(JobState {
                phase: if must_wait { Phase::Waiting } else { Phase::Running },
                answer: String::new(),
                reasoning_chars: 0,
                truncated: false,
                error: None,
                tools_unsupported: false,
                calls: Vec::new(),
                stopped: None,
            }),
        });
        self.keep(&job);
        let (control, wait_step, worker) = (Arc::clone(&self.control), self.wait_step, Arc::clone(&job));
        std::thread::spawn(move || {
            run(&worker, server.as_ref(), &registry, &setup, &question, must_wait.then_some((&control, wait_step)));
        });
        Ok(job.id)
    }

    pub fn poll(&self, id: u64) -> Option<QuestionView> {
        let job = self.find(id)?;
        let state = job.lock();
        Some(QuestionView {
            id,
            phase: state.phase.name().to_owned(),
            question: job.question.clone(),
            answer: state.answer.clone(),
            reasoning_chars: state.reasoning_chars,
            truncated: state.truncated,
            error: state.error.clone(),
            tools_unsupported: state.tools_unsupported,
            calls: state.calls.clone(),
            stopped: state.stopped.map(str::to_owned),
            level: job.level.to_owned(),
        })
    }

    /// Asks a running question to stop. What was looked up and answered so far is kept.
    pub fn cancel(&self, id: u64) {
        if let Some(job) = self.find(id) {
            job.cancel.store(true, Ordering::SeqCst);
            // A turn of a slow model can take minutes and cannot be interrupted, so the page is told now; the worker's late result is dropped.
            let mut state = job.lock();
            if matches!(state.phase, Phase::Waiting | Phase::Running) {
                state.phase = Phase::Cancelled;
                state.stopped = Some(stop_name(StopReason::Cancelled));
            }
        }
    }

    fn find(&self, id: u64) -> Option<Arc<Job>> {
        self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|job| job.id == id).cloned()
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

fn run(
    job: &Job,
    server: &dyn LlmService,
    registry: &ToolRegistry,
    setup: &LoopSetup,
    question: &str,
    wait: Option<(&Arc<AiControl>, Duration)>,
) {
    if let Some((control, wait_step)) = wait {
        match wait_until_ready(server, control, wait_step, &|| job.cancelled()) {
            Ok(()) => {}
            Err(WaitEnd::Cancelled) => return job.finish(Phase::Cancelled, None),
            Err(WaitEnd::Failed(message)) => return job.finish(Phase::Failed, Some(message)),
        }
    }
    {
        let mut state = job.lock();
        if state.phase == Phase::Waiting {
            state.phase = Phase::Running;
        }
    }
    match run_question_loop(server, registry, setup, question, job) {
        Ok(outcome) => record_outcome(job, outcome),
        Err(_) if job.cancelled() => job.finish(Phase::Cancelled, None),
        Err(LlmError::ToolsUnsupported) => {
            job.lock().tools_unsupported = true;
            job.finish(Phase::Failed, Some(TOOLS_UNSUPPORTED_MESSAGE.to_owned()));
        }
        Err(error) => job.finish(Phase::Failed, Some(describe(&error))),
    }
}

fn record_outcome(job: &Job, outcome: LoopOutcome) {
    let phase = if outcome.stopped == StopReason::Cancelled { Phase::Cancelled } else { Phase::Done };
    let mut state = job.lock();
    if state.phase == Phase::Cancelled {
        // Stop was pressed while the model was still working: what arrives now is not shown.
        return;
    }
    state.answer = outcome.answer;
    state.truncated = outcome.cut_off;
    state.reasoning_chars = outcome.reasoning_chars;
    state.stopped = Some(stop_name(outcome.stopped));
    state.phase = phase;
}
