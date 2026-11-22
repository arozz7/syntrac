//! The saved answers of the AI help (docs/PHASE4-PLAN.md, L4i-a): what is kept, replaced and forgotten, the file it lives in, and the limit.

use std::path::PathBuf;

use syntrac_app_lib::ai_investigate::SubjectRef;
use syntrac_app_lib::investigation_store::{InvestigationStore, JsonInvestigationStore, SavedInvestigation};

fn program(path: &str) -> SubjectRef {
    SubjectRef::Program { path: path.to_owned() }
}

fn finding(id: i64) -> SubjectRef {
    SubjectRef::Finding { id }
}

fn saved(subject: SubjectRef, answer: &str, saved_ms: i64) -> SavedInvestigation {
    SavedInvestigation {
        subject,
        answer: answer.to_owned(),
        truncated: false,
        model: "some-model".to_owned(),
        level: "summary".to_owned(),
        sent: "<evidence>{}</evidence>".to_owned(),
        saved_ms,
    }
}

/// A folder of its own for one test, removed first so nothing is left over from an earlier run.
fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("syntrac-store-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn what_was_saved_is_given_back_whole() {
    let store = JsonInvestigationStore::in_memory();
    let mut entry = saved(program(r"C:\apps\widget.exe"), "It is a sync tool.", 1_000);
    entry.truncated = true;
    store.put(entry.clone());

    assert_eq!(store.get(&program(r"C:\apps\widget.exe")), Some(entry));
    assert_eq!(store.get(&program(r"C:\apps\other.exe")), None);
    assert_eq!(store.get(&finding(1)), None);
}

#[test]
fn a_program_is_found_whatever_the_letter_case_of_its_path() {
    let store = JsonInvestigationStore::in_memory();
    store.put(saved(program(r"C:\Apps\Widget.exe"), "first", 1));
    assert_eq!(store.get(&program(r"c:\apps\widget.EXE")).unwrap().answer, "first");
}

#[test]
fn saving_again_replaces_the_earlier_answer_for_the_same_subject() {
    let store = JsonInvestigationStore::in_memory();
    store.put(saved(program(r"C:\apps\widget.exe"), "old", 1));
    store.put(saved(program(r"c:\APPS\widget.exe"), "new", 2));
    store.put(saved(finding(7), "about a finding", 3));

    assert_eq!(store.get(&program(r"C:\apps\widget.exe")).unwrap().answer, "new");
    assert_eq!(store.list().len(), 2, "one program and one finding, not three");
}

#[test]
fn a_program_and_a_finding_never_share_an_answer() {
    let store = JsonInvestigationStore::in_memory();
    store.put(saved(finding(7), "finding seven", 1));
    store.put(saved(finding(8), "finding eight", 2));
    assert_eq!(store.get(&finding(7)).unwrap().answer, "finding seven");
    assert_eq!(store.get(&finding(8)).unwrap().answer, "finding eight");
}

#[test]
fn forgetting_removes_only_that_answer() {
    let store = JsonInvestigationStore::in_memory();
    store.put(saved(program(r"C:\a.exe"), "a", 1));
    store.put(saved(program(r"C:\b.exe"), "b", 2));

    store.forget(&program(r"C:\A.exe"));

    assert_eq!(store.get(&program(r"C:\a.exe")), None);
    assert!(store.get(&program(r"C:\b.exe")).is_some());
    store.forget(&program(r"C:\never-saved.exe")); // nothing to forget is not an error
}

#[test]
fn the_list_names_every_subject_with_its_date_and_no_answers() {
    let store = JsonInvestigationStore::in_memory();
    store.put(saved(program(r"C:\a.exe"), "a long answer", 10));
    store.put(saved(finding(3), "b", 20));

    let marks = store.list();

    assert_eq!(marks.len(), 2);
    assert!(marks.iter().any(|mark| mark.subject == program(r"C:\a.exe") && mark.saved_ms == 10));
    assert!(marks.iter().any(|mark| mark.subject == finding(3) && mark.saved_ms == 20));
    assert!(!serde_json::to_string(&marks).unwrap().contains("long answer"));
}

#[test]
fn only_the_newest_answers_are_kept_when_there_are_too_many() {
    let store = JsonInvestigationStore::in_memory().with_limit(3);
    for number in 1..=5 {
        store.put(saved(finding(number), "x", number * 100));
    }

    let kept: Vec<i64> = {
        let mut times: Vec<i64> = store.list().iter().map(|mark| mark.saved_ms).collect();
        times.sort_unstable();
        times
    };
    assert_eq!(kept, vec![300, 400, 500]);
    assert_eq!(store.get(&finding(1)), None);
}

#[test]
fn answers_survive_a_restart_through_the_file() {
    let folder = scratch("restart");
    let file = folder.join("investigations.json");
    let first = JsonInvestigationStore::with_file(file.clone());
    first.put(saved(program(r"C:\apps\widget.exe"), "kept on disk", 5));
    first.put(saved(finding(9), "also kept", 6));
    first.forget(&finding(9));

    let second = JsonInvestigationStore::with_file(file);

    assert_eq!(second.get(&program(r"C:\apps\widget.exe")).unwrap().answer, "kept on disk");
    assert_eq!(second.get(&finding(9)), None, "a forgotten answer is gone from the file too");
}

#[test]
fn a_missing_file_means_nothing_saved_and_the_folder_is_made_when_saving() {
    let folder = scratch("missing");
    let file = folder.join("deeper").join("investigations.json");
    let store = JsonInvestigationStore::with_file(file.clone());
    assert!(store.list().is_empty());

    store.put(saved(finding(1), "x", 1));

    assert!(file.exists());
}

#[test]
fn a_damaged_file_is_treated_as_empty_and_does_not_stop_saving() {
    let folder = scratch("damaged");
    let file = folder.join("investigations.json");
    std::fs::write(&file, "{ this is not json").unwrap();

    let store = JsonInvestigationStore::with_file(file.clone());
    assert!(store.list().is_empty());
    store.put(saved(finding(1), "fresh", 1));

    assert_eq!(JsonInvestigationStore::with_file(file).get(&finding(1)).unwrap().answer, "fresh");
}

#[test]
fn a_file_with_a_byte_order_mark_is_still_read() {
    let folder = scratch("bom");
    let file = folder.join("investigations.json");
    let written = JsonInvestigationStore::with_file(file.clone());
    written.put(saved(finding(1), "with a mark", 1));
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("\u{feff}{text}")).unwrap();

    assert_eq!(JsonInvestigationStore::with_file(file).get(&finding(1)).unwrap().answer, "with a mark");
}

#[test]
fn a_file_that_cannot_be_written_keeps_the_answers_for_this_run() {
    let folder = scratch("unwritable");
    let blocker = folder.join("not-a-folder");
    std::fs::write(&blocker, "a file where a folder is needed").unwrap();
    let store = JsonInvestigationStore::with_file(blocker.join("investigations.json"));

    store.put(saved(finding(1), "still here", 1));

    assert_eq!(store.get(&finding(1)).unwrap().answer, "still here");
}
