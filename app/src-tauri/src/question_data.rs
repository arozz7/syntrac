//! Phase 5, slice B3 (docs/PHASE5-PLAN.md): the facts behind the question box's tools, read through the same read-only views the screens use.
//! Generic over the repositories, so they run against the in-memory store in tests; `Backend` calls them inside one read-only database
//! session (`question_data` in `backend.rs`). Nothing here writes.

use syntrac_core::investigate::{Evidence, FindingFacts};
use syntrac_core::storage_guard::StorageStats;
use syntrac_core::{
    ContextRepository, FindingRepository, FindingStatus, IdentityRepository, InstallRepository, KnownProductRepository,
    LauncherRepository, ProcessQueries, RepoError, RollupRepository, Timestamp, TrendRepository,
};

use crate::history_programs::{program_ranking, Grouping, ProgramMetricParam};
use crate::history_tree::{process_tree, ProcessNodeView};
use crate::investigate_evidence::{finding_facts, program_evidence, EvidenceSource};
use crate::question_tools::{CountedName, ProgramFamily, TopProgram};

const DAY_SECS: u64 = 24 * 3600;
const MB: u64 = 1024 * 1024;
/// How far back who-started-whom is read (the days of process detail that are kept).
const FAMILY_WINDOW_SECS: u64 = 14 * DAY_SECS;
const MAX_COMMAND_LINES: usize = 3;

/// The evidence for a program the store has recorded, or `None` for a path it has never seen (so a model cannot make the app describe a
/// file it invented, and nothing is ever read from the disk).
pub fn recorded_program<R: EvidenceSource>(
    repo: &R,
    now: Timestamp,
    path: &str,
    user_name: Option<String>,
) -> Result<Option<Evidence>, RepoError> {
    if repo.executable_ledger(path)?.is_none() {
        return Ok(None);
    }
    program_evidence(repo, now, path, user_name).map(Some)
}

/// The open findings, most recent activity first.
pub fn open_findings<R: FindingRepository>(repo: &R, limit: usize) -> Result<Vec<FindingFacts>, RepoError> {
    Ok(repo.findings(Some(FindingStatus::Open), limit)?.iter().map(finding_facts).collect())
}

/// The ranking the Programs screen shows, as plain rows. The bucket of processes whose program is unknown has no path and is left out.
pub fn ranked_programs<R>(
    repo: &R,
    now: Timestamp,
    metric: ProgramMetricParam,
    days: u64,
    limit: usize,
) -> Result<Vec<TopProgram>, RepoError>
where
    R: RollupRepository
        + TrendRepository
        + StorageStats
        + IdentityRepository
        + ContextRepository
        + LauncherRepository
        + InstallRepository
        + KnownProductRepository,
{
    let view = program_ranking(repo, now, days * DAY_SECS, metric, limit, Grouping::None)?;
    Ok(view
        .programs
        .into_iter()
        .filter_map(|row| {
            Some(TopProgram {
                name: row.name,
                path: row.path?,
                location: row.identity.location,
                cpu_avg_percent: row.cpu_avg_percent,
                memory_peak_mb: row.memory_peak_bytes / MB,
                bytes_sent: row.bytes_sent,
                bytes_received: row.bytes_received,
                starts: row.starts,
            })
        })
        .collect())
}

/// Who started a program and what it started, from the process tree of the days of detail that are kept. `None` when no process of that
/// program is in it.
pub fn program_family<R>(repo: &R, now: Timestamp, path: &str) -> Result<Option<ProgramFamily>, RepoError>
where
    R: ProcessQueries
        + IdentityRepository
        + ContextRepository
        + LauncherRepository
        + InstallRepository
        + KnownProductRepository,
{
    let forest = process_tree(repo, now, FAMILY_WINDOW_SECS)?;
    let mut tally = FamilyTally::default();
    for root in &forest.roots {
        tally.visit(root, None, path);
    }
    Ok((tally.instances > 0).then(|| tally.finish()))
}

#[derive(Default)]
struct FamilyTally {
    instances: usize,
    started_by: Vec<CountedName>,
    started: Vec<CountedName>,
    command_lines: Vec<String>,
}

impl FamilyTally {
    fn visit(&mut self, node: &ProcessNodeView, parent: Option<&ProcessNodeView>, path: &str) {
        if node.image_path.eq_ignore_ascii_case(path) {
            self.instances += 1;
            if let Some(parent) = parent {
                count(&mut self.started_by, &parent.name);
            }
            for child in &node.children {
                count(&mut self.started, &child.name);
            }
            if let Some(line) = &node.command_line {
                if self.command_lines.len() < MAX_COMMAND_LINES && !self.command_lines.contains(line) {
                    self.command_lines.push(line.clone());
                }
            }
        }
        for child in &node.children {
            self.visit(child, Some(node), path);
        }
    }

    fn finish(mut self) -> ProgramFamily {
        for list in [&mut self.started_by, &mut self.started] {
            list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        }
        ProgramFamily {
            instances: self.instances,
            started_by: self.started_by,
            started: self.started,
            command_lines: self.command_lines,
        }
    }
}

fn count(list: &mut Vec<CountedName>, name: &str) {
    match list.iter_mut().find(|item| item.name == name) {
        Some(item) => item.count += 1,
        None => list.push(CountedName { name: name.to_owned(), count: 1 }),
    }
}
