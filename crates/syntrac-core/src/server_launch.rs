//! Phase 4, slice L4c (docs/PHASE4-PLAN.md): the command the user may set up so the app can start their model server. The rules here decide
//! what is allowed to run at all; the program is only ever started by a button the user presses, directly (no shell), and a person sees the
//! exact command before it runs.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::exe_location::{classify_location, LocationClass};

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_CHARS: usize = 1024;
/// Folders whose name says "not a place to run a server from", wherever they are.
const REFUSED_FOLDER_NAMES: [&str; 3] = ["downloads", "temp", "tmp"];
const SEPARATORS: [char; 2] = ['\\', '/'];

/// What to run: a full path to an `.exe`, its arguments as a list (never one text to be split), and optionally the folder to run in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LaunchProblem {
    #[error(r"the program must be written as a full path such as C:\Tools\server.exe")]
    NotAbsolute,
    #[error("no program is set")]
    NoProgram,
    #[error("the program path has characters or parts that are not allowed")]
    Malformed,
    #[error("only .exe files are started (a script would need a shell)")]
    NotAnExe,
    #[error("programs in temporary, download or network folders are not started")]
    UnsafeFolder,
    #[error("an argument is too long, has a control character, or there are too many")]
    BadArgument,
    #[error("the working folder must be a full local path")]
    BadWorkingFolder,
}

impl LaunchCommand {
    /// Whether the command may be started. This checks the text only; that the file exists is found out when it is started.
    pub fn check(&self) -> Result<(), LaunchProblem> {
        check_program(self.program.trim())?;
        if self.args.len() > MAX_ARGUMENTS
            || self.args.iter().any(|arg| arg.chars().count() > MAX_ARGUMENT_CHARS || arg.chars().any(char::is_control))
        {
            return Err(LaunchProblem::BadArgument);
        }
        match self.working_dir.as_deref().map(str::trim).filter(|folder| !folder.is_empty()) {
            Some(folder) if !is_local_absolute(folder) || has_parent_step(folder) || has_bad_character(folder) => {
                Err(LaunchProblem::BadWorkingFolder)
            }
            _ => Ok(()),
        }
    }

    /// The program path as it is run: without the spaces around it that a person may have typed.
    pub fn program_path(&self) -> &str {
        self.program.trim()
    }

    /// The working folder as it is used: `None` when it is empty.
    pub fn working_folder(&self) -> Option<&str> {
        self.working_dir.as_deref().map(str::trim).filter(|folder| !folder.is_empty())
    }

    /// The command as it is shown to the person before it runs, quoted the way Windows reads it.
    pub fn display(&self) -> String {
        let mut text = format!("\"{}\"", self.program_path());
        for arg in &self.args {
            text.push(' ');
            if arg.is_empty() || arg.contains([' ', '\t', '"']) {
                text.push('"');
                text.push_str(&arg.replace('"', "\\\""));
                text.push('"');
            } else {
                text.push_str(arg);
            }
        }
        text
    }
}

fn check_program(program: &str) -> Result<(), LaunchProblem> {
    if program.is_empty() {
        return Err(LaunchProblem::NoProgram);
    }
    if has_bad_character(program) {
        return Err(LaunchProblem::Malformed);
    }
    if program.starts_with("\\\\") || program.starts_with("//") {
        return Err(LaunchProblem::UnsafeFolder);
    }
    if !is_local_absolute(program) {
        return Err(LaunchProblem::NotAbsolute);
    }
    if has_parent_step(program) {
        return Err(LaunchProblem::Malformed);
    }
    let file_name = program.rsplit(SEPARATORS).next().unwrap_or_default();
    if !file_name.to_ascii_lowercase().ends_with(".exe") {
        return Err(LaunchProblem::NotAnExe);
    }
    let folders = &program[..program.len() - file_name.len()];
    let named_unsafe =
        folders.split(SEPARATORS).any(|part| REFUSED_FOLDER_NAMES.contains(&part.to_ascii_lowercase().as_str()));
    if named_unsafe || matches!(classify_location(program), LocationClass::Temporary | LocationClass::Network) {
        return Err(LaunchProblem::UnsafeFolder);
    }
    Ok(())
}

/// `X:\...` or `X:/...`: a drive letter and a separator. A network path (`\\host\...`) and a relative path are not.
fn is_local_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/')
}

fn has_parent_step(path: &str) -> bool {
    path.split(SEPARATORS).any(|part| part == "..")
}

/// Control characters and the characters Windows does not allow in a path.
fn has_bad_character(text: &str) -> bool {
    text.chars().any(|c| c.is_control() || matches!(c, '"' | '|' | '<' | '>' | '*' | '?'))
}
