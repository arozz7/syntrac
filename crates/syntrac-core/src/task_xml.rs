//! What a Windows scheduled-task definition says: the programs it runs, when it starts, whether it is switched off. Pure: the text
//! of one task file in, facts out (`docs/RUNAS-PLAN.md`, I4b).

use crate::clean_text::clean_text;
use crate::mini_xml::{parse_xml, XmlNode};

/// Longest command and arguments kept.
const MAX_COMMAND_CHARS: usize = 1024;
const MAX_ARGUMENT_CHARS: usize = 400;

/// One program a task runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAction {
    pub command: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInfo {
    /// `false` when the task's own settings switch it off (a trigger that is switched off does not count).
    pub enabled: bool,
    /// The kinds of trigger, lower case without the word "Trigger" ("logon", "boot", "time"...), once each, sorted.
    pub triggers: Vec<String>,
    /// The programs it runs (`Exec` actions only).
    pub actions: Vec<TaskAction>,
    /// Actions that run a COM handler instead of a command: Windows' own, with no program path to map.
    pub com_handlers: u32,
}

fn action_of(exec: &XmlNode) -> Option<TaskAction> {
    let command = clean_text(exec.child_text("Command"), MAX_COMMAND_CHARS)?;
    let arguments = clean_text(exec.child_text("Arguments"), MAX_ARGUMENT_CHARS).unwrap_or_default();
    Some(TaskAction { command, arguments })
}

/// Reads one task file. `None` when it is not a task definition or not well formed.
pub fn parse_task(xml: &str) -> Option<TaskInfo> {
    let root = parse_xml(xml)?;
    if root.name != "Task" {
        return None;
    }
    let enabled =
        root.child("Settings").is_none_or(|settings| !settings.child_text("Enabled").eq_ignore_ascii_case("false"));
    let mut triggers: Vec<String> = root
        .child("Triggers")
        .map(|triggers| {
            triggers
                .children
                .iter()
                .map(|trigger| trigger.name.trim_end_matches("Trigger").to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    triggers.sort();
    triggers.dedup();
    let (mut actions, mut com_handlers) = (Vec::new(), 0);
    if let Some(list) = root.child("Actions") {
        for action in &list.children {
            match action.name.as_str() {
                "Exec" => actions.extend(action_of(action)),
                "ComHandler" => com_handlers += 1,
                _ => {}
            }
        }
    }
    Some(TaskInfo { enabled, triggers, actions, com_handlers })
}
