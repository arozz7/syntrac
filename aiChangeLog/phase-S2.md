# Phase S2: compact layout (schema v4)

Implements S2 of `docs/STORAGE-PLAN.md` (it runs before S3, rollups). Status: implemented and rehearsed on a copy of the real
database; **not yet applied to the live database** (that happens when the upgraded service is installed).

## What changed

**Migration v4** (`crates/syntrac-store/src/migrations.rs`), one transaction, tables rebuilt:
- Ids (`events.id`, `process_instance_id` columns, `processes.id/parent_id`, `process_metrics.instance`) are 16-byte blobs
  instead of 36-character text, converted in SQL with `unhex(replace(id, '-', ''))`.
- `events` has `seq INTEGER PRIMARY KEY` (the old rowid, so order is unchanged) and a nullable `payload`.
- A network flow is stored once: its `events` row has a NULL payload (only when its `flows` row exists, so no event can lose
  content) and `flows.seq` is the same number as its event's `seq`. Reading an event with a NULL payload rebuilds the flow
  from `flows`.
- Indexes no query plan uses are dropped: `idx_flows_process`, `idx_flows_remote`, `idx_process_metrics_instance_ts`. The three
  `events` indexes stay (the event query API filters by time, category and process; nothing in production calls it yet).
- Decided not to do: "per-process CPU as integer hundredths" (about 1.6 MB of 334 MB, and it changes stored precision).

**Around the migration** (`connection.rs`): before the first upgrade of an existing database, a consistent backup is written
next to it (`syntrac.db.v3-backup`, then `-2`, `-3`, never overwriting; if it cannot be written the upgrade does not run). After
the upgrade the store runs `VACUUM` and truncates the WAL, otherwise the file would not shrink.

**Repositories** (`events.rs`, `processes.rs`, `metrics.rs`, `codec.rs`): blob ids (`IdBytes`); event reads `LEFT JOIN flows`;
`reconcile_running` passes hex ids and decodes them with `unhex`; retention deletes a purged flow event's `flows` row first.

## Tests (written first)
- Contract suites (run against the in-memory AND SQLite implementations): a parent link round trips; a metrics sample without
  an owner is kept and listed; purging removes a flow event together with its flow.
- `migration_v4_tests.rs` (11 tests): builds a real v3 database with text ids, upgrades it, and checks every event unchanged
  (including a flow rebuilt from its row and a flow event that had no flows row), ids are 16-byte blobs everywhere, parent
  links, `resolve`, the alive list of `reconcile_running`, flow queries, metrics with and without an owner, dropped and kept
  indexes, the backup exists and equals the old database, an existing backup is never overwritten, no backup for a new or current
  database, and the file shrinks.

## Rehearsal on a copy of the real database
Tool: `cargo run --release -p syntrac-store --example rehearse_migration -- <source.db> <work-dir>` (the source is only read).
It also compares every event and process value by value through the repositories, fingerprints the flow and metrics tables, and
prints `EXPLAIN QUERY PLAN` for the queries the application runs. Output is kept in `aiChangeLog/s2-rehearsal-output.txt`.
Helper: `examples/table_sizes.rs` (per table and index sizes).

Final run on a copy of the live database (2026-11-13, 457,218 events, 300,740 flows, 276,925 samples, 68,489 processes):

| | before | after |
|---|---|---|
| File size | 337.9 MB | **151.1 MB (45%)** |
| Upgrade time (backup, rewrite, VACUUM) | | 11.6 s |
| Integrity check | ok | ok |
| Events, flows, samples, processes | | identical row counts |

- Every one of the 457,218 events and 68,489 processes was read back through the repositories and compared value by value:
  identical, in order (flows rebuilt from their `flows` rows included). Flow, sample and process fingerprints (sums of bytes,
  ports, pids, windows, cpu, memory; distinct owners and addresses) are unchanged.
- Flow events without a flows row: 0. Flows rows without an event: 0.
- Query plans: `top_destinations`, `recent_flows` and `cpu_breakdown` use `idx_flows_window` / `idx_process_metrics_ts` and
  need nothing that was dropped; `idx_flows_remote` appeared in no plan (before or after).
- Estimated steady growth after S2: about 45% of the earlier ~52 MB/hour, so roughly 23 MB/hour; 14 days of detail would be
  around 8 GB before rollups and the downsampling of S4 (an estimate, to be measured on the live service).

## Operating notes
- The service reports Running before it opens the database, so the upgrade (about 12 s here, plus the backup) cannot trip the
  Service Control Manager's start timeout; the pipe answers once the upgrade is done.
- The backup is about the size of the old database and stays until you delete it.
- Service and UI must be upgraded together; an older UI shows a schema-mismatch state.
- Building with the UI running: the running `syntrac-app.exe` locks `target/debug`, so `verify.ps1` was run with
  `CARGO_TARGET_DIR` on another drive. Close the app before a normal run.
