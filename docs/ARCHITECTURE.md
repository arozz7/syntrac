# SYNTRAC Architecture

Three loosely coupled layers, wired together by dependency injection. See [PLAN.md](PLAN.md) for decisions and phases.

## Layers

| Layer | Responsibility | Where |
|---|---|---|
| Reasoning (brain) | Pure, synchronous logic: correlate, detect, score. No I/O, no vendor imports. | `syntrac-core` |
| Memory (context) | State and storage behind repository traits. | traits in `syntrac-core`; `syntrac-store` (SQLite), `syntrac-testkit` (in-memory) |
| Tools (hands) | Side effects behind adapter traits: ETW, file hashing, signatures, LLM. | traits in `syntrac-core`; `syntrac-collector` |

## Process model

```
syntrac-service (elevated Windows service)   -- writes -->   SQLite (WAL)
        ^                                                        |
        | named pipe (commands, live push; ACL: current user)    | read-only
        |                                                        v
syntrac-app (Tauri 2, unelevated)  <--------------------------- reads
```

The service is the only writer. The UI can close without stopping collection.

## Installed layout (Phase 1b)

| Path | Who can write | Contents |
|---|---|---|
| `%ProgramFiles%\Syntrac\bin\syntrac-service.exe` | administrators only | the service binary |
| `%ProgramFiles%\Syntrac\config\service.toml` | administrators only | configuration read by the service (SYSTEM) |
| `%ProgramData%\Syntrac\data\syntrac.db` | SYSTEM, administrators, **the installing user (Modify)** | the database |
| `%ProgramData%\Syntrac\logs\` | SYSTEM, administrators; the user may read | rolling daily logs, 14 kept |

The binary and the config are deliberately NOT in the user-writable data folder: the service runs as SYSTEM, so a
user-writable executable or config would be a privilege-escalation path. The user needs Modify on `data\` only because
SQLite must create the WAL `-shm` file for an unelevated reader. This is the accepted trade-off of the "UI reads the
database directly" decision: any program running as that user can read or change the database.

Console and service use different ETW session names (`SyntracConsole`, `SyntracCollector`) and different default
databases, so a console run can never clear the installed service's trace or write into its database.
## Status pipe (Phase 1b)

`\\.\pipe\syntrac-collector` (console mode: `syntrac-console`). One JSON object per line, one request per connection:
`{"request":"ping"}` and `{"request":"status"}`; answers are `pong`, a `status` report (version, pid, uptime, database
path, totals, ETW health, last pass time) or an `error`. Both requests are read-only. Code: crate `syntrac-ipc`
(`protocol`, `serve`, `sddl` are pure and tested; `pipe` is the thin Win32 layer). The collector publishes to a
`StatusBoard` after each pass; connection threads only read it, so a slow client can never slow collection.

Controls, each tested: an explicit access list (SYSTEM, Administrators and the installing user's SID, validated so it
cannot inject entries; nobody else, nothing inherited); `FILE_FLAG_FIRST_PIPE_INSTANCE` (starting FAILS if the name is
taken, so it cannot be squatted); remote clients rejected; 4 KiB request cap; a total deadline per connection (default
3 s); at most 8 connections at once. If the pipe cannot start, collection carries on without it.

Client: `syntrac-service --status` / `--ping` (`--pipe <name>`), exit codes 0 ok, 4 not running, 5 access denied.
 `syntrac-core::ports`)

| Interface | Kind | Status |
|---|---|---|
| `ProcessRepository` | Memory | Implemented (SQLite, in-memory). `record_start` is an idempotent upsert that never clears a recorded exit |
| `EventRepository` | Memory | Implemented (SQLite, in-memory) |
| `MetricsRepository` | Memory | Implemented: system series and `cpu_breakdown` (which processes used the CPU, plus the long tail and the unaccounted share) |
| `FlowQueries` | Memory | Implemented: `top_destinations` and `recent_flows` (with the process name) from the indexed `flows` table |
| `ProcessQueries` | Memory | Implemented: `list_processes` (running only, name search, newest first) for the UI |
| `Retention` | Memory | Implemented: `purge_before(cutoff)` (detail only; rollups and ledgers are never purged by it) |
| `StorageStats` / `FreeSpace` / `DetailStore` | Memory + Tool | Implemented (SQLite, in-memory; Windows `GetDiskFreeSpaceExW`): size accounting, space recovery and the narrow view the storage guard works on |
| `RollupRepository` | Memory | Implemented (SQLite, in-memory; shared contract suite): loads one hour of detail, stores its rollup + ledgers + watermark atomically, reads history and ledgers. The maths is the pure `syntrac_core::rollup::roll_up_hour` |
| `DestinationTrendRepository` | Memory | Implemented (SQLite, in-memory; shared contract suite): per-destination sums over a range, destinations first seen after an instant (with the first program), and the programs of one destination. Ranking, scope and the "new" rule are pure (`destination_trend.rs`, `addr_class.rs`) |
| `InstallRepository` / `InstallSource` | Memory + Tool | Implemented (SQLite, in-memory; shared contract suite; Windows adapter and a test fake): the products the Uninstall keys name, matched to programs by install folder. The rules are pure (`install_records.rs`) |
| `LauncherRepository` / `LaunchSource` | Memory + Tool | Implemented (SQLite, in-memory; shared contract suite; Windows adapters and a test fake): how each program gets started, per executable. The rules are pure (`launch_target.rs`, `task_xml.rs`, `launch_run.rs`) |
| `OwnerRepository` / `DatasetFetcher` | Memory + Tool | Implemented (SQLite, in-memory; shared contract suite; WinHTTP adapter and a test fake): the downloaded IP-to-owner dataset and how it is fetched. The rules are pure (`owner_dataset.rs`, `owner_refresh.rs`) |
| `ContextRepository` / `ProcessContextSource` | Memory + Tool | Implemented (SQLite, in-memory; shared contract suite; Windows adapter and a test fake): which account each program has been seen running as and which services it hosts, per executable. The rules are pure (`exe_context.rs`, `context_run.rs`) |
| `TelemetrySource` | Tool | Implemented: `EtwSource`, `SnapshotSource` (Windows), `MockTelemetrySource` |
| `MetricsSource` | Tool | Implemented: `SysinfoMetricsSource`, `MockMetricsSource` |
| `Clock` | Tool | Implemented: `SystemClock`, `ManualClock` |
| `EntityRepository` / `ProfileRepository` / `BaselineRepository` / `AlertRepository` | Memory | Planned (Phases 2 and 3) |
| `FileEnricher`, `IntelProvider` | Tool | Planned (Phases 2 and 3) |
| `LLMService` | Tool | Planned (Phase 4): adapter over the local llama-server's OpenAI-compatible API |

## Reasoning loop

```
sources -> sort by time -> correlate (PID -> process instance) -> raw network activity -> FlowAggregator
                                                               -> everything else ----> events table
FlowAggregator --(window ended, or shutdown)--> flow events --> events table (no payload) + indexed flows table (the flow)
MetricsSource --> resolve each PID to its instance --> system_metrics + process_metrics
```

* `Pipeline::ingest` drains a source, sorts by timestamp and correlates each event. Raw `NetworkActivity` is never
  stored; it is folded into `NetworkFlow` events keyed by **process instance** (not bare PID).
* `Pipeline::flush_due(t)` stores windows that ended before `t`. The collector passes `now - flow_flush_grace`, so events
  that ETW delivers late still join their window instead of creating a duplicate row. `flush_all` stores every open
  window at shutdown.
* The collector gathers ALL sources into one batch before correlating (`Pipeline::ingest_events` sorts the whole batch
  by time), so the order sources are listed in never changes attribution.
* `Collector` (in `syntrac-service`) is the run loop around the pipeline: drain sources, flush finished windows, sample
  metrics and purge on their schedules. A failing source, sample or write is counted and logged; the loop never stops
  for one. It is proven offline in `syntrac-service/tests/collector_loop.rs`.

Later phases add `Detector` and `RiskScorer` after the correlator, each pure and injected.


## Rollups and ledgers (schema v5)

Detail (events, flows, per-process samples) is kept for a short window; what survives for trends is a set of small hourly
summaries (UTC) written by the collector once an hour has ended and settled (5 minutes after the hour, so late flow events
are in). `roll_up_hour` (pure) turns one hour of raw rows into: one system row (CPU average and peak, memory, disk and network
totals), one row per executable (samples present, CPU average over ALL the hour's samples, CPU peak, memory peak, starts), and
one row per executable and destination address (bytes, flows, first and last seen, distinct ports, busiest port). The port is
deliberately not part of the key (most ports are ephemeral). Executables are identified by their lower-cased image path;
owners are resolved from the process INSTANCE, never the PID. Ledgers (first and last seen per executable, per destination
and per pair) are permanent and widened with MIN/MAX, so updating them is idempotent. A watermark (`rollup_progress`) moves
forward in the same transaction that stores an hour; an hour at or below it is refused, so a re-roll can never overwrite
good rows with thinner ones after detail has been purged. The collector rolls up before it purges and works a backlog off a
couple of hours per pass.


## Retention and the storage guard

Detail is deleted only when it is older than `detail_retention_days` (default 14) AND already rolled up: the purge cutoff is
`min(now - retention, rollup watermark)` (`retention::purge_cutoff`), and the early-trim cutoff is bounded by the same
watermark, so nothing that has not been summarised is ever deleted. Purging and trimming work an hour at a time (deleting costs
about a second per hour of detail) with a pause between passes, so the loop keeps draining ETW. The guard measures the database
(live bytes, free pages) and the data drive (`GetDiskFreeSpaceExW`) every minute: `Trimming` when live data exceeds
`max_database_gb` or free space (counting pages the database has freed but not yet returned) is below `min_free_space_gb`;
`Paused` below `hard_floor_free_gb`, when the collector keeps only process events and skips sampling until free space is back above
the minimum. Levels change with a 10% margin so they do not flap. The database uses incremental auto-vacuum, switched on once
at start (a failure is a logged notice and a retry at the next start, never a crash loop); each purge returns freed pages to the
drive. The pipe's status carries the result (`storage`), and the UI shows it.

## History views (trends)

The app's History screen reads the hourly rollups (read-only, like every other view), so it still works after the detail has been
purged. `history.rs` builds the view models from `RollupRepository` and `StorageStats` (`system_history`: the stored UTC hours in
a range, plus where the history begins); the store returns completed hours only, and a missing hour is a gap, never a zero. Grouping
hours into LOCAL-time days (for ranges beyond 7 days) is done in the UI (`charts/historyBuckets.ts`), which knows the time zone
and daylight saving; averages are weighted by the samples behind each hour. Plan and slices: `docs/TRENDS-PLAN.md`.

Programs are ranked over a range by `syntrac_core::trend`: the store only sums (`TrendRepository`: one group-by per table, so a
year of rows never reaches the app), and merging, averaging and ranking are pure. A program's CPU average divides by the samples of
the whole range (not recorded counts as zero); network totals leave out loopback. Summing a year takes about 2 s, ranking the
sums under 1 ms, so the app's backend keeps the sums of each range until the rollup watermark moves (an hour).

## Network history

The Network tab (History) answers where the machine talks to over days and months. Two things make it work after the detail is
purged. First, the permanent address-to-name table (`address_names`, schema v8): the pure `dns_names.rs` turns each hour's DNS
answers into (address, name) pairs, stored with the hourly rollup in one transaction, plus a one-time backfill of the lookups still
retained when v8 first starts; only first and last seen are kept, so writing twice changes nothing, and at most 50 names are kept
per address. Second, the existing rollups and ledgers: `DestinationTrendRepository` sums one group-by per question, and
`destination_trend.rs` ranks (bytes, flows or programs; ties by address), applies the scope (internet addresses by default, chosen
with the pure `addr_class.rs`) BEFORE the limit, and decides what is new: first seen after the window start, but never inside the
first 24 hours of history (the baseline). A name is what a program ASKED for, never an owner, and the screen says so. The backend
(`history_network.rs`) keeps each range's sums until the rollup watermark moves, like the Programs ranking. Plan: `docs/NETWORK-PLAN.md`.

## How programs get started (launch records)

A worker of its own (`syntrac-service/src/launch_worker.rs`, own thread and connection) sweeps every `launch_interval_minutes` (10)
the places that say what starts, through the `LaunchSource` port: services (`launch_services.rs`), scheduled tasks
(`launch_tasks.rs`, the task files under System32Tasks), Run and RunOnce keys of the machine and of each loaded user
(`launch_registry.rs`, `registry.rs`) and the Startup folders (`launch_startup.rs`, shortcuts resolved through the shell). The pure
core turns each raw entry into the program it starts (`launch_target.rs`: quotes, unquoted paths with spaces settled against the
file system, variables, proxies recorded as themselves) and records it (`launch_run.rs`); the repository keeps first and last seen and
when each kind was last read completely. The identity panel shows "Starts with Windows via", the table a quiet "Starts at boot".
Plan: `docs/RUNAS-PLAN.md`.

## Findings (what is new, what is unusual, and what you told us is expected)

The detect worker (`syntrac-service/src/detect_worker.rs`, own thread, own connection, off until `detect_enabled = true`) runs the detectors every few minutes
(`syntrac_core::run_detection`): each `Detector` returns `Candidate`s, the shared cleaner caps their text, and the pure `reconcile` drops what the user said is
expected (a kind of finding about exactly one subject), touches or reopens what is already stored (by `dedupe_key` and status: expected stays quiet, a snooze
stays quiet until it ends, not-expected stays visible) and inserts the rest, most serious first, up to a cap per run. The stores (`findings`, `expectations`,
`detect_state`, schema v13) are behind `FindingRepository`, `ExpectationRepository` and `DetectionStateRepository` with one shared contract suite. The user's
decisions (`expected_this_time`, `always_expected`, `not_expected`, `snooze`, `reopen`) and removing an expectation reach the service over the pipe
(`FindingDecide`, `ExpectationRemove`); the service builds any expectation from the finding IT stored, never from what the client names. The app reads the
tables read-only (`history_findings.rs`) and shows the Findings tab. The first detector is `NewThingsDetector` (`syntrac-core/src/new_things.rs`, F2): it loads the programs, start entries, accounts, known products and file changes through read ports and applies four pure rules (new program, new start entry, changed file, new account), each limited to what appeared AFTER detection was first switched on. A new version of a product already here (same maker, same install folder) is an update, not a new program; a new version by the same signer is not a changed file. The second detector is `UsageDetector` (`syntrac-core/src/usage.rs`, F3): for each program and metric (CPU as a share of the whole machine, memory peak, bytes sent, bytes received) the last two completed hours are compared with the program's own earlier active hours (the 95th percentile and the highest of the 14 days before, leaving out hours that already raised a finding); a finding needs more than 3 x the 95th percentile AND 1.5 x the highest hour, over an absolute floor, for two hours in a row (one hour of uploading ten times the highest is raised alone), and a program with fewer than 48 active hours is not judged. The sentence carries the numbers. The CPU floor is half of one core of this machine. Not per-program disk (the rollups hold none). The third detector is `NewOwnerDetector` (`new_owner.rs`, F4): an unsigned (or untrusted) program reaching a network owner it never had before, from the pairs of program and address first seen after the switch-on and the downloaded owner data (off without the data; a validly signed program's new owners are only counted in the digest; a program that is itself new has its own finding). "Review what is here today" (`review.rs`) groups the programs that were already here at the switch-on by product (maker and install folder), puts the groups with something to look at first, and offers to expect new programs below a group's folder (the pipe request `ReviewConfirm`, which the service checks against the programs it knows and refuses unless the folder is protected, see `folder_protection.rs`), for any other folder to expect that group's programs one by one (`ReviewConfirmPrograms`: the service rebuilds the set with `review_group_keys`, at most 500, never the folder), or to mark it as a known program. The Findings tab also shows a digest of the last day (new internet addresses, networks never reached before), a note of how many open findings are new since the tab was last looked at (kept in the browser, nothing depends on it), and a count of open findings beside the tab in the sidebar. An expectation covers exactly one subject, or (F6) everything below one PROTECTED install folder (Windows or Program Files on the Windows drive, minus the few folders inside them that users can write to; `syntrac-core/src/folder_protection.rs`, pure path rules; the service refuses a whole folder it cannot call protected, and matching never lets a protected folder cover a writable folder inside it), or the new programs below one deliberately chosen BUILD folder (`build:<folder>`, the `new_program` kind only, refused for places where things are dropped; the programs it hides are counted in the Findings digest). Expectations made before F6 for writable folders are still honoured and flagged. The fourth detector is `PatternDetector` (`syntrac-core/src/pattern_run.rs`, F5a, off until `patterns_enabled = true`): it matches a pack of known patterns of rogue software (`config/patterns/rogue-software.toml`, embedded in the build, parsed by `patterns.rs` with a closed vocabulary of conditions: no regular expressions over user data, no code) against programs seen running in the last 30 days and their start entries; one kind of finding per pattern, at most "suspicious", each with its technique id and a "often harmless" text. The patterns about a started program read which program started which through `SpawnRepository` (one join and group-by over the raw process rows, filtered to the child names the pack asks for; 14 days of history; no command lines are stored), and "first network use" reads the programs whose earliest sighting in the permanent program-and-address ledger is after the switch-on (`DestinationTrendRepository::first_online_since`). See `docs/PATTERNS.md`.

## Install records (where a program came from)

The launch worker also reads the Uninstall registry keys (`syntrac-collector/src/install_registry.rs`, on the read-only `registry.rs`) through
the `InstallSource` port. The pure core (`install_records.rs`) cleans each entry, works out its install folder (stated, or derived from the icon or
the uninstall command, refusing folders too general to mean anything) and, when a program is shown, matches it to the record whose folder holds
it (the most specific folder; ties all shown). The identity panel shows "Registered by an installer as"; a product group with no product name and
no signer takes the registered name before the folder name. It is a clue: it never changes a standing or merges two products. Plan:
`docs/RUNAS-PLAN.md`.

## Owner data (which network owns an address)

The one part of SYNTRAC that touches the internet, and only when the user asks. Pure core: `owner_dataset.rs` (parse the IPtoASN
file into sorted non-overlapping ranges, look an address up; IPv4 lives in IPv4-mapped IPv6 space so one ordering serves both),
`owner_refresh.rs` (the `OwnerRepository` and `DatasetFetcher` ports, and `refresh_owner_data`, which checks a download before it
may replace the stored dataset). Adapter: `syntrac-collector/src/owner_fetch.rs` (WinHTTP for the compiled-in address, size caps,
gzip). Store: `asn_ranges` and `owner_dataset` (schema v10), replaced through a staging table. Service: `owner_data.rs` runs a
download on its own thread when the pipe receives `owner_data_refresh` and reports through `owner_data_status`. App: owners are
looked up when a page is shown (one index seek per listed address; nothing is stored per address), and the Network tab has the
button with its confirmation. Plan: `docs/OWNER-PLAN.md`.

## How programs run (run-as account and services)

A worker of its own (`syntrac-service/src/context_worker.rs`, own thread and database connection, like the identity worker) looks
at the running programs every `context_interval_secs` (30). The Windows adapter (`syntrac-collector/src/process_context.rs`) takes
one process snapshot, reads each process's token (opened with the least right that works, query only) and lists the running
services once; a SID is resolved to a name once and cached. The pure core merges instances of one program under one account,
classifies the SID (`classify_sid`), and a `ContextTracker` makes sure a program that keeps running is written only when it is new,
its set of services changed, or `context_refresh_minutes` (10) have passed: about 15 ms of CPU per look and a few thousand row
writes a day instead of a million. No event schema change and nothing in the ETW path. The identity panel shows "Runs as" and
"Services"; the table shows a quiet "Service" badge. Plan: `docs/RUNAS-PLAN.md`.

## Executable identity

What the files themselves tell us about each executable (signature, version resource, SHA-256, size and date, mark of the
web), kept permanently per executable (`executable_identity`, schema v6) and shown as evidence, never as a verdict. Core
(`exe_identity.rs`) holds the types and the two ports: `FileInspector` (reads a file; the Windows adapter arrives in I1b, tests
use `FakeInspector`) and `IdentityRepository` (in-memory and SQLite, one contract suite). `identity_run.rs` is the pure,
bounded pass `inspect_due`: an executable is due when never looked at, when a failure's back-off (1 h, 6 h, 1 day, weekly) has
passed, or when it has RUN since the last look and a day has passed; a recheck compares size and time first and reads the file
again only if they differ. `exe_location.rs` derives, from the path alone, a location class (System, Program Files, ProgramData,
user profile, Temporary, Network, Other) and a product key (signer or company, plus install root) that groups the four
Acme programs into one product. Plan and slices: `docs/IDENTITY-PLAN.md`.

The app shows identity as evidence in the Programs history: each ranking row has a badge (signature state, publisher, a note when the
program runs from a temporary, user or network folder), the drill-down has an Identity panel, and a "Group by product" switch folds the
programs of one product into one row (`syntrac_core::program_group`, pure; the app builds product keys from the signature or company
and the install folder, and does not fold programs whose files have not been read yet). Identities are read once per request
(`identity_view::Identities`), so a 200-row ranking costs one query.

The Windows adapter (`syntrac-collector`: `file_inspector`, `file_trust`, `file_version`, `file_hash`, with the pure meaning of raw
values in `file_parse`) opens each file once, read-only, with full sharing, and never follows a link; the hash and the signature come
from that handle. Signatures are checked offline (revocation off, cache-only URL retrieval). The service runs it in an
`IdentityWorker` on its own thread with its own database connection (`identity_worker.rs`, started in `host.rs`), paced by
`identity_*` config keys, so reading files cannot delay ETW draining.

## Known programs

The user confirms a product from a program's drill-down (Mark as known); the app never writes the database. It sends
`known_product_add { path, label, note }` over the service pipe, and `syntrac_service::KnownProductsAdmin` (behind a
`ServiceHandler` that sends ping and status to the status board) builds the product from the identity the service itself measured
(`syntrac_core::known::new_known_product`: refuses unread, unreadable and altered files) and stores it in `known_products` (schema
v7). The pipe has its own database connection, like the identity worker. The read side is pure: `KnownIndex` judges any program as
known (its `product_key` is on the list), a lookalike (it names a known product, or has the file name of a known program without a
valid signature, with two exceptions that keep the real thing quiet) or unlisted. The app attaches that standing to ranking rows,
product rows and the drill-down, and the History > Known programs tab lists the products and the lookalikes.

## Collector adapters (`syntrac-collector`)

Each Windows adapter is split in two: a thin adapter that reads raw values, and a **pure, unit-tested function** that
decides what they mean (using values captured from real traces).

| Adapter | Pure part | Notes |
|---|---|---|
| `etw` (Kernel-Process, Kernel-Network, DNS-Client) | `etw_map`, `paths` | Bounded channel + `try_send`; drops and parse errors are counted. A stale trace session from a crashed run is stopped at start. Kernel-Process uses keyword mask `0x10` |
| `snapshot` (processes via Toolhelp, TCP table via IP Helper) | `snapshot_map` | Stamped with the time the snapshot BEGAN; `ProcessObserved`, not `ProcessStarted` |
| `sysmetrics` (`sysinfo`) | `metrics_build` | One CPU unit: share of the WHOLE machine |
| tray icon and alerts (`tray.rs`, Tauri, Windows notifications) | `toast_watch` (`plan_toasts`, `Watcher`), `tray_text`, `prefs` | One program with or without a window: closing the window keeps it in the tray; one sender of toasts (a Rust thread, every 30 s); "new" = an id above the last announced, kept in `%LOCALAPPDATA%Syntracpp.json`; a settings file that cannot be written never repeats a toast |
| `gpu_counters` (the Windows GPU performance counters, through PDH) | `syntrac_core::gpu` (`parse_engine_instance`, `gpu_figures`), `metrics_build` | One GPU unit: share of the busiest ENGINE (not of the machine); no counters = "not available", never zero; read-only, about 2 ms per sample on the test machine |
| `volumes` (`QueryDosDevice`) | `paths::NtPathConverter` | Injected volume map |

## Key invariants

1. **Process identity is `(pid, real creation time)`**, never the PID alone (`ProcessInstanceId`, UUIDv5). The creation
   time is the OS value (ETW `CreateTime`, `GetProcessTimes` in the snapshot), converted by one function
   (`Timestamp::from_filetime`), so ETW and the snapshot agree. `resolve(pid, at)` returns the instance that owned the
   PID at that moment, so a reused PID never inherits an old process's activity.
2. **Unknown stays unknown.** An event whose process cannot be identified (including the exit of a process we never saw)
   is stored unattributed rather than guessed, and never fails the batch.
3. **A parent is resolved as of the child's creation time**, not when we heard about the child.
4. **Restarts do not leave ghost processes.** A batch containing `ProcessObserved` events is a complete startup snapshot;
   anything recorded as running that is not in it exited while the collector was down, and is closed at the snapshot time
   with `ended_approximate = true` (the exit time is a bound, not a measurement).
5. **A late snapshot cannot resurrect an exited process**, and a process that exits after the snapshot began is closed
   even if its exit is delivered first (ordering by timestamp).
6. **One event envelope** (`EventEnvelope`) for all sources. Sysmon or other sources later map into the same shape.
7. **Severity tiers are distinct**: `Unusual` (baseline deviation) < `Suspicious` (rule match) < `Confirmed` (intel).
8. **CPU has one unit** (share of the whole machine). The breakdown also reports CPU that no process explains.
9. **Config-driven**: service settings are in `config/service.toml` (unknown keys rejected); rules, thresholds, prompts and
   the llama-server endpoint will live in `config/` too, not in code.

## Deviations from the plan

- The in-memory repository lives in `syntrac-testkit` (not `syntrac-store`). It is test infrastructure, and placing it
  there avoids a dependency cycle between the store and the shared contract suites.
- Network flows are stored once (schema v4): the `events` row keeps the timeline entry (id, time, owner) with a NULL payload,
  and the flow itself lives in the indexed `flows` table under the same `seq`; reading an event rebuilds the flow payload
  from it. Ids are stored as 16-byte blobs. (Before v4 flows were stored twice.)
- DNS: only the caller-side response event (3008) is stored, so one lookup is one record.

## Logging

`tracing` with structured fields and stable, low-cardinality messages, e.g. `info!(id = %id, "Process started")`.
The console host logs a periodic status line (events stored, ETW received/dropped/parse errors, own CPU).

