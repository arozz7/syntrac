//! Which program a startup entry starts. The text of a service's binary path, a task's command or a Run value is turned into the
//! program's full path and its arguments. Pure: the environment (`%windir%`...) and the file system (to settle an unquoted path
//! that contains spaces) are passed in as functions. `docs/RUNAS-PLAN.md`, I4b.
//!
//! What it does not do: guess what a proxy (`rundll32`, `cmd /c`, `powershell`...) then loads. A proxy is recorded as itself, with
//! its arguments, because that is what really runs.

use crate::clean_text::clean_text;

/// Program names that start something else (a DLL, a script, an installer). Recorded as themselves.
const PROXIES: [&str; 9] =
    ["rundll32", "cmd", "powershell", "pwsh", "wscript", "cscript", "mshta", "msiexec", "regsvr32"];
const EXTENSIONS: [&str; 5] = [".exe", ".com", ".bat", ".cmd", ".scr"];
const MAX_ARGUMENT_CHARS: usize = 400;
/// An unquoted value is tried at this many word boundaries at most, and no path is longer than this (Windows' own limit is far
/// larger, but a legitimate startup entry is not).
const MAX_BOUNDARIES: usize = 32;
const MAX_PATH_CHARS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    /// The program's full path, as spelled in the entry (variables expanded, slashes normalised).
    pub path: String,
    /// What follows the program, cleaned and capped.
    pub arguments: String,
    /// The program starts something else (see `PROXIES`).
    pub proxy: bool,
    /// The file is there now. `false` is kept, not dropped: a startup entry for a missing file is worth knowing about.
    pub exists: bool,
}

fn has_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    EXTENSIONS.iter().any(|extension| lower.ends_with(extension))
}

fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with(r"\\")
        || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\')
}

/// Replaces `%NAME%` by the variable's value. `None` when a variable is not known (the entry cannot be mapped).
fn expand(text: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push('%');
            rest = after;
            continue;
        };
        let name = &after[..end];
        if name.is_empty() {
            out.push('%');
        } else {
            out.push_str(&env(name)?);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// A program path as written in an entry, made plain: device prefixes removed, `\SystemRoot` replaced, variables expanded, forward
/// slashes turned round, quotes and ends trimmed.
fn normalise(raw: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut text = raw.trim().trim_matches('"').trim().to_owned();
    for prefix in [r"\??\", r"\\?\"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.to_owned();
        }
    }
    if text.len() >= 12 && text[..12].eq_ignore_ascii_case(r"\SystemRoot\") {
        text = format!(r"{}\{}", env("SystemRoot")?, &text[12..]);
    }
    let text = expand(&text, env)?.replace('/', r"\");
    (!text.is_empty() && text.chars().count() <= MAX_PATH_CHARS).then_some(text)
}

/// A program name with no folder (`cmd.exe`, `notepad`): looked for in the system folder, as Windows would.
fn system_program(name: &str, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if name.contains('\\') || name.contains(':') || name.is_empty() {
        return None;
    }
    let file = if has_extension(name) { name.to_owned() } else { format!("{name}.exe") };
    let path = format!(r"{}\System32\{file}", env("SystemRoot")?);
    exists(&path).then_some(path)
}

fn build(path: String, arguments: &str, exists: &dyn Fn(&str) -> bool) -> LaunchTarget {
    let file = path.rsplit('\\').next().unwrap_or(&path).to_ascii_lowercase();
    let stem = file.rsplit_once('.').map_or(file.as_str(), |(stem, _)| stem);
    let proxy = PROXIES.contains(&stem);
    let found = exists(&path);
    LaunchTarget {
        path,
        arguments: clean_text(arguments, MAX_ARGUMENT_CHARS).unwrap_or_default(),
        proxy,
        exists: found,
    }
}

/// One program path, as the whole of a quoted part: an extension is added only when that file exists.
fn quoted_program(raw: &str, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let path = normalise(raw, env)?;
    if !is_absolute(&path) {
        return system_program(&path, env, exists);
    }
    if has_extension(&path) {
        return Some(path);
    }
    let with_exe = format!("{path}.exe");
    exists(&with_exe).then_some(with_exe)
}

/// The program an unquoted value starts: the shortest prefix, ending at a word boundary, that is a path ending in a program
/// extension and exists (what Windows itself tries first); when none exists, the first prefix that looks like a program.
fn unquoted_program(
    command: &str,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<(String, usize)> {
    let mut ends: Vec<usize> =
        command.char_indices().filter(|(_, c)| c.is_whitespace()).map(|(at, _)| at).take(MAX_BOUNDARIES).collect();
    ends.push(command.len());
    let mut first_looking: Option<(String, usize)> = None;
    for (index, end) in ends.iter().copied().enumerate() {
        let prefix = &command[..end];
        if index == 0 {
            // The first word on its own may be a bare name in the system folder.
            if let Some(path) =
                normalise(prefix, env)
                    .and_then(|p| if is_absolute(&p) { None } else { system_program(&p, env, exists) })
            {
                return Some((path, end));
            }
        }
        let Some(path) = normalise(prefix, env) else { continue };
        if !is_absolute(&path) || !has_extension(&path) {
            continue;
        }
        if exists(&path) {
            return Some((path, end));
        }
        first_looking.get_or_insert((path, end));
    }
    first_looking
}

/// The program that `command` starts, with its arguments. `None` when no program path can be made of it (empty, an unknown
/// variable, no recognisable program).
pub fn launch_target(
    command: &str,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<LaunchTarget> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if let Some(after) = command.strip_prefix('"') {
        if let Some(end) = after.find('"') {
            let path = quoted_program(&after[..end], env, exists)?;
            return Some(build(path, &after[end + 1..], exists));
        }
        // An opening quote that is never closed: read the rest as if it were not quoted.
        return launch_target(after, env, exists);
    }
    let (path, end) = unquoted_program(command, env, exists)?;
    Some(build(path, &command[end..], exists))
}
