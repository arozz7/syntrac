//! Contract suite for "which program started which" (F5b): every `SpawnRepository` (in-memory, SQLite) must pass these.

use syntrac_core::{ProcessRecord, ProcessRepository, SpawnPair, SpawnRepository};

use crate::fixtures::running_process;
use crate::rollup_contract::t;

pub trait SpawnSubject: SpawnRepository + ProcessRepository {}
impl<T> SpawnSubject for T where T: SpawnRepository + ProcessRepository {}

const PARENT: &str = r"C:\Apps\Parent.exe";
const OTHER_PARENT: &str = r"C:\Apps\Other.exe";
const CMD: &str = r"C:\Windows\System32\cmd.exe";

pub fn spawn_contract<R: SpawnSubject>(make: impl Fn() -> R) {
    an_empty_database_has_no_pairs(make());
    children_are_grouped_by_parent_and_child_with_count_and_first_and_last_start(make());
    only_children_started_at_or_after_the_time_count(make());
    a_name_matches_the_whole_file_name_without_regard_to_case_and_means_itself(make());
    a_child_whose_parent_is_not_known_is_left_out(make());
    no_names_read_nothing(make());
}

fn start<R: SpawnSubject>(
    repo: &mut R,
    pid: u32,
    started: syntrac_core::Timestamp,
    exe: &str,
    parent: Option<&ProcessRecord>,
) -> ProcessRecord {
    let mut record = running_process(pid, started);
    record.image_path = exe.to_owned();
    record.parent_id = parent.map(|p| p.id);
    repo.record_start(record.clone()).unwrap();
    record
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

/// Sorted plain rows: (parent path, child path, count, first, last).
fn plain(pairs: Vec<SpawnPair>) -> Vec<(String, String, u32, i64, i64)> {
    let mut rows: Vec<_> = pairs
        .into_iter()
        .map(|p| (p.parent.path, p.child.path, p.count, p.first.as_micros(), p.last.as_micros()))
        .collect();
    rows.sort();
    rows
}

fn an_empty_database_has_no_pairs<R: SpawnSubject>(repo: R) {
    assert!(repo.spawn_pairs(t(0, 0), &names(&["cmd.exe"])).unwrap().is_empty());
}

fn children_are_grouped_by_parent_and_child_with_count_and_first_and_last_start<R: SpawnSubject>(mut repo: R) {
    let parent = start(&mut repo, 10, t(0, 0), PARENT, None);
    let other = start(&mut repo, 11, t(0, 0), OTHER_PARENT, None);
    start(&mut repo, 20, t(1, 0), CMD, Some(&parent));
    start(&mut repo, 21, t(2, 0), CMD, Some(&parent));
    start(&mut repo, 22, t(3, 0), CMD, Some(&other));

    let rows = plain(repo.spawn_pairs(t(0, 0), &names(&["cmd.exe"])).unwrap());

    assert_eq!(
        rows,
        vec![
            (OTHER_PARENT.to_owned(), CMD.to_owned(), 1, t(3, 0).as_micros(), t(3, 0).as_micros()),
            (PARENT.to_owned(), CMD.to_owned(), 2, t(1, 0).as_micros(), t(2, 0).as_micros()),
        ]
    );
}

fn only_children_started_at_or_after_the_time_count<R: SpawnSubject>(mut repo: R) {
    let parent = start(&mut repo, 10, t(0, 0), PARENT, None);
    start(&mut repo, 20, t(1, 0), CMD, Some(&parent));
    start(&mut repo, 21, t(5, 0), CMD, Some(&parent));

    let rows = plain(repo.spawn_pairs(t(2, 0), &names(&["cmd.exe"])).unwrap());

    assert_eq!(rows, vec![(PARENT.to_owned(), CMD.to_owned(), 1, t(5, 0).as_micros(), t(5, 0).as_micros())]);
    assert!(repo.spawn_pairs(t(6, 0), &names(&["cmd.exe"])).unwrap().is_empty());
}

fn a_name_matches_the_whole_file_name_without_regard_to_case_and_means_itself<R: SpawnSubject>(mut repo: R) {
    let parent = start(&mut repo, 10, t(0, 0), PARENT, None);
    start(&mut repo, 20, t(1, 0), r"C:\Apps\CMD.EXE", Some(&parent));
    start(&mut repo, 21, t(1, 0), r"C:\Apps\notcmd.exe", Some(&parent));
    start(&mut repo, 22, t(1, 0), r"C:\Apps\my_tool.exe", Some(&parent));
    start(&mut repo, 23, t(1, 0), r"C:\Apps\myXtool.exe", Some(&parent));
    start(&mut repo, 24, t(1, 0), r"C:\Apps\100%.exe", Some(&parent));
    start(&mut repo, 25, t(1, 0), r"C:\Apps\100abc.exe", Some(&parent));

    let cmd = plain(repo.spawn_pairs(t(0, 0), &names(&["cmd.exe"])).unwrap());
    let underscore = plain(repo.spawn_pairs(t(0, 0), &names(&["my_tool.exe"])).unwrap());
    let percent = plain(repo.spawn_pairs(t(0, 0), &names(&["100%.exe"])).unwrap());

    assert_eq!(cmd.len(), 1, "CMD.EXE matches cmd.exe, notcmd.exe does not");
    assert_eq!(cmd[0].1, r"C:\Apps\CMD.EXE");
    assert_eq!(underscore.len(), 1, "an underscore is not a wildcard");
    assert_eq!(underscore[0].1, r"C:\Apps\my_tool.exe");
    assert_eq!(percent.len(), 1, "a percent sign is not a wildcard");
    assert_eq!(percent[0].1, r"C:\Apps\100%.exe");
}

fn a_child_whose_parent_is_not_known_is_left_out<R: SpawnSubject>(mut repo: R) {
    let parent = start(&mut repo, 10, t(0, 0), PARENT, None);
    start(&mut repo, 20, t(1, 0), CMD, None);
    start(&mut repo, 21, t(1, 0), CMD, Some(&parent));

    let rows = plain(repo.spawn_pairs(t(0, 0), &names(&["cmd.exe"])).unwrap());

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, 1);
}

fn no_names_read_nothing<R: SpawnSubject>(mut repo: R) {
    let parent = start(&mut repo, 10, t(0, 0), PARENT, None);
    start(&mut repo, 20, t(1, 0), CMD, Some(&parent));

    assert!(repo.spawn_pairs(t(0, 0), &[]).unwrap().is_empty());
}
