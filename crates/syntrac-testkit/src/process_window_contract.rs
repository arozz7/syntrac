//! Contract for listing the processes alive in a time window (`ProcessFilter::alive_between`), which the process tree is built from.
//! Every implementation (in-memory, SQLite, ...) must pass it.

use syntrac_core::{ProcessFilter, ProcessQueries, ProcessRepository};

use crate::fixtures::{at_secs, running_process};

pub fn process_window_contract<R: ProcessRepository + ProcessQueries>(make: impl Fn() -> R) {
    a_window_lists_every_process_whose_life_overlaps_it(make());
    a_window_includes_its_edges(make());
    a_window_combines_with_the_other_filters(make());
    no_window_means_no_time_limit(make());
}

/// pid 1 ended before the window, 2 ended inside it, 3 spans it, 4 starts and ends inside it, 5 is still running from before,
/// 6 starts after it.
fn fill<R: ProcessRepository>(repo: &mut R) {
    for (pid, start, end) in
        [(1, 10, Some(50)), (2, 40, Some(120)), (3, 20, Some(500)), (4, 110, Some(130)), (5, 30, None), (6, 300, None)]
    {
        let record = running_process(pid, at_secs(start));
        repo.record_start(record.clone()).unwrap();
        if let Some(end) = end {
            repo.record_exit(record.id, at_secs(end), Some(0)).unwrap();
        }
    }
}

fn pids<R: ProcessQueries>(repo: &R, filter: ProcessFilter) -> Vec<u32> {
    let mut found: Vec<u32> = repo.list_processes(&filter).unwrap().iter().map(|p| p.pid).collect();
    found.sort_unstable();
    found
}

fn between(from: i64, to: i64) -> ProcessFilter {
    ProcessFilter { alive_between: Some((at_secs(from), at_secs(to))), limit: 100, ..ProcessFilter::default() }
}

fn a_window_lists_every_process_whose_life_overlaps_it<R: ProcessRepository + ProcessQueries>(mut repo: R) {
    fill(&mut repo);
    assert_eq!(pids(&repo, between(100, 200)), vec![2, 3, 4, 5]);
}

fn a_window_includes_its_edges<R: ProcessRepository + ProcessQueries>(mut repo: R) {
    fill(&mut repo);
    // Process 1 ended exactly when the window starts; 6 starts exactly when it ends.
    assert!(pids(&repo, between(50, 60)).contains(&1));
    assert!(pids(&repo, between(250, 300)).contains(&6));
    assert!(!pids(&repo, between(51, 60)).contains(&1));
    assert!(!pids(&repo, between(250, 299)).contains(&6));
}

fn a_window_combines_with_the_other_filters<R: ProcessRepository + ProcessQueries>(mut repo: R) {
    fill(&mut repo);
    let running = ProcessFilter { running_only: true, ..between(100, 200) };
    assert_eq!(pids(&repo, running), vec![5]);
    let named = ProcessFilter { name_contains: Some("proc3".into()), ..between(100, 200) };
    assert_eq!(pids(&repo, named), vec![3]);
    let limited = ProcessFilter { limit: 2, ..between(100, 200) };
    assert_eq!(repo.list_processes(&limited).unwrap().len(), 2);
}

fn no_window_means_no_time_limit<R: ProcessRepository + ProcessQueries>(mut repo: R) {
    fill(&mut repo);
    assert_eq!(pids(&repo, ProcessFilter { limit: 100, ..ProcessFilter::default() }), vec![1, 2, 3, 4, 5, 6]);
}
