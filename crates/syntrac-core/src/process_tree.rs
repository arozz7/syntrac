//! The process tree (docs/PHASE5-PLAN.md, A1): who started whom, from the processes alive in a window. Pure: records in, a forest out.
//!
//! A process hangs under the instance named by its `parent_id`, not under whichever process shares its parent's pid, so a reused pid cannot
//! attach a child to the wrong parent. A process whose parent is not in the list (it ended before the window, or was never recorded) is a root.
//! The tree is bounded in depth and in size; what is cut is counted, never silently dropped.

use std::collections::{HashMap, VecDeque};

use crate::{ProcessInstanceId, ProcessRecord};

/// The deepest `max_depth` that is honoured: the tree is built recursively, so it stays far from any stack limit.
const DEEPEST_ALLOWED: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeLimits {
    /// At most this many processes are shown; the rest are counted.
    pub max_nodes: usize,
    /// Roots are at depth 0; nothing deeper than this is shown.
    pub max_depth: usize,
}

impl Default for TreeLimits {
    fn default() -> Self {
        Self { max_nodes: 2000, max_depth: 16 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub process: ProcessRecord,
    /// A root whose starter is named but not in the list. A system process that was never given a parent is not "missing" one.
    pub parent_missing: bool,
    /// Children in the order they started.
    pub children: Vec<TreeNode>,
    /// Processes below this one that exist but are not shown (the tree was cut here).
    pub hidden_descendants: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessForest {
    /// Roots in the order they started.
    pub roots: Vec<TreeNode>,
    /// How many processes were given.
    pub total: usize,
    /// How many are shown.
    pub shown: usize,
}

/// The tree of the given processes, in start order, cut to `limits` (nearest the top kept first).
pub fn build_forest(records: Vec<ProcessRecord>, limits: TreeLimits) -> ProcessForest {
    let total = records.len();
    let max_depth = limits.max_depth.min(DEEPEST_ALLOWED);
    let mut order: Vec<usize> = (0..total).collect();
    order.sort_by(|&a, &b| {
        records[a].started.cmp(&records[b].started).then(records[a].pid.cmp(&records[b].pid)).then(a.cmp(&b))
    });
    // A stable order whatever the input order was: positions in `sorted` stand for the processes from here on.
    let sorted: Vec<ProcessRecord> = order.iter().map(|&i| records[i].clone()).collect();
    let by_id: HashMap<ProcessInstanceId, usize> = sorted.iter().enumerate().map(|(i, p)| (p.id, i)).collect();

    let parent = parents(&sorted, &by_id);
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); total];
    let mut roots = Vec::new();
    for (index, parent) in parent.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(index),
            None => roots.push(index),
        }
    }
    let sizes = subtree_sizes(&children, &roots);

    // Breadth first, so what is cut is the deepest and the latest, never a top level.
    let mut shown_children: Vec<Vec<usize>> = vec![Vec::new(); total];
    let mut hidden = vec![0usize; total];
    let mut kept_roots = Vec::new();
    let mut shown = 0usize;
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
    for &root in &roots {
        if shown < limits.max_nodes {
            shown += 1;
            kept_roots.push(root);
            queue.push_back((root, 0));
        }
    }
    while let Some((node, depth)) = queue.pop_front() {
        for &child in &children[node] {
            if depth < max_depth && shown < limits.max_nodes {
                shown += 1;
                shown_children[node].push(child);
                queue.push_back((child, depth + 1));
            } else {
                hidden[node] += sizes[child];
            }
        }
    }

    let nodes = Assembler { sorted: &sorted, shown_children: &shown_children, hidden: &hidden, parent: &parent };
    ProcessForest { roots: kept_roots.iter().map(|&root| nodes.node(root)).collect(), total, shown }
}

/// The index of each process's parent in `sorted`, or `None` for a root. Loops (a process that is its own ancestor) are cut at their
/// earliest-started member, so every process is reachable from a root.
fn parents(sorted: &[ProcessRecord], by_id: &HashMap<ProcessInstanceId, usize>) -> Vec<Option<usize>> {
    let mut parent: Vec<Option<usize>> = sorted
        .iter()
        .enumerate()
        .map(|(i, p)| p.parent_id.and_then(|id| by_id.get(&id).copied()).filter(|&j| j != i))
        .collect();
    // 0 = not looked at, 1 = on the path being followed, 2 = known to reach a root.
    let mut state = vec![0u8; sorted.len()];
    for start in 0..sorted.len() {
        let mut path = Vec::new();
        let mut at = Some(start);
        while let Some(index) = at {
            match state[index] {
                2 => break,
                1 => {
                    // Back on the path: a loop. The earliest started member (the lowest index, as `sorted` is in start order) becomes a root.
                    let from = path.iter().position(|&member| member == index).unwrap_or(0);
                    if let Some(&earliest) = path[from..].iter().min() {
                        parent[earliest] = None;
                    }
                    break;
                }
                _ => {
                    state[index] = 1;
                    path.push(index);
                    at = parent[index];
                }
            }
        }
        for index in path {
            state[index] = 2;
        }
    }
    parent
}

/// How many processes are in the subtree of each one, itself included. Leaves first, without recursion.
fn subtree_sizes(children: &[Vec<usize>], roots: &[usize]) -> Vec<usize> {
    let mut visit_order = Vec::with_capacity(children.len());
    let mut stack: Vec<usize> = roots.to_vec();
    while let Some(node) = stack.pop() {
        visit_order.push(node);
        stack.extend(children[node].iter().copied());
    }
    let mut sizes = vec![1usize; children.len()];
    for &node in visit_order.iter().rev() {
        let below: usize = children[node].iter().map(|&child| sizes[child]).sum();
        sizes[node] += below;
    }
    sizes
}

struct Assembler<'a> {
    sorted: &'a [ProcessRecord],
    shown_children: &'a [Vec<usize>],
    hidden: &'a [usize],
    parent: &'a [Option<usize>],
}

impl Assembler<'_> {
    /// Built recursively; the depth is bounded by `DEEPEST_ALLOWED`.
    fn node(&self, index: usize) -> TreeNode {
        let process = self.sorted[index].clone();
        let parent_missing = self.parent[index].is_none() && (process.parent_id.is_some() || process.parent_pid != 0);
        TreeNode {
            parent_missing,
            children: self.shown_children[index].iter().map(|&child| self.node(child)).collect(),
            hidden_descendants: self.hidden[index],
            process,
        }
    }
}
