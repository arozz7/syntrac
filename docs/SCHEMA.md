# SYNTRAC Database Schema

SQLite in WAL mode. The service is the only writer; the UI opens the file read-only.
Migrations are an ordered, append-only list in `crates/syntrac-store/src/migrations.rs`; the applied count is kept in
`PRAGMA user_version`. A database newer than the binary is rejected, never modified.

Timestamps are **microseconds since the Unix epoch (UTC)**, stored as INTEGER.
Ids are UUIDs stored as hyphenated TEXT.

## Version 1

### `processes`: one row per process *instance*

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | `ProcessInstanceId`: UUIDv5 of `(pid, creation time)` |
| `pid` | INTEGER | Windows PID (reused over time; not unique) |
| `parent_pid` | INTEGER | PID reported at start |
| `parent_id` | TEXT NULL | Resolved parent instance, when known at start |
| `image_path` | TEXT | Executable path |
| `command_line` | TEXT NULL | Subject to redaction rules (see THREAT-MODEL) |
| `started` | INTEGER | |
| `ended` | INTEGER NULL | NULL while running |
| `exit_code` | INTEGER NULL | |

Index: `(pid, started)`, which serves `resolve(pid, at)`.

`resolve(pid, at)` = the row with `pid` where `started <= at AND (ended IS NULL OR at <= ended)`, latest start wins.

### `events`: normalized event log

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | Event UUID |
| `ts` | INTEGER | |
| `category` | TEXT | `process`, `network`, `dns` (derived from payload) |
| `kind` | TEXT | e.g. `process_started`, `network_connection` |
| `process_instance_id` | TEXT NULL | NULL when the PID could not be attributed |
| `severity` | TEXT NULL | `unusual`, `suspicious`, `confirmed` |
| `payload` | TEXT | JSON of `EventPayload` (includes the `kind` tag) |

Indexes: `(ts)`, `(category, ts)`, `(process_instance_id, ts)`.

## Version 2

### `flows`: indexed copy of each `network_flow` event

One row per (process instance, protocol, remote address, remote port, time window). Also present in `events` for the
timeline; this table exists so queries such as "top destinations" use indexes, not JSON.

| Column | Type | Notes |
|---|---|---|
| `event_id` | TEXT PK | Same id as the row in `events` |
| `window_start`, `window_end` | INTEGER | Aligned fixed windows (default 10 s) |
| `process_instance_id` | TEXT NULL | NULL when unattributed |
| `pid` | INTEGER | PID the source reported |
| `protocol` | TEXT | `tcp` / `udp` |
| `remote_addr`, `remote_port` | TEXT, INTEGER | Canonical form (IPv4-mapped IPv6 becomes IPv4); ports in host order |
| `bytes_sent`, `bytes_received` | INTEGER | Saturate at i64::MAX |
| `opened`, `closed`, `failed` | INTEGER | Counts of connect/accept, close, failure events in the window |

Indexes: `(window_start)`, `(remote_addr, remote_port, window_start)`, `(process_instance_id, window_start)`.

### `system_metrics`: one row per sample

`ts` (PK), `cpu_percent` (share of the WHOLE machine, 0-100), `memory_used`, `memory_total`, `disk_read`, `disk_write`,
`net_received`, `net_sent` (bytes since the previous sample), and the long tail not listed individually:
`others_count`, `others_cpu`, `others_memory`.

### `process_metrics`: the heaviest processes per sample

`ts`, `instance` (NULL when the process is unknown), `pid`, `cpu_percent` (share of the whole machine), `memory_bytes`.
Indexes: `(ts)`, `(instance, ts)`.

`cpu_breakdown(from, to)` averages over ALL samples in the window (absent samples count as zero), and reports:
the heaviest processes, the long tail, and `unaccounted` = system CPU minus everything measured. A gap is expected:
interrupt/driver time belongs to no process, and unelevated runs cannot read many system processes.

## Version 3

`processes.ended_approximate` (INTEGER, default 0): 1 when `ended` is only an upper bound. Set when a startup snapshot
finds a process recorded as running that is no longer alive (it exited while the collector was down); closed at the
snapshot time. A later real exit report replaces the bound and clears the flag.

## Executable identity (v6)

`executable_identity` (migration v6, additive): one row per executable, `exe_id` = `executables.id` (the same dimension the
rollups use). Permanent: the purge of detail never touches it. Columns: `last_attempt` (when the file was last looked at,
successfully or not), `inspected_at` (when the facts were measured), `failure_kind` / `failure_message` /
`failure_attempts` (NULL kind: the last look succeeded; attempts count consecutive failures and set the back-off), and the
facts, all NULL until a first successful read: `size`, `modified`, `sha256` (NULL when too large to hash),
`signature_status` (`valid`, `untrusted`, `altered`, `unsigned`), `signer`, `thumbprint`, `signature_kind` (`embedded`,
`catalog`), version resource (`company`, `product`, `description`, `original_filename`, `file_version`) and `zone_id` (the
mark of the web; 3 = Internet). Anything derivable from the path alone (location class, product) is NOT stored: it is computed
by `syntrac_core::exe_location` when shown. Plan: `docs/IDENTITY-PLAN.md`.

## Retention

`Retention::purge_before(cutoff)` removes events, flows, metrics older than the cutoff, and ended processes that ended
before it. Running processes are always kept. Default: 30 days, checked hourly.

## Read-only access

`SqliteStore::open_read_only` opens a WAL database for reading, including while the writer is running. SQLite needs to
create the `-shm` file, so the database folder must let the reading user create files. Restricted ACLs on the folder
(planned for the Phase 1b installer) must keep that in mind.

## Planned (not yet migrated)

Remote endpoints (first/last seen), DNS index table, config snapshots
and diffs, baselines, application profiles, alerts, hourly/daily rollups.
