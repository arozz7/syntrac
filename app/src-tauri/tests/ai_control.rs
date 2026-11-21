//! What the page may ask about the optional AI help (docs/PHASE4-PLAN.md, L4c), with a scripted server and a fake process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use syntrac_app_lib::ai_control::{AiControl, AiSettingsDto, ServiceFactory};
use syntrac_app_lib::prefs::PrefsHandle;
use syntrac_core::llm::{ChatRequest, Flow, LlmError, LlmService, ServerState, StreamEvent};
use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::{ChildHandle, LaunchError, ProcessSpawner};
use syntrac_testkit::mock_llm::MockLlm;
use syntrac_testkit::mock_process::FakeSpawner;

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
    control: AiControl,
    server: Arc<MockLlm>,
    spawner: Arc<FakeSpawner>,
    prefs: PrefsHandle,
    server_lookups: Arc<AtomicUsize>,
}

fn rig() -> Rig {
    let prefs = PrefsHandle::in_memory(syntrac_app_lib::prefs::Prefs::default());
    let server = Arc::new(MockLlm::failing(ServerState::NotRunning, LlmError::NotRunning));
    let spawner = Arc::new(FakeSpawner::default());
    let server_lookups = Arc::new(AtomicUsize::new(0));
    let (shared, counter) = (Arc::clone(&server), Arc::clone(&server_lookups));
    let service: ServiceFactory = Arc::new(move |_endpoint| {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::new(SharedLlm(Arc::clone(&shared)))
    });
    let control = AiControl::new(prefs.clone(), Box::new(SharedSpawner(Arc::clone(&spawner))), service);
    Rig { control, server, spawner, prefs, server_lookups }
}

fn configured() -> AiSettingsDto {
    AiSettingsDto {
        enabled: true,
        endpoint: "http://127.0.0.1:1234/v1".into(),
        model: "some-model".into(),
        detail_level: "summary".into(),
        launch_program: r"C:\Tools\srv.exe".into(),
        launch_args: vec!["serve".into()],
        launch_working_dir: String::new(),
        start_on_investigate: false,
        stop_on_quit: false,
    }
}

fn set_up() -> Rig {
    let rig = rig();
    rig.control.save(configured()).unwrap();
    rig
}

#[test]
fn everything_is_off_until_the_user_turns_it_on_and_nothing_asks_a_server() {
    let rig = rig();
    let status = rig.control.status();
    assert_eq!(status.state, "off");
    assert!(!status.can_start && !status.can_stop);
    assert_eq!(rig.server_lookups.load(Ordering::SeqCst), 0);
    assert!(rig.control.start().is_err());
    assert_eq!(rig.spawner.count(), 0);
}

#[test]
fn turned_on_without_a_model_or_an_address_it_says_it_is_not_set_up_without_asking_a_server() {
    let rig = rig();
    rig.control.save(AiSettingsDto { model: String::new(), ..configured() }).unwrap();
    assert_eq!(rig.control.status().state, "not_set_up");
    rig.control.save(AiSettingsDto { endpoint: String::new(), ..configured() }).unwrap();
    assert_eq!(rig.control.status().state, "not_set_up");
    assert_eq!(rig.server_lookups.load(Ordering::SeqCst), 0);
}

#[test]
fn settings_that_break_the_rules_are_refused_and_nothing_is_kept() {
    let rig = rig();
    let kept = rig.control.settings();
    let refused = [
        AiSettingsDto { endpoint: "http://example.com/v1".into(), ..configured() },
        AiSettingsDto { launch_program: r"C:\Users\a\Downloads\srv.exe".into(), ..configured() },
        AiSettingsDto { launch_program: "srv.bat".into(), ..configured() },
        AiSettingsDto { detail_level: "everything".into(), ..configured() },
    ];
    for dto in refused {
        assert!(rig.control.save(dto.clone()).is_err(), "{dto:?}");
        assert_eq!(rig.control.settings(), kept);
    }
}

#[test]
fn saved_settings_come_back_and_an_empty_launch_command_means_none() {
    let rig = rig();
    assert_eq!(rig.control.save(configured()).unwrap(), configured());
    assert_eq!(rig.prefs.ai_settings().launch.unwrap().args, vec!["serve".to_owned()]);

    let without = AiSettingsDto { launch_program: String::new(), launch_args: vec![], ..configured() };
    rig.control.save(without.clone()).unwrap();
    assert!(rig.prefs.ai_settings().launch.is_none());
    assert_eq!(rig.control.settings(), without);
}

#[test]
fn a_stopped_server_can_be_started_only_when_a_command_is_set_and_the_exact_command_is_shown() {
    let rig = set_up();
    let status = rig.control.status();
    assert_eq!(status.state, "not_running");
    assert!(status.can_start && !status.can_stop);
    assert_eq!(status.command.as_deref(), Some(r#""C:\Tools\srv.exe" serve"#));

    rig.control.save(AiSettingsDto { launch_program: String::new(), launch_args: vec![], ..configured() }).unwrap();
    let status = rig.control.status();
    assert!(!status.can_start && status.command.is_none(), "without a command the app does not manage the server");
    assert!(rig.control.start().is_err());
}

#[test]
fn starting_shows_progress_then_ready_with_a_stop_button_only_for_its_own_server() {
    let rig = set_up();
    rig.control.start().unwrap();
    assert_eq!(rig.spawner.count(), 1);
    let status = rig.control.status();
    assert_eq!(status.state, "starting");
    assert!(status.can_stop && !status.can_start && status.started_by_this_app);

    rig.server.set_state(ServerState::Ready);
    let status = rig.control.status();
    assert_eq!(status.state, "ready");
    assert!(status.can_stop && status.started_by_this_app);

    rig.control.stop().unwrap();
    assert!(rig.spawner.last().lock().unwrap().killed);
}

#[test]
fn a_server_that_was_already_running_is_shown_but_neither_started_again_nor_stopped() {
    let rig = set_up();
    rig.server.set_state(ServerState::Ready);
    let status = rig.control.status();
    assert_eq!((status.state.as_str(), status.can_start, status.can_stop), ("ready", false, false));
    assert!(rig.control.start().unwrap_err().contains("already answers"));
    assert!(rig.control.stop().is_err());
    assert_eq!(rig.spawner.count(), 0);
}

#[test]
fn a_server_that_ends_is_reported_and_can_be_started_again() {
    let rig = set_up();
    rig.control.start().unwrap();
    rig.spawner.last().lock().unwrap().exit_code = Some(1);
    let status = rig.control.status();
    assert_eq!(status.state, "exited");
    assert!(status.message.contains("code 1") && status.can_start);
}

#[test]
fn at_quit_the_server_is_stopped_only_when_the_user_chose_that() {
    let rig = set_up();
    rig.control.start().unwrap();
    rig.control.shutdown();
    assert!(!rig.spawner.last().lock().unwrap().killed);

    rig.control.save(AiSettingsDto { stop_on_quit: true, ..configured() }).unwrap();
    rig.control.shutdown();
    assert!(rig.spawner.last().lock().unwrap().killed);
}
