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

## Planned (not yet migrated)

Executables (hash, signature, first/last seen), remote endpoints (IP/domain, first/last seen), metrics samples,
config snapshots and diffs, baselines, application profiles, alerts, hourly/daily rollups, retention bookkeeping.
