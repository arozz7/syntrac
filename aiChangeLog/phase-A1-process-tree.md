# Phase A1: the process tree (no schema change)

Part of the approved plan (Step 0 splits, then A1 process tree, A2 connection map, B question box, C installer). The service has always stored
`parent_pid` and `parent_id` for every process; nothing showed them.

## What was built
- **Core, pure:** `crates/syntrac-core/src/process_tree.rs`. `build_forest(records, TreeLimits)` returns roots and children in start order.
  A process hangs under the instance named by `parent_id` (never under whichever process shares the parent's pid, so a reused pid cannot
  adopt a child). A process whose parent is not in the list is a root, marked `parent_missing` when a parent was named. Loops (a process that
  is its own ancestor) are cut at the earliest-started member, so every process appears exactly once. The tree is cut by depth (16) and size
  (2000), breadth first, so the top levels stay whole; every cut is counted in `hidden_descendants`. Tests first: `tests/process_tree.rs` (10).
- **Reader:** `ProcessFilter::alive_between` (inclusive window; started at or before its end, not ended before its start) in the trait,
  SQLite (`started <= ? AND (ended IS NULL OR ended >= ?)`) and the in-memory store, with a shared contract
  `crates/syntrac-testkit/src/process_window_contract.rs` that both pass.
- **App:** `app/src-tauri/src/history_tree.rs` (`process_tree`, command `get_process_tree`), window 1 minute to 14 days (the detail kept).
  Each node has the name, path, start, end (approximate flagged), the existing identity badge and a command line cleaned with `clean_text` and cut
  to 300 characters. Tests first: `tests/history_tree.rs` (9).
- **Page:** History, **Process tree** tab (`views/ProcessTree.tsx`): nested list, per-node expand and collapse, Expand all / Collapse all, the top
  two levels open at first, window choice (1 hour to 14 days), a name opens the program's details below, "parent not recorded" and "N more not shown"
  notes. Command lines are rendered as text only. Tests first: `views/ProcessTree.test.tsx` (14).
- **Demo:** the fictional machine now has who-started-whom (the shell starts most programs, the service host starts the backup service, which starts its
  agent) and a few command lines (`STARTED_BY`, `COMMAND_LINES` in `world.rs`); each level starts ten minutes after its starter. The parents chosen are
  not browsers or office programs, so no rogue-software pattern fires and the findings are unchanged. Tests: `tests/demo.rs` (+2).
- **Docs and pictures:** README (new picture and row), user guide, `history-process-tree` shot in `demo-capture.ps1`.

## Files
- New: `crates/syntrac-core/src/process_tree.rs`, `crates/syntrac-core/tests/process_tree.rs`, `crates/syntrac-testkit/src/process_window_contract.rs`,
  `app/src-tauri/src/history_tree.rs`, `app/src-tauri/tests/history_tree.rs`, `app/src/api/processTreeTypes.ts`, `app/src/test/fakeTree.ts`,
  `app/src/views/ProcessTree.tsx`, `app/src/views/ProcessTree.test.tsx`.
- Changed: `ports.rs` (`alive_between`), `syntrac-store/src/processes.rs`, `syntrac-testkit/src/memory.rs`, `backend.rs`, `commands.rs`, `lib.rs`,
  `api/api.ts`, `api/types.ts`, `test/fakeApi.ts`, `views/History.tsx`, `theme/app.css`, demo `world.rs`, `generate.rs`, `demo_collector.rs`.

## Limits worth knowing
Only 14 days of per-process detail exist, so the tree cannot look further back. A parent is only linked when the service resolved it at the start;
a process that started while the service was down has no link and shows as a root.
