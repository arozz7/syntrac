# Phase 01a: The collection pipeline (console mode)

Date: 2026-11-13. Status: **built; offline-verified; first elevated ETW run captured good data but exposed a shutdown hang, now fixed and awaiting a second run.**

## Goal

Everything between the operating system and the database, runnable from a terminal: core types, storage, the Windows
adapters, and a console host. The Windows service wrapper, the named pipe and the UI are Phase 1b.

## Built

| Area | Result |
|---|---|
| Core | Real-`CreateTime` identity; `Timestamp::from_filetime`; `ProcessObserved` (snapshot) vs `ProcessStarted`; raw `NetworkActivity` folded into `NetworkFlow` by `FlowAggregator`; `DnsQuery` with addresses; metrics types; `Pipeline`; ports `MetricsRepository`, `FlowQueries`, `Retention`, `MetricsSource` |
| Testkit | In-memory metrics/flows/retention, `MockMetricsSource`, `FailingTelemetrySource`, shared `ManualClock`, new contract suites |
| Store | `sqlite.rs` split into `connection`, `processes`, `events`, `metrics`, `codec`; migration 2 (`flows`, `system_metrics`, `process_metrics`); retention; `open_read_only`; `event_counts_by_kind` |
| Collector | Pure mappers (`paths`, `etw_map`, `metrics_build`, `snapshot_map`) plus Windows adapters (`etw`, `snapshot`, `sysmetrics`, `volumes`, `system`) |
| Service | `Collector` run loop, `ServiceConfig` (`config/service.toml`), CLI, console `main` |
| Tooling | `scripts/dev/run-console.ps1` (elevated launcher) |

## Tests

All written before the code they cover. See the final `verify.ps1` result at the bottom of this file (Rust tests plus the unchanged 47 frontend tests).
Highlights, each tied to a failure mode rather than a happy path:

- Identity: ETW and the snapshot give the same process the same id (exact spike FILETIME `134390031692131159`).
- Parent resolved as of the child's creation time; a stop for a process never seen is unattributed and does NOT lose
  the rest of the batch; a late snapshot cannot resurrect an exited process; a process that exits after the snapshot
  began is closed even when its exit is delivered first.
- Flows keyed by process instance (a reused PID never shares a flow); windows close by event time and by the injected
  clock; shutdown flushes every open window; raw activity is never stored.
- Network mapping from real values: port 47873 becomes 443; TCP uses `daddr`; **UDP receive uses `saddr`**; events 14,
  16, 18 (and IPv6 equivalents) are ignored so bytes are not double-counted; IPv4-mapped IPv6 is canonicalised.
- DNS: only event 3008 (one lookup, one record); `QueryResults` parsed from the spike string (trailing `;`, CNAME
  entries, duplicates).
- NT paths: `\Device\HarddiskVolume7\...` becomes `C:\...`; `Volume70` is not `Volume7`; unknown devices pass through.
- Store: contract suites against in-memory and SQLite; v1 database upgrades with data kept; read-only open works while
  a writer holds the database, with no writer present, and refuses writes.
- Run loop: a failing source or sample is counted and the loop continues; purge on schedule; shutdown drains and flushes.
- **Late events (found in review):** ETW delivers in buffered batches, so an event stamped 9.8 s can arrive after the
  clock passed 10 s. A window is now stored only `flow_flush_grace_secs` (default 3) after it ends, and a test proves a
  late event joins its window instead of creating a duplicate row.
- **Source order cannot change attribution (found in review):** all sources are merged into one time-ordered batch
  before correlation, so listing ETW before or after the snapshot gives the same result (tested both ways).
- ETW diagnostics: parse errors are bucketed by (provider, event id); failure events (17, 49), which carry only an error
  code, are no longer miscounted as parse errors; a data event missing its byte count IS an error rather than a silent
  zero; delivery lag and "future" timestamps are tracked, which settles the FILETIME-vs-QPC question with data.
- Config: unknown keys rejected; zero values rejected; the shipped `config/service.toml` must stay valid.

## Findings

1. **The earlier direction note was wrong.** `phase-00.md` said receive events carry the LOCAL endpoint in `daddr`. The
   manifest and data show: TCP events always describe the connection as local `saddr` / remote `daddr`; only for UDP
   receive is `daddr` this machine (192.168.1.50 is this machine's address) or a multicast group. The mapper encodes the
   corrected rule. The `phase-00.md` note has been fixed.
2. **Event 18/34 is a second count of the same bytes** ("bytes copied on behalf of user"): ignored.
3. **Live unelevated check (real data, `--no-etw`):** 244 processes and 55 connections observed, 42 flows, metrics every
   5 s, zero errors, collector overhead 0.01-0.04% of the machine. But:
   - 173 of ~417 processes were **skipped** (their creation time cannot be read unelevated), so those have no identity.
   - Listed process CPU plus the long tail was 0.7-2.5% against a system figure of 4-6%. The gap is interrupt/driver
     time (belongs to no process) plus processes that cannot be read. The breakdown now reports it explicitly as
     **unaccounted**, so the 30%-CPU question cannot be silently mis-answered.
   - The first metrics sample was all zeros (CPU is a delta). The first sample now waits one interval.
4. `sqlite.rs` is superseded by the split modules and is no longer referenced. It was NOT deleted (see below).

## First elevated run (2026-11-12 20:05, 60 s, `data/syntrac-20261112-200551.db`)

The owner ran `run-console.ps1`. Read afterwards with `inspect-run.py`:

| Check | Result |
|---|---|
| Processes | 404 observed at startup (vs 244 unelevated), 45 started live, 34 exited live |
| Exit matching | **34 of 34 exits matched their process** (the real-`CreateTime` identity works across ETW and the snapshot) |
| Flows | 904 flow rows over 339 distinct remote endpoints; **99% attributed**; **0 duplicate flow keys** |
| Byte counts | real values (for example 11 MB received from one endpoint), so the `size` / `saddr` / `sport` field names are right |
| DNS | 158 lookups, **100% attributed** to a calling process |
| Metrics | 11 samples, 100% of listed processes resolved to an instance |
| CPU | system 8.6%, listed processes 5.7%, long tail 0.6%, **not attributed 2.3%** (kernel/interrupt time and unreadable processes) |

**Lost:** the run summary (ETW drops, parse errors by event, delivery lag / future timestamps, the snapshot's skipped
count, the collector's own CPU). The output was only tee'd through a pipe, and PowerShell's `Tee-Object` writes nothing
until the pipeline ends, so a stalled exit left a 0-byte file. These questions are therefore still open.

**Defect found: the process outlived its 60 s run.** The database's last write was about 62 s after start, so the final
drain and flush completed; what stalled was teardown. `ferrisetw`'s own stop calls `CloseTrace` first and then stops the
session (the reverse of Microsoft's guidance), and `CloseTrace` can block. The exact blocking call is not proven, because
the evidence was lost with the summary. Fixes, all in place:

1. `TelemetrySource::stop()` is a new lifecycle step; the collector calls it on every source BEFORE the final drain
   (tested: events a source releases on stop are still stored).
2. `EtwSource` stops the session BY NAME on a helper thread with a 5 s timeout and leaks the `ferrisetw` trace object
   rather than dropping it (dropping repeats the blocking sequence).
3. The console host starts a 20 s shutdown watchdog that ends the process (exit code 3) if teardown ever stalls. By then
   the data is already stored.
4. The program writes its own summary file (`--summary-file`), and shutdown logs each step, so a future stall shows
   exactly where.

These fixes are unit-tested only where possible (the stop ordering) and exercised live on the non-ETW path; the ETW stop
itself is unverified until the next elevated run.
## Still not verified (needs a second elevated run)

- ETW **drops** and **parse errors by event id** (events did flow and parse, but the counters were lost with the summary).
- That the process now EXITS promptly after its run (the fix above).
- That `raw_timestamp()` is system time (FILETIME) on this trace (documented behaviour; the console run will show
  event times next to wall-clock time through ordering and attribution).
- How many processes the snapshot skips when elevated.
- Collector CPU with ETW running, idle and under load, against the 2% budget.
- The caller-PID attribution of DNS events (consistent in the spike, not proven).

Run: `powershell -ExecutionPolicy Bypass -File scripts/dev/run-console.ps1 -Seconds 60` (UAC prompt). During the run:
open and close a few programs, browse a site or two, and generate some CPU load. Output: `aiChangeLog/console-run-<stamp>.txt`
and the database `data/syntrac-<stamp>.db` (git-ignored). Afterwards: `python -I scripts/dev/inspect-run.py data/syntrac-<stamp>.db`
reports attribution rates, duplicate flows (should be 0), process lifetimes, the CPU breakdown including the unattributed
share, and the busiest destinations.

## Deviations and pending confirmation

- Removed with the owner's confirmation (2026-11-13): `crates/syntrac-store/src/sqlite.rs` (superseded by the split modules;
  it was already absent from the working tree when I went to delete it) and the Phase 0 spike binary
  `crates/syntrac-collector/src/bin/etw_spike/`. `scripts/dev/run-etw-spike.ps1` was kept but now only says the spike was
  removed (it was not named in the confirmation). The spike's results remain in `aiChangeLog/etw-spike-result-kw*.txt`.
- `run-console.ps1` deletes nothing: every run writes its own timestamped summary and database (an earlier draft cleared the previous database; that was changed after review).

## Next

Phase 1b: Windows service host, restricted database ACL (the reading user must still be able to create the `-shm`
file), named-pipe link, and the UI (dashboard, process list, connections list).


## Final gate (end of Phase 1a build)

`scripts/dev/verify.ps1` exit code 0: **141 Rust tests** and **47 frontend tests**, clippy with warnings as errors, format
checks, build and file-size check all clean. Live ETW behaviour is NOT covered by these and remains unverified until the
elevated run.


