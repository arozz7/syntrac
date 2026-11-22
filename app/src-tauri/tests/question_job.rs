//! Running one question (docs/PHASE5-PLAN.md, B3): the page asks, polls, and may stop. A scripted model and fake facts stand in for the real
//! ones, so everything runs with no server and no database.

use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_app_lib::ai_control::{AiControl, AiSettingsDto, ServiceFactory};
use syntrac_app_lib::history_programs::ProgramMetricParam;
use syntrac_app_lib::prefs::{Prefs, PrefsHandle};
use syntrac_app_lib::question_job::{QuestionView, Questioner};
use syntrac_app_lib::question_tools::{ProgramFamily, QuestionData, TopProgram};
use syntrac_core::investigate::{Evidence, FindingFacts, ProgramFacts};
use syntrac_core::llm::{ChatRequest, Flow, LlmError, LlmService, ServerState, StreamEvent};
use syntrac_core::question_config::QuestionConfig;
use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::{ChildHandle, LaunchError, ProcessSpawner};
use syntrac_core::tool_chat::{TurnMessage, TurnReply, TurnRequest};
use syntrac_testkit::mock_process::FakeSpawner;
use syntrac_testkit::mock_tools::{asks_for, says, ScriptedTurns};

struct FakeData;

impl QuestionData for FakeData {
    fn program(&self, _path: &str) -> Result<Option<Evidence>, String> {
        Ok(Some(Evidence {
            program: Some(ProgramFacts {
                file_name: "widgetsync.exe".into(),
                location: "user_profile".into(),
                ..ProgramFacts::default()
            }),
            ..Evidence::default()
        }))
    }

    fn findings(&self, _limit: usize) -> Result<Vec<FindingFacts>, String> {
        Ok(vec![])
    }

    fn top_programs(&self, _: ProgramMetricParam, _: u64, _: usize) -> Result<Vec<TopProgram>, String> {
        Ok(vec![])
    }

    fn family(&self, _path: &str) -> Result<Option<ProgramFamily>, String> {
        Ok(None)
    }
}

struct Shared(Arc<ScriptedTurns>);

impl LlmService for Shared {
    fn state(&self) -> ServerState {
        self.0.state()
    }

    fn stream_chat(&self, _: &ChatRequest, _: &mut dyn FnMut(StreamEvent) -> Flow) -> Result<(), LlmError> {
        Err(LlmError::BadResponse("not streamed".into()))
    }

    fn chat_turn(&self, request: &TurnRequest) -> Result<TurnReply, LlmError> {
        self.0.chat_turn(request)
    }
}

/// A model whose first reply comes slowly, so a question can be stopped while it waits.
struct Slow;

impl LlmService for Slow {
    fn state(&self) -> ServerState {
        ServerState::Ready
    }

    fn stream_chat(&self, _: &ChatRequest, _: &mut dyn FnMut(StreamEvent) -> Flow) -> Result<(), LlmError> {
        Err(LlmError::BadResponse("not streamed".into()))
    }

    fn chat_turn(&self, _: &TurnRequest) -> Result<TurnReply, LlmError> {
        std::thread::sleep(Duration::from_millis(300));
        asks_for(&[("program_facts", r#"{"path":"c:\\a.exe"}"#)])
    }
}

struct SharedSpawner(Arc<FakeSpawner>);

impl ProcessSpawner for SharedSpawner {
    fn spawn(&self, command: &LaunchCommand) -> Result<Box<dyn ChildHandle>, LaunchError> {
        self.0.spawn(command)
    }
}

fn settings(enabled: bool) -> AiSettingsDto {
    AiSettingsDto {
        enabled,
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

fn questioner(service: ServiceFactory, enabled: bool) -> Questioner {
    let prefs = PrefsHandle::in_memory(Prefs::default());
    let spawner = Arc::new(FakeSpawner::default());
    let control = Arc::new(AiControl::new(prefs.clone(), Box::new(SharedSpawner(spawner)), Arc::clone(&service)));
    control.save(settings(enabled)).unwrap();
    Questioner::new(prefs, control, service, Arc::new(FakeData), QuestionConfig::builtin())
        .with_wait_step(Duration::from_millis(10))
        .with_user_name(Some("alice".into()))
}

fn scripted(script: Vec<Result<TurnReply, LlmError>>) -> (Questioner, Arc<ScriptedTurns>) {
    let model = Arc::new(ScriptedTurns::new(script));
    let shared = Arc::clone(&model);
    let service: ServiceFactory = Arc::new(move |_| Box::new(Shared(Arc::clone(&shared))));
    (questioner(service, true), model)
}

fn finished(questioner: &Questioner, id: u64) -> QuestionView {
    let begin = Instant::now();
    loop {
        let view = questioner.poll(id).expect("the question is known");
        if !matches!(view.phase.as_str(), "waiting" | "running") {
            return view;
        }
        assert!(begin.elapsed() < Duration::from_secs(5), "the question did not finish: {view:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn help_that_is_off_is_not_ready_and_asks_nothing() {
    let model = Arc::new(ScriptedTurns::new(vec![says("never")]));
    let shared = Arc::clone(&model);
    let service: ServiceFactory = Arc::new(move |_| Box::new(Shared(Arc::clone(&shared))));
    let off = questioner(service, false);

    let refused = off.start("what is busy?").unwrap_err();

    assert!(refused.contains("not ready"), "{refused}");
    assert!(model.requests().is_empty());
}

#[test]
fn an_empty_question_is_refused() {
    let (questioner, model) = scripted(vec![says("never")]);
    assert!(questioner.start("   ").unwrap_err().contains("question"));
    assert!(model.requests().is_empty());
}

#[test]
fn a_question_is_answered_after_the_model_looks_something_up() {
    let (questioner, model) = scripted(vec![
        asks_for(&[("program_facts", r#"{"path":"c:\\apps\\widgetsync.exe"}"#)]),
        says("It is a sync tool."),
    ]);

    let id = questioner.start("What is widgetsync?").unwrap();
    let view = finished(&questioner, id);

    assert_eq!(view.phase, "done");
    assert_eq!(view.answer, "It is a sync tool.");
    assert_eq!(view.stopped.as_deref(), Some("answered"));
    assert_eq!(view.level, "summary");
    assert_eq!(view.calls.len(), 1);
    assert_eq!(view.calls[0].name, "program_facts");
    assert_eq!(view.calls[0].status, "done");
    assert!(view.calls[0].arguments.contains("widgetsync.exe"));
    let first = &model.requests()[0];
    assert_eq!(first.model, "some-model");
    assert_eq!(first.tools.len(), 4);
    assert_eq!(first.messages[0], TurnMessage::System(QuestionConfig::builtin().system.trim().to_owned()));
    assert_eq!(first.messages[1], TurnMessage::User("What is widgetsync?".into()));
}

#[test]
fn the_user_name_is_replaced_in_the_question_before_it_is_sent() {
    let (questioner, model) = scripted(vec![says("ok")]);
    let id = questioner.start("What did alice run?").unwrap();
    finished(&questioner, id);
    assert_eq!(model.requests()[0].messages[1], TurnMessage::User("What did <user> run?".into()));
}

#[test]
fn a_server_that_cannot_do_tools_is_explained_on_the_page() {
    let (questioner, _) = scripted(vec![Err(LlmError::ToolsUnsupported)]);
    let id = questioner.start("What is busy?").unwrap();

    let view = finished(&questioner, id);

    assert_eq!(view.phase, "failed");
    assert!(view.tools_unsupported);
    let error = view.error.unwrap();
    assert!(error.contains("tool calls") && error.contains("--jinja"), "{error}");
}

#[test]
fn another_failure_of_the_server_is_a_failed_question_with_its_reason() {
    let (questioner, _) = scripted(vec![Err(LlmError::TimedOut)]);
    let id = questioner.start("What is busy?").unwrap();

    let view = finished(&questioner, id);

    assert_eq!(view.phase, "failed");
    assert!(!view.tools_unsupported);
    assert!(view.error.unwrap().contains("did not answer in time"));
}

#[test]
fn stop_ends_the_question_and_keeps_the_calls_that_were_made() {
    let service: ServiceFactory = Arc::new(|_| Box::new(Slow));
    let questioner = questioner(service, true);
    let id = questioner.start("What is busy?").unwrap();

    questioner.cancel(id);
    let view = finished(&questioner, id);

    assert_eq!(view.phase, "cancelled");
    assert_eq!(view.stopped.as_deref(), Some("cancelled"));
}

#[test]
fn a_server_that_is_not_running_is_reported_before_anything_is_asked() {
    struct Down;
    impl LlmService for Down {
        fn state(&self) -> ServerState {
            ServerState::NotRunning
        }
        fn stream_chat(&self, _: &ChatRequest, _: &mut dyn FnMut(StreamEvent) -> Flow) -> Result<(), LlmError> {
            Err(LlmError::NotRunning)
        }
    }
    let service: ServiceFactory = Arc::new(|_| Box::new(Down));
    let questioner = questioner(service, true);

    let refused = questioner.start("What is busy?").unwrap_err();

    assert!(refused.contains("not running"), "{refused}");
}

#[test]
fn an_unknown_question_number_is_nothing() {
    let (questioner, _) = scripted(vec![]);
    assert!(questioner.poll(99).is_none());
    questioner.cancel(99);
}

#[test]
fn every_call_the_model_made_is_listed_for_the_page() {
    let (questioner, _) = scripted(vec![asks_for(&[("top_programs", "{}")]), says("done")]);
    let id = questioner.start("What is busiest?").unwrap();
    let view = finished(&questioner, id);
    assert_eq!(view.calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["top_programs"]);
}

/// A model whose turn does not return until it is released (or a few seconds pass), like a slow model thinking for minutes.
struct Gate(Arc<std::sync::atomic::AtomicBool>);

impl LlmService for Gate {
    fn state(&self) -> ServerState {
        ServerState::Ready
    }

    fn stream_chat(&self, _: &ChatRequest, _: &mut dyn FnMut(StreamEvent) -> Flow) -> Result<(), LlmError> {
        Err(LlmError::BadResponse("not streamed".into()))
    }

    fn chat_turn(&self, _: &TurnRequest) -> Result<TurnReply, LlmError> {
        let begin = Instant::now();
        while !self.0.load(std::sync::atomic::Ordering::SeqCst) && begin.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        says("a late answer")
    }
}

#[test]
fn stop_shows_at_once_even_while_a_turn_is_still_being_waited_for_and_the_late_answer_is_dropped() {
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate = Arc::clone(&release);
    let service: ServiceFactory = Arc::new(move |_| Box::new(Gate(Arc::clone(&gate))));
    let questioner = questioner(service, true);
    let id = questioner.start("What is busy?").unwrap();

    questioner.cancel(id);
    let at_once = questioner.poll(id).unwrap();
    assert_eq!(at_once.phase, "cancelled", "the page must not keep saying the model is working");
    assert_eq!(at_once.stopped.as_deref(), Some("cancelled"));

    release.store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(100));
    let later = questioner.poll(id).unwrap();
    assert_eq!(later.phase, "cancelled");
    assert_eq!(later.answer, "", "an answer that arrives after Stop is not shown");
}
