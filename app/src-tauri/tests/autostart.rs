//! Start with Windows (docs/ALERTS-PLAN.md, slice T2): the start mode (`--tray` means no window), the one command written to the user's
//! Run key, the rules around it (off by default, a repair when the program moved), and one real round trip on a throwaway registry key.
//! The production Run key itself is never written by a test.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use syntrac_app_lib::autostart::{
    autostart_command, exe_of_command, is_enabled, repair, set_enabled, AutostartStore, StartMode, VALUE_NAME,
};
use syntrac_app_lib::autostart_registry::{RegistryAutostart, RUN_KEY};

#[derive(Clone, Default)]
struct FakeStore {
    value: Arc<Mutex<Option<String>>>,
    fail_writes: Arc<Mutex<bool>>,
}

impl FakeStore {
    fn holding(command: &str) -> Self {
        Self { value: Arc::new(Mutex::new(Some(command.to_owned()))), ..Self::default() }
    }
    fn current(&self) -> Option<String> {
        self.value.lock().unwrap().clone()
    }
}

impl AutostartStore for FakeStore {
    fn registered(&self) -> Result<Option<String>, String> {
        Ok(self.current())
    }
    fn register(&self, command: &str) -> Result<(), String> {
        if *self.fail_writes.lock().unwrap() {
            return Err("access denied".into());
        }
        *self.value.lock().unwrap() = Some(command.to_owned());
        Ok(())
    }
    fn unregister(&self) -> Result<(), String> {
        if *self.fail_writes.lock().unwrap() {
            return Err("access denied".into());
        }
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}

const EXE: &str = r"C:\Program Files\Syntrac\syntrac-app.exe";

// ---- the start mode and the command

#[test]
fn the_tray_flag_means_no_window_and_anything_else_means_a_window() {
    let args = |list: &[&str]| list.iter().map(|text| (*text).to_owned()).collect::<Vec<_>>();
    assert_eq!(StartMode::from_args(args(&["syntrac-app.exe"])), StartMode::Window);
    assert_eq!(StartMode::from_args(args(&["syntrac-app.exe", "--tray"])), StartMode::Tray);
    assert_eq!(StartMode::from_args(args(&["syntrac-app.exe", "--TRAY"])), StartMode::Tray);
    assert_eq!(StartMode::from_args(args(&["syntrac-app.exe", "--tray-not", "x"])), StartMode::Window);
}

#[test]
fn the_command_is_the_quoted_program_and_the_tray_flag() {
    assert_eq!(autostart_command(Path::new(EXE)).unwrap(), format!("\"{EXE}\" --tray"));
    assert_eq!(autostart_command(Path::new(r"C:\a\b.exe")).unwrap(), r#""C:\a\b.exe" --tray"#);
}

#[test]
fn a_path_that_could_break_out_of_the_quotes_gets_no_command() {
    assert_eq!(autostart_command(Path::new("C:\\a\"b\\x.exe")), None);
    assert_eq!(autostart_command(Path::new("")), None);
}

#[test]
fn the_program_is_read_back_out_of_a_command() {
    assert_eq!(exe_of_command(&format!("\"{EXE}\" --tray")), Some(PathBuf::from(EXE)));
    assert_eq!(exe_of_command(r"C:\tools\x.exe --tray"), Some(PathBuf::from(r"C:\tools\x.exe")));
    assert_eq!(exe_of_command("  "), None);
}

// ---- switching it on and off

#[test]
fn it_is_off_until_it_is_switched_on_and_off_again_removes_the_entry() {
    let store = FakeStore::default();
    assert!(!is_enabled(&store));

    assert_eq!(set_enabled(&store, Path::new(EXE), true), Ok(true));
    assert_eq!(store.current(), Some(format!("\"{EXE}\" --tray")));
    assert!(is_enabled(&store));

    assert_eq!(set_enabled(&store, Path::new(EXE), false), Ok(false));
    assert_eq!(store.current(), None);
    assert_eq!(set_enabled(&store, Path::new(EXE), false), Ok(false), "removing what is not there is not an error");
}

#[test]
fn switching_it_on_again_replaces_a_stale_entry_with_this_program() {
    let store = FakeStore::holding(r#""C:\old\syntrac-app.exe" --tray"#);
    set_enabled(&store, Path::new(EXE), true).unwrap();
    assert_eq!(store.current(), Some(format!("\"{EXE}\" --tray")));
}

#[test]
fn a_write_that_fails_says_so_and_changes_nothing() {
    let store = FakeStore::holding(r#""C:\old\x.exe" --tray"#);
    *store.fail_writes.lock().unwrap() = true;

    assert!(set_enabled(&store, Path::new(EXE), true).is_err());
    assert!(set_enabled(&store, Path::new(EXE), false).is_err());
    assert_eq!(store.current(), Some(r#""C:\old\x.exe" --tray"#.to_owned()));
}

#[test]
fn a_program_that_cannot_be_named_safely_is_refused_and_nothing_is_written() {
    let store = FakeStore::default();
    assert!(set_enabled(&store, Path::new("C:\\a\"b\\x.exe"), true).is_err());
    assert_eq!(store.current(), None);
}

// ---- the repair after the program moved

#[test]
fn an_entry_whose_program_no_longer_exists_is_pointed_at_this_one() {
    let store = FakeStore::holding(r#""C:\old\syntrac-app.exe" --tray"#);

    let repaired = repair(&store, Path::new(EXE), |_| false);

    assert!(repaired);
    assert_eq!(store.current(), Some(format!("\"{EXE}\" --tray")));
}

#[test]
fn nothing_is_touched_when_it_is_off_already_right_or_points_at_another_program_that_exists() {
    let off = FakeStore::default();
    assert!(!repair(&off, Path::new(EXE), |_| false));
    assert_eq!(off.current(), None, "switched off stays off");

    let right = FakeStore::holding(&format!("\"{EXE}\" --tray"));
    assert!(!repair(&right, Path::new(EXE), |_| false));

    let other = FakeStore::holding(r#""D:\elsewhere\syntrac-app.exe" --tray"#);
    assert!(!repair(&other, Path::new(EXE), |_| true), "a copy the user chose is left alone");
    assert_eq!(other.current(), Some(r#""D:\elsewhere\syntrac-app.exe" --tray"#.to_owned()));
}

// ---- the real registry, on a throwaway key

#[test]
fn the_real_store_round_trips_on_a_throwaway_key_and_never_touches_the_run_key() {
    assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
    assert_eq!(VALUE_NAME, "SYNTRAC");

    let unique = format!(
        "Software\\Syntrac\\AutostartTest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    );
    let store = RegistryAutostart::at(&unique);
    assert_eq!(store.registered(), Ok(None), "no key yet is not an error");
    assert_eq!(store.unregister(), Ok(()), "removing from a key that is not there is not an error");

    let command = format!("\"{EXE}\" --tray");
    store.register(&command).unwrap();
    assert_eq!(store.registered(), Ok(Some(command.clone())));
    let replaced = r#""C:\other\x.exe" --tray"#;
    store.register(replaced).unwrap();
    assert_eq!(store.registered(), Ok(Some(replaced.to_owned())));

    store.unregister().unwrap();
    assert_eq!(store.registered(), Ok(None));

    // Clean up the throwaway key itself (the value is already gone).
    syntrac_app_lib::autostart_registry::delete_key_for_tests(&unique);
}
