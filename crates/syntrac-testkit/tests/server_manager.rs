//! Starting, watching and stopping the model server the user set a launch command for (docs/PHASE4-PLAN.md, L4c), with a fake process.

use std::sync::Arc;
use std::time::{Duration, Instant};

use syntrac_core::llm::{LlmError, ServerState};
use syntrac_core::server_launch::{LaunchCommand, LaunchProblem};
use syntrac_core::server_manager::{LaunchError, ProcessSpawner, ServerManager, ServerStatus};
use syntrac_testkit::mock_llm::MockLlm;
use syntrac_testkit::mock_process::FakeSpawner;

const LIMIT: Duration = Duration::from_secs(600);

fn command() -> LaunchCommand {
    LaunchCommand { program: r"C:\Tools\srv.exe".into(), args: vec!["serve".into()], working_dir: None }
}

/// The manager and a handle on its fake spawner, sharing the recorded lists.
fn manager() -> (ServerManager, Arc<FakeSpawner>) {
    struct Shared(Arc<FakeSpawner>);
    impl ProcessSpawner for Shared {
        fn spawn(
            &self,
            command: &LaunchCommand,
        ) -> Result<Box<dyn syntrac_core::server_manager::ChildHandle>, LaunchError> {
            self.0.spawn(command)
        }
    }
    let spawner = Arc::new(FakeSpawner::default());
    (ServerManager::new(Box::new(Shared(Arc::clone(&spawner))), LIMIT), spawner)
}

fn down() -> MockLlm {
    MockLlm::failing(ServerState::NotRunning, LlmError::NotRunning)
}

#[test]
fn nothing_is_started_until_asked_and_the_status_says_so() {
    let (manager, spawner) = manager();
    assert_eq!(manager.status(&down(), Instant::now()), ServerStatus::NotRunning);
    assert_eq!(spawner.count(), 0);
}

#[test]
fn starting_runs_the_command_and_the_status_follows_the_server_until_it_is_ready() {
    let (manager, spawner) = manager();
    let server = down();
    let begin = Instant::now();

    manager.start(&command(), &server, begin).unwrap();
    assert_eq!(spawner.count(), 1);
    assert_eq!(spawner.spawned.lock().unwrap()[0], command());
    assert_eq!(manager.status(&server, begin + Duration::from_secs(5)), ServerStatus::Starting { seconds: 5 });

    server.set_state(ServerState::Loading);
    assert_eq!(manager.status(&server, begin + Duration::from_secs(40)), ServerStatus::Starting { seconds: 40 });

    server.set_state(ServerState::Ready);
    assert_eq!(
        manager.status(&server, begin + Duration::from_secs(90)),
        ServerStatus::Ready { started_by_this_app: true }
    );
}

#[test]
fn a_server_that_never_answers_is_called_slow_not_failed() {
    let (manager, _) = manager();
    let server = down();
    let begin = Instant::now();
    manager.start(&command(), &server, begin).unwrap();
    assert_eq!(manager.status(&server, begin + Duration::from_secs(601)), ServerStatus::SlowToStart { seconds: 601 });
}

#[test]
fn a_command_that_breaks_the_rules_is_never_started() {
    let (manager, spawner) = manager();
    let mut bad = command();
    bad.program = r"C:\Users\a\Downloads\srv.exe".into();
    assert_eq!(manager.start(&bad, &down(), Instant::now()), Err(LaunchError::Refused(LaunchProblem::UnsafeFolder)));
    assert_eq!(spawner.count(), 0);
}

#[test]
fn a_second_server_is_not_started_while_one_answers_or_while_ours_runs() {
    let (manager, spawner) = manager();
    let answering = MockLlm::answering(&[]);
    assert_eq!(manager.start(&command(), &answering, Instant::now()), Err(LaunchError::AlreadyRunning));
    answering.set_state(ServerState::Loading);
    assert_eq!(manager.start(&command(), &answering, Instant::now()), Err(LaunchError::AlreadyRunning));
    assert_eq!(spawner.count(), 0);
    assert_eq!(manager.status(&answering, Instant::now()), ServerStatus::Loading);
    answering.set_state(ServerState::Ready);
    assert_eq!(manager.status(&answering, Instant::now()), ServerStatus::Ready { started_by_this_app: false });

    let server = down();
    manager.start(&command(), &server, Instant::now()).unwrap();
    assert_eq!(manager.start(&command(), &server, Instant::now()), Err(LaunchError::AlreadyStarted));
    assert_eq!(spawner.count(), 1);
}

#[test]
fn stopping_ends_only_the_process_this_app_started() {
    let (manager, spawner) = manager();
    let server = down();
    manager.start(&command(), &server, Instant::now()).unwrap();

    manager.stop().unwrap();

    assert!(spawner.last().lock().unwrap().killed);
    assert_eq!(manager.status(&server, Instant::now()), ServerStatus::NotRunning);
    assert_eq!(manager.stop(), Err(LaunchError::NotStartedByThisApp), "nothing is left to stop");
}

#[test]
fn a_server_that_was_already_running_is_never_stopped() {
    let (manager, spawner) = manager();
    let _ = manager.status(&MockLlm::answering(&[]), Instant::now());
    assert_eq!(manager.stop(), Err(LaunchError::NotStartedByThisApp));
    assert_eq!(spawner.count(), 0);
}

#[test]
fn a_server_that_ends_by_itself_is_reported_with_its_exit_code_until_the_next_start() {
    let (manager, spawner) = manager();
    let server = down();
    manager.start(&command(), &server, Instant::now()).unwrap();
    spawner.last().lock().unwrap().exit_code = Some(3);

    assert_eq!(manager.status(&server, Instant::now()), ServerStatus::Exited { code: Some(3) });
    assert_eq!(manager.status(&server, Instant::now()), ServerStatus::Exited { code: Some(3) }, "it stays reported");
    assert_eq!(manager.stop(), Err(LaunchError::NotStartedByThisApp), "the process is gone");

    manager.start(&command(), &server, Instant::now()).unwrap();
    assert_eq!(spawner.count(), 2);
    assert!(matches!(manager.status(&server, Instant::now()), ServerStatus::Starting { .. }));
}

#[test]
fn a_start_that_fails_leaves_nothing_behind() {
    let (manager, spawner) = manager();
    *spawner.fail_with.lock().unwrap() = Some(LaunchError::ProgramMissing);
    assert_eq!(manager.start(&command(), &down(), Instant::now()), Err(LaunchError::ProgramMissing));
    assert_eq!(manager.status(&down(), Instant::now()), ServerStatus::NotRunning);
    assert_eq!(manager.stop(), Err(LaunchError::NotStartedByThisApp));
}

#[test]
fn a_stop_that_fails_says_so() {
    let (manager, spawner) = manager();
    manager.start(&command(), &down(), Instant::now()).unwrap();
    spawner.last().lock().unwrap().kill_fails = true;
    assert!(matches!(manager.stop(), Err(LaunchError::Kill(_))));
}

#[test]
fn at_the_end_of_the_program_the_server_is_stopped_only_when_the_setting_says_so() {
    let (manager, spawner) = manager();
    manager.start(&command(), &down(), Instant::now()).unwrap();
    manager.shutdown(false);
    assert!(!spawner.last().lock().unwrap().killed, "left running");

    manager.shutdown(true);
    assert!(spawner.last().lock().unwrap().killed, "stopped");
}

/// A server whose state probe waits until the test lets it go, like a probe that is slow to be answered.
struct SlowProbe {
    entered: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl syntrac_core::llm::LlmService for SlowProbe {
    fn state(&self) -> ServerState {
        self.entered.lock().unwrap().send(()).unwrap();
        let _ = self.release.lock().unwrap().recv_timeout(Duration::from_secs(5));
        ServerState::NotRunning
    }

    fn stream_chat(
        &self,
        _request: &syntrac_core::llm::ChatRequest,
        _on_event: &mut dyn FnMut(syntrac_core::llm::StreamEvent) -> syntrac_core::llm::Flow,
    ) -> Result<(), LlmError> {
        Err(LlmError::NotRunning)
    }
}

#[test]
fn a_slow_status_probe_does_not_make_a_start_or_a_stop_wait() {
    let (manager, spawner) = manager();
    let manager = Arc::new(manager);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let slow = SlowProbe { entered: std::sync::Mutex::new(entered_tx), release: std::sync::Mutex::new(release_rx) };
    let status_call = {
        let manager = Arc::clone(&manager);
        std::thread::spawn(move || manager.status(&slow, Instant::now()))
    };
    entered_rx.recv_timeout(Duration::from_secs(2)).expect("the status call is inside its probe");

    // While that probe waits, a start must go through at once (it is a different, quick server).
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let starter = {
        let manager = Arc::clone(&manager);
        std::thread::spawn(move || {
            let result = manager.start(&command(), &down(), Instant::now());
            done_tx.send(result).unwrap();
        })
    };
    let started = done_rx.recv_timeout(Duration::from_millis(1500));
    release_tx.send(()).unwrap();
    status_call.join().unwrap();
    starter.join().unwrap();

    assert_eq!(started.expect("the start was not held up by the slow probe"), Ok(()));
    assert_eq!(spawner.count(), 1);
}
