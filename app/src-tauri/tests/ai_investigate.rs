//! Running one investigation (docs/PHASE4-PLAN.md, L4d): the prompt that would be sent, the streamed answer, cancel, the failures, and starting
//! the server when the user chose that. A scripted model and a fake process stand in for the real ones.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_app_lib::ai_control::{AiControl, AiSettingsDto, ServiceFactory};
use syntrac_app_lib::ai_investigate::{EvidenceProvider, Investigator, SubjectRef};
use syntrac_app_lib::investigation_store::{InvestigationStore, JsonInvestigationStore};
use syntrac_app_lib::prefs::{Prefs, PrefsHandle};
use syntrac_core::investigate::{Evidence, FindingFacts, ProgramFacts, PromptConfig};
use syntrac_core::llm::{ChatRequest, Flow, LlmError, LlmService, ServerState, StreamEvent};
use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::{ChildHandle, LaunchError, ProcessSpawner};
use syntrac_testkit::mock_llm::MockLlm;
use syntrac_testkit::mock_process::FakeSpawner;

/// A model that gives one piece, then keeps "thinking" until it is told to stop (or a few seconds pass).
struct SlowLlm {
    stopped: Arc<AtomicBool>,
}

impl LlmService for SlowLlm {
    fn state(&self) -> ServerState {
        ServerState::Ready
    }

    fn stream_chat(
        &self,
        _request: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent) -> Flow,
    ) -> Result<(), LlmError> {
        let begin = Instant::now();
        if on_event(StreamEvent::Answer("first words".into())) == Flow::Stop {
            self.stopped.store(true, Ordering::SeqCst);
            return Ok(());
        }
        while begin.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
            if on_event(StreamEvent::Reasoning("…".into())) == Flow::Stop {
                self.stopped.store(true, Ordering::SeqCst);
                return Ok(());
            }
        }
        Ok(())
    }
}

struct SharedLlm(Arc<MockLlm>);

impl LlmService for SharedLlm {
    fn state(&self) -> ServerState {
        self.0.state()
    }

    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_event: &mut dyn FnMut(StreamEvent) -> Flow,
    ) -> Result<(), LlmError> {
        self.0.stream_chat(request, on_event)
    }
}

struct SharedSpawner(Arc<FakeSpawner>);

impl ProcessSpawner for SharedSpawner {
    fn spawn(&self, command: &LaunchCommand) -> Result<Box<dyn ChildHandle>, LaunchError> {
        self.0.spawn(command)
    }
}

struct Rig {
    investigator: Investigator,
    control: Arc<AiControl>,
    server: Arc<MockLlm>,
    spawner: Arc<FakeSpawner>,
    store: Arc<JsonInvestigationStore>,
}

fn evidence() -> Evidence {
    Evidence {
        finding: Some(FindingFacts {
            kind: "usage".into(),
            tier: "unusual".into(),
            title: "widgetsync.exe is busy".into(),
            ..FindingFacts::default()
        }),
        program: Some(ProgramFacts {
            file_name: "widgetsync.exe".into(),
            folder: r"C:\Users\alice\AppData\Local\Fabrikam".into(),
            location: "user_profile".into(),
            signature: Some("valid".into()),
            publisher: Some("Fabrikam Inc".into()),
            ..ProgramFacts::default()
        }),
        user_name: Some("alice".into()),
    }
}

fn settings() -> AiSettingsDto {
    AiSettingsDto {
        enabled: true,
        endpoint: "http://127.0.0.1:1234/v1".into(),
        model: "some-model".into(),
        detail_level: "summary".into(),
        launch_program: r"C:\Tools\srv.exe".into(),
        launch_args: vec![],
        launch_working_dir: String::new(),
        start_on_investigate: false,
        stop_on_quit: false,
    }
}

fn rig_with(llm: impl Fn(Arc<MockLlm>) -> Box<dyn LlmService> + Send + Sync + 'static, server: Arc<MockLlm>) -> Rig {
    let prefs = PrefsHandle::in_memory(Prefs::default());
    let spawner = Arc::new(FakeSpawner::default());
    let shared = Arc::clone(&server);
    let service: ServiceFactory = Arc::new(move |_| llm(Arc::clone(&shared)));
    let control =
        Arc::new(AiControl::new(prefs.clone(), Box::new(SharedSpawner(Arc::clone(&spawner))), Arc::clone(&service)));
    control.save(settings()).unwrap();
    let provider: EvidenceProvider = Arc::new(|subject| match subject {
        SubjectRef::Finding { id: 0 } => Err("The finding no longer exists.".to_owned()),
        _ => Ok(evidence()),
    });
    let store = Arc::new(JsonInvestigationStore::in_memory());
    let investigator = Investigator::new(prefs, Arc::clone(&control), service, provider, PromptConfig::builtin())
        .with_wait_step(Duration::from_millis(10))
        .with_store(Arc::clone(&store) as Arc<dyn InvestigationStore>)
        .with_clock(Arc::new(|| NOW_MS));
    Rig { investigator, control, server, spawner, store }
}

/// The moment every test investigation is saved at.
const NOW_MS: i64 = 1_700_000_000_000;

fn rig(server: MockLlm) -> Rig {
    rig_with(|shared| Box::new(SharedLlm(shared)), Arc::new(server))
}

fn finding() -> SubjectRef {
    SubjectRef::Finding { id: 7 }
}

fn wait_for(
    rig: &Rig,
    id: u64,
    mut done: impl FnMut(&str) -> bool,
) -> syntrac_app_lib::ai_investigate::InvestigationView {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let view = rig.investigator.poll(id).expect("the job exists");
        if done(&view.phase) {
            return view;
        }
        assert!(Instant::now() < deadline, "timed out in phase {}", view.phase);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_answer_streams_in_and_the_page_can_see_exactly_what_was_sent() {
    let rig = rig(MockLlm::answering(&["It is ", "a sync tool."]));
    let id = rig.investigator.start(&finding()).unwrap();

    let view = wait_for(&rig, id, |phase| phase == "done");

    assert_eq!(view.answer, "It is a sync tool.");
    assert_eq!(view.error, None);
    assert!(view.sent.contains("<evidence>") && view.sent.contains("widgetsync.exe") && view.sent.contains("<user>"));
    assert!(!view.sent.to_lowercase().contains("alice"));
    let asked = rig.server.requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].model, "some-model", "the model name goes exactly as typed");
    assert_eq!(asked[0].messages.len(), 2);
    assert_eq!(asked[0].messages[1].content, view.sent);
    assert!(asked[0].messages[0].content.to_lowercase().contains("data"));
}

#[test]
fn nothing_runs_while_the_help_is_off_or_not_set_up() {
    let rig = rig(MockLlm::answering(&["x"]));
    rig.control.save(AiSettingsDto { enabled: false, ..settings() }).unwrap();
    assert!(rig.investigator.start(&finding()).is_err());
    rig.control.save(AiSettingsDto { model: String::new(), ..settings() }).unwrap();
    assert!(rig.investigator.start(&finding()).is_err());
    assert!(rig.server.requests().is_empty());
}

#[test]
fn a_server_that_is_not_running_is_reported_and_not_started_unless_the_user_chose_that() {
    let rig = rig(MockLlm::failing(ServerState::NotRunning, LlmError::NotRunning));
    let message = rig.investigator.start(&finding()).unwrap_err();
    assert!(message.contains("not running"), "{message}");
    assert_eq!(rig.spawner.count(), 0);

    rig.server.set_state(ServerState::Loading);
    assert!(rig.investigator.start(&finding()).unwrap_err().contains("loading"));
}

#[test]
fn when_the_user_chose_it_investigate_starts_the_server_and_waits_for_it() {
    let rig = rig(MockLlm::answering(&["Done."]));
    rig.server.set_state(ServerState::NotRunning);
    rig.control.save(AiSettingsDto { start_on_investigate: true, ..settings() }).unwrap();

    let id = rig.investigator.start(&finding()).unwrap();
    assert_eq!(rig.spawner.count(), 1, "the server was started");
    let waiting = wait_for(&rig, id, |phase| phase == "waiting");
    assert_eq!(waiting.answer, "");

    rig.server.set_state(ServerState::Ready);
    let view = wait_for(&rig, id, |phase| phase == "done");
    assert_eq!(view.answer, "Done.");
    assert_eq!(rig.spawner.count(), 1, "and only once");
}

#[test]
fn a_server_that_ends_while_it_is_awaited_fails_the_investigation() {
    let rig = rig(MockLlm::answering(&["x"]));
    rig.server.set_state(ServerState::NotRunning);
    rig.control.save(AiSettingsDto { start_on_investigate: true, ..settings() }).unwrap();
    let id = rig.investigator.start(&finding()).unwrap();
    rig.spawner.last().lock().unwrap().exit_code = Some(2);

    let view = wait_for(&rig, id, |phase| phase == "failed");

    assert!(view.error.unwrap().contains("ended"), "the server ended before it was ready");
}

#[test]
fn a_failing_server_gives_a_plain_message_and_no_answer() {
    let rig = rig(MockLlm::failing(ServerState::Ready, LlmError::TimedOut));
    let id = rig.investigator.start(&finding()).unwrap();
    let view = wait_for(&rig, id, |phase| phase == "failed");
    assert_eq!(view.answer, "");
    assert!(view.error.unwrap().contains("did not answer in time"));
}

#[test]
fn cancel_stops_a_running_answer_and_keeps_what_arrived() {
    let stopped = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&stopped);
    let rig = rig_with(move |_| Box::new(SlowLlm { stopped: Arc::clone(&seen) }), Arc::new(MockLlm::answering(&[])));
    let id = rig.investigator.start(&finding()).unwrap();
    wait_for(&rig, id, |_| rig.investigator.poll(id).is_some_and(|view| !view.answer.is_empty()));

    rig.investigator.cancel(id);

    let view = wait_for(&rig, id, |phase| phase == "cancelled");
    assert_eq!(view.answer, "first words");
    let deadline = Instant::now() + Duration::from_secs(2);
    while !stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(stopped.load(Ordering::SeqCst), "the model was told to stop");
}

#[test]
fn a_subject_that_cannot_be_found_is_reported() {
    let rig = rig(MockLlm::answering(&["x"]));
    let message = rig.investigator.start(&SubjectRef::Finding { id: 0 }).unwrap_err();
    assert!(message.contains("no longer exists"));
}

#[test]
fn the_preview_shows_the_prompt_without_needing_the_help_to_be_turned_on() {
    let rig = rig(MockLlm::answering(&["x"]));
    rig.control.save(AiSettingsDto { enabled: false, ..settings() }).unwrap();

    let outside = rig.investigator.preview(&finding(), true).unwrap();

    assert_eq!(outside.level, "external");
    assert!(outside.text.contains(&outside.system) && outside.text.contains("<evidence>"));
    for hidden in ["widgetsync", "alice", "Fabrikam\\Local"] {
        assert!(!outside.text.contains(hidden), "{hidden}");
    }
    let local = rig.investigator.preview(&finding(), false).unwrap();
    assert_eq!(local.level, "summary");
    assert!(local.text.contains("widgetsync.exe"));
    assert!(rig.server.requests().is_empty(), "a preview asks no server");
}

#[test]
fn only_the_latest_few_investigations_are_kept_and_an_unknown_one_is_nothing() {
    let rig = rig(MockLlm::answering(&["x"]));
    let ids: Vec<u64> = (0..10).map(|_| rig.investigator.start(&finding()).unwrap()).collect();
    assert!(rig.investigator.poll(ids[0]).is_none(), "the oldest is dropped");
    assert!(rig.investigator.poll(ids[9]).is_some());
    assert!(rig.investigator.poll(9999).is_none());
}

#[test]
fn an_answer_cut_off_by_the_length_limit_is_marked_so_it_does_not_look_finished() {
    let rig = rig(MockLlm::playing(vec![
        StreamEvent::Reasoning("thinking it over".into()),
        StreamEvent::Answer("The program is".into()),
        StreamEvent::CutOff,
    ]));
    let id = rig.investigator.start(&finding()).unwrap();

    let view = wait_for(&rig, id, |phase| phase == "done");

    assert_eq!(view.answer, "The program is");
    assert!(view.truncated, "the page must be able to say the answer is cut off");
    assert_eq!(view.reasoning_chars, "thinking it over".chars().count());
}

#[test]
fn a_finished_answer_is_not_marked_cut_off_and_the_limit_leaves_room_for_thinking() {
    let rig = rig(MockLlm::answering(&["Complete."]));
    let id = rig.investigator.start(&finding()).unwrap();
    let view = wait_for(&rig, id, |phase| phase == "done");
    assert!(!view.truncated);
    // A model that thinks first spends part of the limit on it, so the limit is generous.
    assert_eq!(rig.server.requests()[0].max_tokens, Some(3000));
}

#[test]
fn a_finished_answer_is_kept_with_what_is_needed_to_judge_it_later() {
    let rig = rig(MockLlm::answering(&["It is ", "a sync tool."]));
    let id = rig.investigator.start(&finding()).unwrap();
    let view = wait_for(&rig, id, |phase| phase == "done");

    // The page sees "done" and asks at once, so the answer must already be kept by then.
    let kept = rig.investigator.saved(&finding()).expect("the answer was kept");

    assert_eq!(kept.answer, "It is a sync tool.");
    assert!(!kept.truncated);
    assert_eq!(kept.model, "some-model");
    assert_eq!(kept.level, "summary");
    assert_eq!(kept.sent, view.sent);
    assert_eq!(kept.saved_ms, NOW_MS);
    assert_eq!(rig.investigator.saved_list().len(), 1);
}

#[test]
fn a_cut_off_answer_is_kept_and_still_says_it_was_cut_off() {
    let rig = rig(MockLlm::playing(vec![StreamEvent::Answer("The program is".into()), StreamEvent::CutOff]));
    let id = rig.investigator.start(&finding()).unwrap();
    wait_for(&rig, id, |phase| phase == "done");
    assert!(rig.investigator.saved(&finding()).unwrap().truncated);
}

#[test]
fn a_failed_a_stopped_or_an_empty_investigation_keeps_nothing() {
    let failing = rig(MockLlm::failing(ServerState::Ready, LlmError::TimedOut));
    let id = failing.investigator.start(&finding()).unwrap();
    wait_for(&failing, id, |phase| phase == "failed");
    assert!(failing.investigator.saved(&finding()).is_none());

    let stopped_flag = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&stopped_flag);
    let slow = rig_with(move |_| Box::new(SlowLlm { stopped: Arc::clone(&seen) }), Arc::new(MockLlm::answering(&[])));
    let id = slow.investigator.start(&finding()).unwrap();
    wait_for(&slow, id, |_| slow.investigator.poll(id).is_some_and(|view| !view.answer.is_empty()));
    slow.investigator.cancel(id);
    wait_for(&slow, id, |phase| phase == "cancelled");
    assert!(slow.investigator.saved(&finding()).is_none(), "half an answer is not worth keeping");

    let silent = rig(MockLlm::playing(vec![StreamEvent::Reasoning("only thinking".into())]));
    let id = silent.investigator.start(&finding()).unwrap();
    wait_for(&silent, id, |phase| phase == "done");
    assert!(silent.investigator.saved(&finding()).is_none(), "no answer, nothing to keep");
}

#[test]
fn asking_again_replaces_the_kept_answer() {
    let rig = rig(MockLlm::answering(&["First answer."]));
    let id = rig.investigator.start(&finding()).unwrap();
    wait_for(&rig, id, |phase| phase == "done");

    rig.store.put(syntrac_app_lib::investigation_store::SavedInvestigation {
        answer: "older text".into(),
        saved_ms: 1,
        ..rig.investigator.saved(&finding()).unwrap()
    });
    let again = rig.investigator.start(&finding()).unwrap();
    wait_for(&rig, again, |phase| phase == "done");

    let kept = rig.investigator.saved(&finding()).unwrap();
    assert_eq!(kept.answer, "First answer.");
    assert_eq!(kept.saved_ms, NOW_MS);
    assert_eq!(rig.investigator.saved_list().len(), 1);
}

#[test]
fn a_program_and_a_finding_are_kept_separately_and_forgetting_removes_one() {
    let rig = rig(MockLlm::answering(&["Answer."]));
    let program = SubjectRef::Program { path: r"C:\Users\alice\widgetsync.exe".into() };
    for subject in [finding(), program.clone()] {
        let id = rig.investigator.start(&subject).unwrap();
        wait_for(&rig, id, |phase| phase == "done");
    }
    assert_eq!(rig.investigator.saved_list().len(), 2);

    rig.investigator.forget(&program);

    assert!(rig.investigator.saved(&program).is_none());
    assert!(rig.investigator.saved(&finding()).is_some());
}
