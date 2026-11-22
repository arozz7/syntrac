//! The process tree view (docs/PHASE5-PLAN.md, A1): who started whom over a window, as the History screen shows it.

use syntrac_app_lib::history_tree::{
    process_tree, ProcessNodeView, ProcessTreeView, FETCH_LIMIT, MAX_COMMAND_LINE_CHARS,
};
use syntrac_core::{ProcessRecord, ProcessRepository, Timestamp};
use syntrac_testkit::fixtures::{at_secs, running_process};
use syntrac_testkit::InMemoryRepository;

const HOUR: i64 = 3600;
const BASE: i64 = 1_794_571_200;

fn now() -> Timestamp {
    at_secs(BASE + 10 * HOUR)
}

fn start(
    repo: &mut InMemoryRepository,
    pid: u32,
    name: &str,
    started: i64,
    parent: Option<&ProcessRecord>,
) -> ProcessRecord {
    let mut record = running_process(pid, at_secs(started));
    record.image_path = format!(r"C:\apps\{name}.exe");
    if let Some(parent) = parent {
        record.parent_pid = parent.pid;
        record.parent_id = Some(parent.id);
    }
    repo.record_start(record.clone()).unwrap();
    record
}

fn end(repo: &mut InMemoryRepository, record: &ProcessRecord, at: i64) {
    repo.record_exit(record.id, at_secs(at), Some(0)).unwrap();
}

fn tree(repo: &InMemoryRepository, window_secs: u64) -> ProcessTreeView {
    process_tree(repo, now(), window_secs).unwrap()
}

fn names(nodes: &[ProcessNodeView]) -> Vec<&str> {
    nodes.iter().map(|node| node.name.as_str()).collect()
}

#[test]
fn processes_hang_under_the_program_that_started_them() {
    let mut repo = InMemoryRepository::default();
    let shell = start(&mut repo, 10, "explorer", BASE + HOUR, None);
    let editor = start(&mut repo, 11, "editor", BASE + 2 * HOUR, Some(&shell));
    start(&mut repo, 12, "compiler", BASE + 3 * HOUR, Some(&editor));

    let view = tree(&repo, 24 * 3600);

    assert_eq!(names(&view.roots), ["explorer.exe"]);
    assert_eq!(names(&view.roots[0].children), ["editor.exe"]);
    assert_eq!(names(&view.roots[0].children[0].children), ["compiler.exe"]);
    assert_eq!((view.total, view.shown), (3, 3));
}

#[test]
fn a_node_carries_what_the_screen_shows() {
    let mut repo = InMemoryRepository::default();
    let mut record = running_process(10, at_secs(BASE + HOUR));
    record.image_path = r"C:\apps\tool.exe".into();
    record.command_line = Some(r"tool.exe --flag".into());
    repo.record_start(record.clone()).unwrap();
    end(&mut repo, &record, BASE + 2 * HOUR);

    let node = &tree(&repo, 24 * 3600).roots[0];

    assert_eq!((node.pid, node.name.as_str(), node.image_path.as_str()), (10, "tool.exe", r"C:\apps\tool.exe"));
    assert_eq!(node.command_line.as_deref(), Some("tool.exe --flag"));
    assert_eq!((node.started_ms, node.ended_ms), ((BASE + HOUR) * 1000, Some((BASE + 2 * HOUR) * 1000)));
    assert!(!node.running);
    assert_eq!(node.identity.state, "pending", "the file has not been read yet");
}

#[test]
fn a_process_still_running_says_so() {
    let mut repo = InMemoryRepository::default();
    start(&mut repo, 10, "tool", BASE + HOUR, None);
    let node = &tree(&repo, 24 * 3600).roots[0];
    assert!(node.running);
    assert_eq!(node.ended_ms, None);
}

#[test]
fn only_processes_alive_in_the_window_are_shown() {
    let mut repo = InMemoryRepository::default();
    let old = start(&mut repo, 10, "old", BASE, None);
    end(&mut repo, &old, BASE + HOUR);
    start(&mut repo, 11, "recent", BASE + 9 * HOUR, None);

    let view = tree(&repo, 2 * 3600);

    assert_eq!(names(&view.roots), ["recent.exe"]);
    assert_eq!(view.to_ms - view.from_ms, 2 * 3600 * 1000);
}

#[test]
fn a_child_whose_parent_left_the_window_is_a_root_that_says_so() {
    let mut repo = InMemoryRepository::default();
    let parent = start(&mut repo, 10, "launcher", BASE, None);
    end(&mut repo, &parent, BASE + HOUR);
    start(&mut repo, 11, "worker", BASE + 2 * HOUR, Some(&parent));

    let view = tree(&repo, 4 * 3600);

    assert_eq!(names(&view.roots), ["worker.exe"]);
    assert!(view.roots[0].parent_missing);
}

#[test]
fn a_command_line_is_cleaned_and_cut() {
    let mut repo = InMemoryRepository::default();
    let mut record = running_process(10, at_secs(BASE + HOUR));
    record.command_line = Some(format!("tool.exe \u{202E}evil\u{0007} {}", "x".repeat(2 * MAX_COMMAND_LINE_CHARS)));
    repo.record_start(record).unwrap();

    let line = tree(&repo, 24 * 3600).roots[0].command_line.clone().unwrap();

    assert!(!line.contains('\u{202E}') && !line.contains('\u{0007}'));
    assert_eq!(line.chars().count(), MAX_COMMAND_LINE_CHARS);
}

#[test]
fn a_blank_command_line_is_no_command_line() {
    let mut repo = InMemoryRepository::default();
    let mut record = running_process(10, at_secs(BASE + HOUR));
    record.command_line = Some("   ".into());
    repo.record_start(record).unwrap();
    assert_eq!(tree(&repo, 24 * 3600).roots[0].command_line, None);
}

#[test]
fn the_window_is_kept_within_the_days_of_detail_that_are_stored() {
    let repo = InMemoryRepository::default();
    let view = tree(&repo, 365 * 24 * 3600);
    assert_eq!(view.to_ms - view.from_ms, 14 * 24 * 3600 * 1000);
}

#[test]
fn an_empty_store_gives_an_empty_tree() {
    let view = tree(&InMemoryRepository::default(), 3600);
    assert!(view.roots.is_empty());
    assert_eq!((view.total, view.shown), (0, 0));
}

#[test]
fn a_window_with_more_processes_than_can_be_read_says_only_the_newest_were() {
    let mut repo = InMemoryRepository::default();
    for pid in 1..=(FETCH_LIMIT as u32 + 5) {
        start(&mut repo, pid, "busy", BASE + HOUR + i64::from(pid), None);
    }

    let view = tree(&repo, 24 * 3600);

    assert!(view.truncated, "the oldest five were not read");
    assert_eq!(view.read, FETCH_LIMIT);
    let fewer = tree(&InMemoryRepository::default(), 3600);
    assert!(!fewer.truncated);
}
