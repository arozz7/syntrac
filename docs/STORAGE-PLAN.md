# Storage plan: relocate, compact, roll up

Status: **APPROVED 2026-11-13.** Implementation starts with S1.

Decisions (2026-11-13): data on a fixed internal SSD with plenty of free space (for example `D:\Syntrac`); detail retention default 14 days and **configurable** (`detail_retention_days` in `service.toml`, so it can be tuned as we learn what is worth keeping); rollups kept, with a configurable limit; no raw archives for now.

## Why

Measured on the installed collector (2026-11-12): the database grows ~52 MB/hour on this workstation, ~1.2 GB/day, ~37 GB
at the current 30-day retention. Only ~15 MB of a 55 MB database is event payload; the rest is duplication (flows are stored
twice), 36-character text ids repeated in every row and three indexes, and per-row overhead. The system drive (C:) has 27 GB
free, so the database must move, and long-term history must become much cheaper than raw detail.

## Goals

1. The database lives on a different, always-attached local drive, chosen at install time.
2. Detail (every event, flow, per-process sample) is kept for a short, configurable window.
3. Compact summaries ("rollups") are kept for much longer, so trends and baselines survive after detail is purged.
4. The collector can never fill a drive, and never deletes detail that has not yet been rolled up.

## Target layout

| Tier | What | Kept | Size (estimate, to be measured) |
|---|---|---|---|
| Detail | events, flows, per-process samples, process records | `detail_retention_days`, default 14 | roughly halved by the compact layout |
| Hourly rollups | system CPU / memory / network / disk (avg, max); per executable: hours seen, avg and peak CPU, peak memory, starts | long (default: keep) | tens of MB per year |
| Daily rollups | per (executable, destination, port): bytes in/out, flow count, first and last seen | long (default: keep) | under ~100 MB per year |
| Ledgers | first/last seen for every executable and every destination, permanently | forever | tiny |

The ledgers are what "new destination observed" and "unusual for this program" detections (Phase 3) will compare against.

## Phases

Each phase ends with `verify.ps1` passing and an `aiChangeLog/phase-S?.md` entry. Tests are written before code.

### S1: Relocatable data root (do first, C: is the pressing problem)

Tasks
1. `install-service.ps1 -DataDir <path>` already exists; extend it to refuse removable or network drives, a missing drive, and
   a path without enough free space, and to apply the same ACLs (SYSTEM and Administrators full, the installing user Modify on
   `data`, read-only on `logs`) to the new root.
2. `scripts/deploy/move-data.ps1`: stops the service, checkpoints the WAL, COPIES (never moves) the database to the new root,
   verifies it (SQLite `integrity_check`, identical row counts per table), switches the config, restarts, and verifies fresh
   writes. The old files are left in place; deleting them is a separate step you confirm.
3. The UI learns the database path from the service's status over the pipe (it already reports `database_path`), caches the
   last known path for when the service is offline, and keeps `SYNTRAC_DB` as an override.
4. Service start fails with a clear message, and keeps retrying, if the data drive is not there. It never creates an empty
   database on the wrong path.

Tests first: config and path validation (missing drive, relative path, removable drive), a move script self-test like
`config-helpers.tests.ps1`, UI path-discovery logic with a fake pipe, the smoke test extended to run against a custom root.

### S2 -> run as S3: Compact layout (migration v4)

Tasks
1. Ids stored as 16-byte blobs instead of 36-character text (events, flows, processes, per-process samples).
2. Network flows stored once: the `events` row for a flow carries no payload, the repository rebuilds it from `flows`.
3. Review the three event indexes against the queries that really run (`EXPLAIN QUERY PLAN`); drop any that are unused.
4. Per-process CPU stored as integer hundredths of a percent.

Safety: the existing repository contract suites run unchanged against the new layout (the in-memory and SQLite
implementations must still agree), the migration is rehearsed on a COPY of the real database with a before/after size and
row-count report, and it is a normal numbered migration (existing databases upgrade in place; the old file is not touched
until you accept the result).

### S3 -> run as S2: Rollups and ledgers (migration v5)

Tasks
1. Pure core module `rollup`: given detail rows for a completed hour or day, produce rollup rows. Only COMPLETED periods are
   rolled up. Re-running a period gives the same result (idempotent).
2. A watermark table records how far rolling up has got, so a restart or a crash cannot skip or double-count a period.
3. Repository trait `RollupRepository` with in-memory and SQLite implementations and a contract suite.
4. The collector loop rolls up what is due (like it samples metrics and purges today).
5. Ledgers (`first_seen` / `last_seen` for executables and destinations) updated as part of the same step.
6. Queries for trends: per-hour system curves, per-executable history, "when was this destination first seen".

Tests first: hourly and daily aggregation maths (empty periods, partial hours, a process spanning an hour boundary, pid reuse),
idempotent re-run, watermark recovery after a simulated crash, ledger first/last seen.

### S4: Retention and the storage guard

Tasks
1. Purge detail only older than `detail_retention_days` AND already rolled up (never one without the other).
2. Downsample per-process samples older than 24 hours (5-second to 1-minute) before they are purged.
3. Storage guard: a configurable cap on database size and a minimum free space on the data drive. When either is reached, the
   oldest rolled-up detail is purged early and the status banner says so; below a hard floor, detail collection pauses (rollups
   and status continue) rather than filling the drive.
4. Status and the UI show database size, free space on the data drive, oldest detail and oldest rollup, so growth is visible.

Tests first: purge refuses un-rolled data, cap and free-space behaviour on a fake volume, downsampling keeps min/avg/max.

### Later (not in this plan)

Trend and baseline views in the UI, "unusual for this program" detection (Phase 3), optional monthly archives of purged detail
to a bigger drive (Parquet/DuckDB, already in the long-term list), and daily system-state snapshots (autostart entries,
listening ports) once those are collected.

## Decisions (resolved)

1. **Data drive.** Recommended: a fixed internal SSD with plenty of free space (for example `D:\Syntrac`). Acceptable: a larger but
   slower hard disk. Not recommended: the system drive when it is nearly full, any drive with little free space, and external (USB or
   network) drives, which the move script refuses.
2. **Detail retention.** Recommended default 14 days. (7 days is smaller; 30 days is the old default.)
3. **Rollup retention.** Recommended: keep (they are small), with a configurable limit.
4. **Raw archives** of purged detail: not now (rollups only), revisit with the DuckDB phase.

## Risks

| Risk | Mitigation |
|---|---|
| Migration damages the live database | Rehearsed on a copy first; copy-then-switch move; old files kept until you confirm |
| Rollup bugs silently distort history | Pure functions with exact-value tests, idempotence, watermark; detail is never purged before it is rolled up |
| Data drive missing at boot | Clear error and retry; never creates a new empty database elsewhere |
| Compact layout hides data from older UI versions | The UI already reports a schema mismatch as its own state instead of failing |
