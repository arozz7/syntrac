//! Which program started which (docs/PATTERNS.md, F5b): the read port behind the "started by" patterns, and its plain data.
//!
//! Only parents SYNTRAC could resolve at the time are listed (a start whose parent was not known has no parent to name), and only the
//! raw process rows are read, so the history is the detail retention (14 days by default). Command lines are NOT stored by SYNTRAC, so
//! nothing here knows what a program was asked to do.

use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

/// A parent and a child program, and how often the parent started the child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPair {
    pub parent: Executable,
    pub child: Executable,
    /// How many times the parent started the child (from `since`).
    pub count: u32,
    /// The first and last of those starts.
    pub first: Timestamp,
    pub last: Timestamp,
}

pub trait SpawnRepository {
    /// One row per (parent program, child program) for children started at or after `since` whose FILE NAME (the part after the last
    /// backslash, without regard to case) is one of `child_names` (lower case). A name matches the whole file name only: `cmd.exe`
    /// does not match `notcmd.exe`, and `_` and `%` in a name mean themselves. Children whose parent is not known are left out.
    /// Any order. An empty list of names reads nothing.
    fn spawn_pairs(&self, since: Timestamp, child_names: &[String]) -> Result<Vec<SpawnPair>, RepoError>;
}
