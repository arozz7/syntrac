//! The Startup folders as launch records (I4b): the all-users folder and each profile's. A shortcut is resolved to the program it
//! points at (that is what starts); a program, batch file or script placed there directly is the entry itself. Read only.

use std::path::{Path, PathBuf};

use syntrac_core::{LaunchSource, LaunchSourceError, LauncherKind, RawEntry, SourceRead};
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// What a shortcut points at: the program and its arguments.
pub trait ShortcutResolver {
    fn resolve(&self, shortcut: &Path) -> Option<(String, String)>;
}

/// The Windows shell's own shortcut reader. COM must be started on the calling thread first (`ComGuard`).
pub struct ShellShortcuts;

impl ShortcutResolver for ShellShortcuts {
    fn resolve(&self, shortcut: &Path) -> Option<(String, String)> {
        let wide: Vec<u16> = shortcut.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the calls; the buffers hold the lengths passed.
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let file: IPersistFile = link.cast().ok()?;
            file.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
            let mut path = [0u16; 1024];
            link.GetPath(&mut path, std::ptr::null_mut(), 0).ok()?;
            let mut arguments = [0u16; 1024];
            link.GetArguments(&mut arguments).ok()?;
            let text = |buffer: &[u16]| {
                String::from_utf16_lossy(&buffer[..buffer.iter().position(|u| *u == 0).unwrap_or(buffer.len())])
            };
            let program = text(&path);
            (!program.is_empty()).then(|| (program, text(&arguments)))
        }
    }
}

/// COM started for the current thread, ended when dropped.
pub struct ComGuard(bool);

impl ComGuard {
    pub fn start() -> Self {
        // SAFETY: balanced by `CoUninitialize` in `Drop` when it succeeded.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the successful `CoInitializeEx` of `start`.
            unsafe { CoUninitialize() };
        }
    }
}

pub struct StartupLaunchSource<S> {
    /// Each startup folder and the profile folder of the user it belongs to (none for the all-users folder).
    folders: Vec<(PathBuf, Option<String>)>,
    shortcuts: S,
    /// Start COM around each read (the real shell resolver needs it; a fake does not).
    needs_com: bool,
}

impl<S: ShortcutResolver> StartupLaunchSource<S> {
    pub fn new(folders: Vec<(PathBuf, Option<String>)>, shortcuts: S, needs_com: bool) -> Self {
        Self { folders, shortcuts, needs_com }
    }
}

impl StartupLaunchSource<ShellShortcuts> {
    /// The startup folders of this machine: all users', and one per profile folder under `<system drive>\Users`.
    pub fn for_this_machine() -> Self {
        let start_menu = r"Microsoft\Windows\Start Menu\Programs\Startup";
        let mut folders = Vec::new();
        if let Ok(data) = std::env::var("ProgramData") {
            folders.push((PathBuf::from(data).join(start_menu), None));
        }
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        if let Ok(users) = std::fs::read_dir(format!(r"{drive}\Users")) {
            for profile in users.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())) {
                let path = profile.path();
                folders
                    .push((path.join(r"AppData\Roaming").join(start_menu), Some(path.to_string_lossy().into_owned())));
            }
        }
        Self::new(folders, ShellShortcuts, true)
    }
}

const PROGRAM_EXTENSIONS: [&str; 5] = ["exe", "com", "bat", "cmd", "scr"];

fn quoted(path: &str) -> String {
    format!("\"{path}\"")
}

impl<S: ShortcutResolver> LaunchSource for StartupLaunchSource<S> {
    fn name(&self) -> &'static str {
        "Startup folders"
    }

    fn kinds(&self) -> &'static [LauncherKind] {
        &[LauncherKind::StartupFolder]
    }

    fn read(&self) -> Result<SourceRead, LaunchSourceError> {
        let _com = self.needs_com.then(ComGuard::start);
        let mut read = SourceRead::default();
        for (folder, profile) in &self.folders {
            let Ok(entries) = std::fs::read_dir(folder) else { continue }; // a folder that is not there has no entries
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
                let extension = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                let command = if extension == "lnk" {
                    match self.shortcuts.resolve(&path) {
                        Some((program, arguments)) if arguments.is_empty() => quoted(&program),
                        Some((program, arguments)) => format!("{} {arguments}", quoted(&program)),
                        None => {
                            read.unreadable += 1;
                            continue;
                        }
                    }
                } else if PROGRAM_EXTENSIONS.contains(&extension.as_str()) {
                    quoted(&path.to_string_lossy())
                } else {
                    continue; // desktop.ini, documents, internet shortcuts: nothing that starts a program by itself
                };
                let whose = match profile {
                    None => "all users".to_owned(),
                    Some(profile) => format!("user {}", profile.rsplit('\\').next().unwrap_or(profile)),
                };
                read.entries.push(RawEntry {
                    kind: LauncherKind::StartupFolder,
                    location: format!(r"Startup ({whose})\{name}"),
                    command,
                    detail: String::new(),
                    profile: profile.clone(),
                });
            }
        }
        Ok(read)
    }
}
