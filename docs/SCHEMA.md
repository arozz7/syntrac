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

## Known products (v7)

`known_products` (migration v7, additive): the products the user has confirmed. `label` and `note` are the user's words;
`publisher` (lower case, '' for a product with no signer and no company) and `root` (the install folder, lower case) are exactly a
`product_key` and together are unique; `product_name` is the version-information product name when it was confirmed.
`created_at` is when it was first confirmed and does not move when the label is edited; `updated_at` does. Ids are never
reused (AUTOINCREMENT). Permanent: the purge of detail never touches it. Written only by the service, on request over the pipe.
Design: `docs/KNOWN-PROGRAMS-DESIGN.md`.

## Address names (v8)

`address_names` (migration v8, additive): one row per (`addr`, `name`) pair ever seen in a DNS answer, with `first_seen` and
`last_seen` only (no counters, so storing the same answers again changes nothing). `addr` is canonical text (an IPv4-mapped IPv6
address is the IPv4 address); `name` is lower case without a trailing dot. At most 50 names are kept per address (the most
recently seen; a tie keeps the name that sorts first). Written in the same transaction as the hourly rollup, plus once by the
backfill of the retained DNS events, which `names_progress` (one row, present when done) remembers. Permanent: the purge of
detail never touches either table. A name is what a program asked for, not who owns the address. Plan: `docs/NETWORK-PLAN.md`.
Migration 8 also adds `idx_exe_destinations_addr` on `exe_destinations (addr, first_seen)`, so the program that was first to talk
to a destination is found without scanning the pairs (the Network tab's "new destinations").

## How programs run (v9)

`executable_accounts` and `executable_services` (migration v9, additive): how each program has been SEEN running. One row per
(`exe_id`, `sid`) with the account's name as Windows gave it (`account_name`, NULL when it could not be named), its kind
(`account_kind`: `system`, `local_service`, `network_service`, `service_account`, `user`, `other`) and `first_seen` / `last_seen`;
one row per (`exe_id`, `service_name`) with the display name and `first_seen` / `last_seen`. A program is a SET of accounts (it
can run as SYSTEM and as a user) and a service host carries many services. Only first and last seen are kept (MIN and MAX), so
recording the same sighting twice changes nothing, and a missing name never erases a known one. Written only by the service's
context worker, and a program the rollups have not met yet gets its `executables` row from there (with the spelling first
seen). Permanent: the purge of detail never touches either table. The account name contains the Windows user name and stays in
this local database. Plan: `docs/RUNAS-PLAN.md`.

## Owner data (v10)

`asn_ranges` and `owner_dataset` (migration v10, additive): the downloaded IP-to-owner dataset. `asn_ranges`: one row per range,
`range_start` (primary key) and `range_end` as 16 big-endian bytes (an IPv4 address is stored IPv4-mapped, so one ordering serves
both families and a lookup is one seek: the range with the greatest start not above the address, then a check against its end),
`asn`, `country` (the country the network is registered in, `ZZ` when unknown) and `org` (cleaned, at most 120 characters).
Ranges the file marks as not routed (AS 0) are not kept; overlapping or malformed ranges are dropped and counted. `owner_dataset`
is one row (id 1): `source` (host it came from), `downloaded_at`, `rows` kept, `bad_lines` dropped, `sha256` of the file as
downloaded. The whole dataset is replaced at once: the new ranges are written into `asn_ranges_staging` in short transactions and
swapped in with one transaction, so a reader sees the old dataset or the new one. Nothing is stored per address: an owner is
looked up when a page is shown. Written only by the service when the user asks for a download. Plan: `docs/OWNER-PLAN.md`.

## How programs get started (v11)

`executable_launchers` and `launcher_sweeps` (migration v11, additive): one row per (`exe_id`, `kind`, `location`) that was ever
seen starting a program: `kind` is `service_auto`, `service_other`, `task`, `run_key`, `run_once` or `startup_folder`; `location` is
the service name, the task path, the key and value name or the shortcut; `detail` is the kinds of trigger of a task or the start type
of a service; `arguments` is what follows the program (cleaned, at most 400 characters); `first_seen` and `last_seen` only (MIN and
MAX). `launcher_sweeps` holds, per kind, when that kind was last read COMPLETELY: an entry whose `last_seen` is older than that is
no longer configured (its history stays). A source that failed does not move its kinds, so nothing is wrongly shown as removed. A
program the rollups have not met yet gets its `executables` row from there. Permanent: the purge of detail never touches them.
Written only by the service's launch worker. Plan: `docs/RUNAS-PLAN.md`.

## Install records (v12)

`install_records` (migration v12, additive): what the Uninstall registry keys say about installed products, as a CLUE about where a program
came from. One row per (`hive`, `key_name`) where `hive` is `machine` (64-bit view), `machine32` (32-bit programs) or `user:<sid>`; `display_name`,
`publisher`, `version` and `install_date` as the registry holds them (cleaned, capped); `location`, the install folder (lower case, no trailing
backslash; NULL when the entry names none or only one too general to mean anything); `location_derived` (the folder was worked out from the icon
or the uninstall command, not stated); `system_component`; `first_seen` and `last_seen` only (MIN and MAX), the other fields being those of the newest
sighting. When the keys were last read completely is the `launcher_sweeps` row of kind `install_records`: a record whose `last_seen` is older is no
longer installed (its history stays). Not stored per program: a program is matched to the record whose folder holds it when shown. Permanent:
the purge of detail never touches it. Written only by the service's launch worker. Plan: `docs/RUNAS-PLAN.md`.

## Findings (v13)

`findings` (migration v13, additive): what the detection rules raised, one row per thing (`dedupe_key` is UNIQUE: the same thing found again is a hit, not a
new row). `kind` is the rule, `subject` the stable lower-case key an expectation matches, `tier` is `unusual`, `suspicious` or `confirmed`, `why` one plain
sentence, `evidence` one fact per line, `status` is `open`, `expected`, `not_expected`, `snoozed` or `resolved`, `first_at` / `last_at` / `hits`, and when the user
decided (`decided_at`, `snoozed_until`). `expectations`: what the user said is expected, UNIQUE per (`kind`, `subject`): exactly that subject, never wider;
`source_finding`; `ceiling` for usage rules ("raise the limit to this level"). `detect_state`: one row with `switched_on_at` (the first time detection ran: "new"
rules count only what came after), `last_run_at` and `last_error`. Permanent: the purge of detail never touches them. Written only by the service (the app asks
through the pipe). Plan: `docs/ALERTS-PLAN.md`.

## Retention

`Retention::purge_before(cutoff)` removes events, flows, metrics older than the cutoff, and ended processes that ended
before it. Running processes are always kept. Default: 30 days, checked hourly.

## Read-only access

`SqliteStore::open_read_only` opens a WAL database for reading, including while the writer is running. SQLite needs to
create the `-shm` file, so the database folder must let the reading user create files. Restricted ACLs on the folder
(planned for the Phase 1b installer) must keep that in mind.

## Planned (not yet migrated)

Config snapshots and diffs, baselines, application profiles.
