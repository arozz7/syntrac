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

### S2: Compact layout (migration v4). Runs BEFORE S3 (rollups are built on the compact tables)

Tasks
1. Ids stored as 16-byte blobs instead of 36-character text (events, flows, processes, per-process samples).
2. Network flows stored once: the `events` row for a flow carries no payload (`payload` is NULL), the repository rebuilds it
   from `flows`.
3. Review the indexes against the queries that really run (`EXPLAIN QUERY PLAN`); drop any that no query uses.
4. ~~Per-process CPU stored as integer hundredths of a percent.~~ **Dropped 2026-11-13**: SQLite already stores whole-number
   REALs compactly, the saving is about 1.6 MB of 334 MB (0.5%), and it would change stored precision for no real gain.

Design (added when S2 started; these are new points beyond the approved plan, recorded here before coding):
- **`events`** becomes `seq INTEGER PRIMARY KEY, id BLOB NOT NULL UNIQUE, ts, category, kind, process_instance_id BLOB,
  severity, payload TEXT NULL`. The old implicit rowid is copied into `seq`, so order (`ts, seq`) is unchanged. The unique
  index on `id` is kept (a replayed batch is still rejected).
- **`flows`** becomes `seq INTEGER PRIMARY KEY` (the SAME number as its event's `seq`: no second id and no second unique
  index), the window, pid, protocol, address, port, byte and flag columns, and `process_instance_id BLOB` (kept so flow
  queries and the S3 rollups need no join). Reading an event with a NULL payload joins `flows` on `seq`.
- **The migration strips a flow event's payload only when its `flows` row exists**; otherwise the payload is kept, so no event
  is ever lost. The rehearsal reports how many flow events have no flows row (expected 0).
- **Retention** deletes flow events and their flows rows together, keyed by the event's `ts` (equal to the flow's
  `window_start`), so a flow event can never be left without its flows row.
- **Indexes dropped** (no query uses them; each can be recreated by the migration that adds the query that needs it):
  `idx_flows_process`, `idx_flows_remote`, `idx_process_metrics_instance_ts` (the rehearsal's `EXPLAIN QUERY PLAN` showed them
  unused). The three `events` indexes stay: the event query API filters by time, category and process, and nothing in
  production calls it yet, so there is no evidence to drop any.
- **Backup before migrating:** before applying v4 to an existing database the store writes a consistent copy next to it
  (`VACUUM INTO` -> `syntrac.db.v3-backup`, a numbered name if that exists). If the backup cannot be written the migration
  does not run. The backup is never deleted by SYNTRAC; you delete it once you accept the result.
- **Compaction:** a rebuild leaves free pages, so the file would not shrink. After v4 the store runs `VACUUM` (outside the
  migration transaction) and truncates the WAL. Needs about the database's size in temporary space.
- **Ship together:** the service and the UI must be upgraded together; an old UI reports a schema mismatch, which is the
  designed behaviour.

Safety: the existing repository contract suites run unchanged against the new layout (the in-memory and SQLite
implementations must still agree), plus new cases (a parent link, a sample without an owner, flows surviving retention with
their events). A migration test builds a real v3 database with text ids and checks every value through the repositories. The
migration is rehearsed on a COPY of the real database (on O:, outside the live folder) with before/after sizes (before and
after VACUUM), per-table row counts, timing, a sample of rows compared value by value, and the query plans.

### S3: Rollups and ledgers (migration v5). Runs AFTER S2 (S2 is done and live; S3 is implemented, not yet live)

Status: **APPROVED 2026-11-13** (user chose: no port in the key with `top_port` and `distinct_ports`; hourly for everything, UTC). A rollup key cannot be
rebuilt once detail has aged out, which is why it was the user's call.

Tasks
1. Pure core module `rollup`: given detail rows for a completed hour, produce rollup rows. ALL aggregation maths lives here,
   so the in-memory and SQLite repositories cannot disagree. Only COMPLETED hours are rolled up. Re-running gives the same
   result (idempotent).
2. A watermark records how far rolling up has got, so a restart or a crash cannot skip or double-count an hour.
3. Repository trait `RollupRepository` (load raw rows for one hour; store the result and advance the watermark in ONE
   transaction; read history) with in-memory and SQLite implementations and a contract suite.
4. The collector loop rolls up what is due, a few hours per pass so a backlog never stalls draining ETW.
5. Ledgers (`first_seen` / `last_seen` for executables, destinations, and each executable-destination pair), permanent.
6. Queries for trends: per-hour system curves, per-executable history, "when was this destination first seen".

Findings that shaped the design (measured 2026-11-13 on 15.7 hours of real data, 300,740 flows):
- Keying traffic by (executable, destination, **port**) is far too big: 89,103 rows for one day, about 1.6 GB a year, against
  the 100 MB a year this plan promised. The cause is ephemeral ports (svchost: 182 addresses but 62,754 address-port pairs;
  loopback traffic of one Intel service uses 11,404 different ports). Without the port the same day is 1,393 rows.
- Keyed by (hour, executable, destination): 4,308 rows in 15.7 hours, about 6,600 a day. Measured after building it: about 1.2 MB a day in total, roughly 430 MB a year (about 100 bytes a traffic row with its index).
- Every stored image path is a drive-letter path (no `\Device\HarddiskVolume` forms), so a lower-cased path is a safe
  executable key. An empty path (12 records) and any process whose path is unknown count as "unattributed".

Design (proposed)
- **One cadence: hourly, UTC.** One watermark, one completeness rule (an hour is rolled up once it ended more than 5 minutes ago,
  after the flow grace), ledgers at most about an hour behind, and a few thousand rows to load per pass instead of a day's
  hundreds of thousands. Hourly rows can be regrouped into local-time days by the UI later; UTC daily rows could not be.
  (A separate daily traffic table would be about 36 MB a year instead of 150 MB, but needs a second watermark, a second gate
  for retention, and ledgers up to a day late.)
- **No port in the key.** Traffic rows are keyed (hour, executable, destination address) and carry bytes sent and received,
  flow count, first and last seen, `distinct_ports` and `top_port` (by bytes, lowest port on a tie). Distinct-port counts
  are per hour and cannot be added up across hours. Limitation, recorded: for loopback or inbound traffic the port that is
  stored is the PEER's, often ephemeral, and the meaningful local port is not collected.
- **Tables (migration v5, additive, no rewrite):** `executables(id, path_key UNIQUE, path, first_seen, last_seen)` with id 1
  reserved for "unattributed"; `destinations(addr PK, first_seen, last_seen)`; `exe_destinations(exe_id, addr, first_seen,
  last_seen)`; `rollup_system_hour(hour_start PK, samples, cpu_avg, cpu_max, memory_used_avg, memory_used_max,
  memory_total, disk_read, disk_write, net_received, net_sent, others_cpu_avg)`; `rollup_process_hour(hour_start, exe_id,
  samples_present, cpu_avg, cpu_peak, memory_peak, starts)`; `rollup_traffic_hour(hour_start, exe_id, addr, bytes_sent,
  bytes_received, flows, first_seen, last_seen, distinct_ports, top_port)`; `rollup_progress(hours_done_before)`.
- **Process rows:** `cpu_avg` is the executable's CPU summed over its instances, averaged over ALL of the hour's samples
  (absent counts as zero, the same rule as `cpu_breakdown`); `cpu_peak` is the highest per-sample sum. Samples record only
  the heaviest N processes (default 25), so an executable is described while it is among them. `starts` counts
  `process_started` EVENTS in the hour (not `processes.started`), so the number does not depend on when a restart snapshot ran.
  Samples are attributed by process INSTANCE, never by PID, so a reused PID cannot inherit another program's history.
- **Never re-roll below the watermark.** Replay protection is REPLACE inside the same transaction as the watermark advance;
  the watermark never moves backwards. Once S4 purges detail, a re-roll would overwrite good rows with thinner ones, so it is
  refused (tested).
- **Backlog:** the first pass starts at the hour of the earliest detail and advances a few hours per pass (a short pause
  between passes while behind). A gap (collector down) yields no rows for those hours but the watermark still passes them.
- **Not in S3:** deleting anything (S4), UI views of the trends (later). Until S4, the existing 30-day purge is unchanged;
  rolling up keeps well ahead of it.

Tests first: the aggregation maths (empty hours, partial hours, a process spanning an hour boundary, several instances of one
executable in one sample, unattributed rows, top-port ties), hour boundaries (a timestamp on the boundary belongs to the NEW
hour), idempotent re-run, the watermark never moving back, a backlog advancing in capped passes, a replay of an hour after a
simulated crash not double-counting, PID reuse attributed by instance, ledgers surviving a purge of the detail, and the
in-memory and SQLite implementations passing the same contract suite.

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
