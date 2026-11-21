//! Phase 4, slice L4c (docs/PHASE4-PLAN.md): what the page may ask about the optional AI help: read and save its settings, see the state of the
//! model server, and start or stop the one this app started. Nothing here talks to a server unless the page asks for the status or presses
//! Start, and nothing happens at all while the help is turned off.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use syntrac_core::llm::{AiSettings, DetailLevel, LlmService, LoopbackEndpoint, SettingsProblem};
use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::{LaunchError, ProcessSpawner, ServerManager, ServerStatus};

use crate::prefs::PrefsHandle;

/// How long a started server may take to answer before the status says it is slow (a large model reads from disk first).
const STARTUP_LIMIT: Duration = Duration::from_secs(15 * 60);

/// How the program reaches the model server at an address already proven to be on this machine.
pub type ServiceFactory = Arc<dyn Fn(LoopbackEndpoint) -> Box<dyn LlmService> + Send + Sync>;

/// The settings as the page sees them (flat, with the launch command spread over three fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsDto {
    pub enabled: bool,
    pub endpoint: String,
    pub model: String,
    /// `summary` or `full`.
    pub detail_level: String,
    pub launch_program: String,
    pub launch_args: Vec<String>,
    pub launch_working_dir: String,
    pub start_on_investigate: bool,
    pub stop_on_quit: bool,
}

impl From<&AiSettings> for AiSettingsDto {
    fn from(settings: &AiSettings) -> Self {
        let launch = settings.launch.clone().unwrap_or_default();
        Self {
            enabled: settings.enabled,
            endpoint: settings.endpoint.clone(),
            model: settings.model.clone(),
            detail_level: match settings.detail_level {
                DetailLevel::Summary => "summary",
                DetailLevel::Full => "full",
            }
            .to_owned(),
            launch_program: launch.program,
            launch_args: launch.args,
            launch_working_dir: launch.working_dir.unwrap_or_default(),
            start_on_investigate: settings.start_on_investigate,
            stop_on_quit: settings.stop_on_quit,
        }
    }
}

impl AiSettingsDto {
    /// The settings this describes, or what is wrong with them. An empty address and an empty launch command are allowed (not set up yet).
    pub fn into_settings(self) -> Result<AiSettings, String> {
        let detail_level = match self.detail_level.as_str() {
            "summary" => DetailLevel::Summary,
            "full" => DetailLevel::Full,
            other => return Err(format!("unknown detail level: {other}")),
        };
        let launch = LaunchCommand {
            program: self.launch_program.trim().to_owned(),
            args: self.launch_args,
            working_dir: Some(self.launch_working_dir.trim().to_owned()).filter(|folder| !folder.is_empty()),
        };
        let has_launch = !launch.program.is_empty() || !launch.args.is_empty() || launch.working_dir.is_some();
        if has_launch {
            launch.check().map_err(|problem| problem.to_string())?;
        }
        if !self.endpoint.trim().is_empty() {
            LoopbackEndpoint::parse(&self.endpoint).map_err(|problem| problem.to_string())?;
        }
        Ok(AiSettings {
            enabled: self.enabled,
            endpoint: self.endpoint.trim().to_owned(),
            model: self.model.trim().to_owned(),
            detail_level,
            launch: has_launch.then_some(launch),
            start_on_investigate: self.start_on_investigate,
            stop_on_quit: self.stop_on_quit,
        })
    }
}

/// What the AI help section shows about the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusView {
    /// `off`, `not_set_up`, `not_running`, `starting`, `slow_to_start`, `loading`, `ready`, `exited` or `failed`.
    pub state: String,
    /// A sentence for the person.
    pub message: String,
    pub seconds: Option<u64>,
    pub started_by_this_app: bool,
    pub can_start: bool,
    pub can_stop: bool,
    /// The exact command a Start would run, when one is set and allowed.
    pub command: Option<String>,
}

pub struct AiControl {
    prefs: PrefsHandle,
    manager: ServerManager,
    service: ServiceFactory,
}

impl AiControl {
    pub fn new(prefs: PrefsHandle, spawner: Box<dyn ProcessSpawner>, service: ServiceFactory) -> Self {
        Self { prefs, manager: ServerManager::new(spawner, STARTUP_LIMIT), service }
    }

    pub fn settings(&self) -> AiSettingsDto {
        AiSettingsDto::from(&self.prefs.ai_settings())
    }

    /// Checks and keeps the settings. Nothing is kept when something is wrong.
    pub fn save(&self, dto: AiSettingsDto) -> Result<AiSettingsDto, String> {
        let settings = dto.into_settings()?;
        self.prefs.set_ai_settings(settings.clone());
        Ok(AiSettingsDto::from(&settings))
    }

    pub fn status(&self) -> AiStatusView {
        let settings = self.prefs.ai_settings();
        if !settings.enabled {
            return plain("off", "AI help is turned off.", &settings);
        }
        let endpoint = match settings.ready_to_use() {
            Ok(endpoint) => endpoint,
            Err(problem @ (SettingsProblem::NoModel | SettingsProblem::Endpoint(_))) => {
                return plain("not_set_up", &format!("Not set up yet: {problem}."), &settings)
            }
            Err(problem) => return plain("off", &problem.to_string(), &settings),
        };
        let server = (self.service)(endpoint);
        let status = self.manager.status(server.as_ref(), Instant::now());
        view_of(&status, &settings)
    }

    /// Starts the server from the launch command, after the same checks as the status. Only a button calls this.
    pub fn start(&self) -> Result<(), String> {
        let settings = self.prefs.ai_settings();
        let endpoint = settings.ready_to_use().map_err(|problem| problem.to_string())?;
        let command =
            settings.launch_command().map_err(|problem| format!("There is no usable launch command: {problem}."))?;
        let server = (self.service)(endpoint);
        self.manager.start(command, server.as_ref(), Instant::now()).map_err(describe_launch_error)
    }

    pub fn stop(&self) -> Result<(), String> {
        self.manager.stop().map_err(describe_launch_error)
    }

    /// At the end of the program: stops the server this app started when the user chose that.
    pub fn shutdown(&self) {
        self.manager.shutdown(self.prefs.ai_settings().stop_on_quit);
    }
}

fn plain(state: &str, message: &str, settings: &AiSettings) -> AiStatusView {
    AiStatusView {
        state: state.to_owned(),
        message: message.to_owned(),
        seconds: None,
        started_by_this_app: false,
        can_start: false,
        can_stop: false,
        command: settings.launch_command().ok().map(LaunchCommand::display),
    }
}

fn view_of(status: &ServerStatus, settings: &AiSettings) -> AiStatusView {
    let command = settings.launch_command().ok();
    let startable = command.is_some();
    let (state, message, seconds, ours, can_start, can_stop) = match status {
        ServerStatus::NotRunning => {
            ("not_running", "The model server is not running.".to_owned(), None, false, startable, false)
        }
        ServerStatus::Starting { seconds } => (
            "starting",
            "Starting. A large model can take several minutes to load.".to_owned(),
            Some(*seconds),
            true,
            false,
            true,
        ),
        ServerStatus::SlowToStart { seconds } => (
            "slow_to_start",
            "Still not answering after a long time. You can stop it and look at the server's own log.".to_owned(),
            Some(*seconds),
            true,
            false,
            true,
        ),
        ServerStatus::Loading => {
            ("loading", "A model server is running and still loading.".to_owned(), None, false, false, false)
        }
        ServerStatus::Ready { started_by_this_app } => {
            let message = if *started_by_this_app {
                "The model server is ready (started by this app)."
            } else {
                "A model server is ready (it was already running; this app will not stop it)."
            };
            ("ready", message.to_owned(), None, *started_by_this_app, false, *started_by_this_app)
        }
        ServerStatus::Exited { code } => {
            let how = code.map_or_else(|| "ended".to_owned(), |code| format!("ended with code {code}"));
            ("exited", format!("The server this app started has {how}."), None, false, startable, false)
        }
        ServerStatus::Failed(reason) => {
            ("failed", format!("The model server answered badly: {reason}"), None, false, startable, false)
        }
    };
    AiStatusView {
        state: state.to_owned(),
        message,
        seconds,
        started_by_this_app: ours,
        can_start,
        can_stop,
        command: command.map(LaunchCommand::display),
    }
}

fn describe_launch_error(error: LaunchError) -> String {
    match error {
        LaunchError::Refused(problem) => format!("The launch command is not allowed: {problem}."),
        other => format!("{other}."),
    }
}
