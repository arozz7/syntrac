//! The process tree (docs/PHASE5-PLAN.md, A1): processes in, a forest of who started whom out. All pure.

use syntrac_core::process_tree::{build_forest, ProcessForest, TreeLimits, TreeNode};
use syntrac_core::{ProcessInstanceId, ProcessRecord, Timestamp};

fn at(secs: i64) -> Timestamp {
    Timestamp::from_micros(secs * 1_000_000)
}

/// A process with this pid that started at `start` seconds, started by `parent` (its pid and start), or by nothing recorded.
fn process(pid: u32, start: i64, parent: Option<(u32, i64)>) -> ProcessRecord {
    ProcessRecord {
        id: ProcessInstanceId::from_pid_and_creation(pid, at(start)),
        pid,
        parent_pid: parent.map_or(0, |(pid, _)| pid),
        parent_id: parent.map(|(pid, start)| ProcessInstanceId::from_pid_and_creation(pid, at(start))),
        image_path: format!(r"C:\apps\p{pid}.exe"),
        command_line: None,
        started: at(start),
        ended: None,
        ended_approximate: false,
        exit_code: None,
    }
}

fn pids(nodes: &[TreeNode]) -> Vec<u32> {
    nodes.iter().map(|node| node.process.pid).collect()
}

fn forest(records: Vec<ProcessRecord>) -> ProcessForest {
    build_forest(records, TreeLimits::default())
}

#[test]
fn nothing_in_gives_an_empty_forest() {
    let result = forest(vec![]);
    assert!(result.roots.is_empty());
    assert_eq!((result.total, result.shown), (0, 0));
}

#[test]
fn a_child_hangs_under_the_process_that_started_it_in_start_order() {
    let result = forest(vec![
        process(30, 300, Some((10, 100))),
        process(10, 100, None),
        process(20, 200, Some((10, 100))),
        process(21, 250, Some((20, 200))),
    ]);
    assert_eq!(pids(&result.roots), vec![10]);
    assert_eq!(pids(&result.roots[0].children), vec![20, 30]);
    assert_eq!(pids(&result.roots[0].children[0].children), vec![21]);
    assert_eq!((result.total, result.shown), (4, 4));
}

#[test]
fn a_process_whose_parent_is_not_in_the_list_is_a_root_and_says_so() {
    let result = forest(vec![process(10, 100, Some((99, 5))), process(11, 110, None)]);
    assert_eq!(pids(&result.roots), vec![10, 11]);
    assert!(result.roots[0].parent_missing, "its parent is named but not in the list");
    assert!(!result.roots[1].parent_missing, "nothing recorded as its parent, the way system processes start");
}

#[test]
fn roots_are_ordered_by_start() {
    let result = forest(vec![process(3, 300, None), process(1, 100, None), process(2, 200, None)]);
    assert_eq!(pids(&result.roots), vec![1, 2, 3]);
}

#[test]
fn the_parent_is_the_instance_named_not_whichever_process_shares_its_pid() {
    // Two processes had pid 10 at different times; the child was started by the later one.
    let result = forest(vec![process(10, 100, None), process(10, 500, None), process(20, 600, Some((10, 500)))]);
    let later = result.roots.iter().find(|node| node.process.started == at(500)).unwrap();
    let earlier = result.roots.iter().find(|node| node.process.started == at(100)).unwrap();
    assert_eq!(pids(&later.children), vec![20]);
    assert!(earlier.children.is_empty());
}

#[test]
fn a_process_that_is_its_own_parent_or_part_of_a_loop_still_shows_up_once() {
    let mut itself = process(1, 100, None);
    itself.parent_id = Some(itself.id);
    let mut a = process(2, 100, Some((3, 200)));
    let mut b = process(3, 200, Some((2, 100)));
    a.parent_pid = 3;
    b.parent_pid = 2;
    let result = forest(vec![itself, a, b]);
    assert_eq!(result.total, 3);
    assert_eq!(result.shown, 3, "every process appears exactly once");
}

#[test]
fn the_order_of_the_input_does_not_matter() {
    let records = vec![
        process(1, 100, None),
        process(2, 110, Some((1, 100))),
        process(3, 120, Some((1, 100))),
        process(4, 130, Some((2, 110))),
    ];
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(forest(records), forest(reversed));
}

#[test]
fn a_chain_deeper_than_the_limit_is_cut_and_the_cut_is_counted() {
    let mut records = vec![process(1, 101, None)];
    for pid in 2..=6 {
        records.push(process(pid, 100 + i64::from(pid), Some((pid - 1, 100 + i64::from(pid) - 1))));
    }
    let result = build_forest(records, TreeLimits { max_nodes: 100, max_depth: 2 });
    let deepest = &result.roots[0].children[0].children[0];
    assert_eq!(deepest.process.pid, 3);
    assert!(deepest.children.is_empty());
    assert_eq!(deepest.hidden_descendants, 3, "pids 4, 5 and 6 are not shown");
    assert_eq!((result.total, result.shown), (6, 3));
}

#[test]
fn too_many_processes_are_cut_nearest_the_top_first() {
    // The root started a (with five children of its own), b and c.
    let mut records = vec![process(1, 100, None), process(2, 110, Some((1, 100))), process(3, 120, Some((1, 100)))];
    records.push(process(4, 130, Some((1, 100))));
    for pid in 10..15 {
        records.push(process(pid, 200 + i64::from(pid), Some((2, 110))));
    }
    let result = build_forest(records, TreeLimits { max_nodes: 4, max_depth: 16 });
    assert_eq!(result.shown, 4);
    assert_eq!(result.total, 9);
    assert_eq!(pids(&result.roots[0].children), vec![2, 3, 4], "the top levels are kept whole");
    assert_eq!(result.roots[0].children[0].hidden_descendants, 5);
    assert_eq!(result.roots[0].hidden_descendants, 0, "what was hidden is counted at the last shown parent");
}

#[test]
fn a_large_list_is_handled_quickly() {
    let mut records = vec![process(1, 1, None)];
    for pid in 2..=60_000u32 {
        records.push(process(pid, i64::from(pid), Some((pid / 2, i64::from(pid / 2)))));
    }
    let begun = std::time::Instant::now();
    let result = build_forest(records, TreeLimits { max_nodes: 2000, max_depth: 16 });
    assert_eq!(result.total, 60_000);
    assert_eq!(result.shown, 2000);
    assert!(begun.elapsed() < std::time::Duration::from_secs(2), "{:?}", begun.elapsed());
}
