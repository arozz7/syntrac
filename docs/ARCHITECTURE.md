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
| `Retention` | Memory | Implemented: `purge_before(cutoff)` |
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

## Collector adapters (`syntrac-collector`)

Each Windows adapter is split in two: a thin adapter that reads raw values, and a **pure, unit-tested function** that
decides what they mean (using values captured from real traces).

| Adapter | Pure part | Notes |
|---|---|---|
| `etw` (Kernel-Process, Kernel-Network, DNS-Client) | `etw_map`, `paths` | Bounded channel + `try_send`; drops and parse errors are counted. A stale trace session from a crashed run is stopped at start. Kernel-Process uses keyword mask `0x10` |
| `snapshot` (processes via Toolhelp, TCP table via IP Helper) | `snapshot_map` | Stamped with the time the snapshot BEGAN; `ProcessObserved`, not `ProcessStarted` |
| `sysmetrics` (`sysinfo`) | `metrics_build` | One CPU unit: share of the WHOLE machine |
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

