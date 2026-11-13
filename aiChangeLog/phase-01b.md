# Phase 01b: Service, permissions and UI

Date: 2026-11-13. Status: **checkpoint 1 passed (smoke test 14/14, committed). Named pipe built; UI next.**

## Decisions (owner, 2026-11-13)

- The UI reads the database directly (simple). Accepted consequence: any program running as the installing user can
  read or change the database. Hardening path later: reads through the service over the pipe.
- The service runs as LocalSystem (ETW needs high privileges).

## Design review outcomes applied

A design review before coding changed the plan in these ways:

1. **Service binary and config live under Program Files** (administrators only), never in the user-writable data folder.
   SYSTEM runs the binary and reads the config; a user-writable copy would be a privilege-escalation path.
2. **Data folder permissions** are explicit: SYSTEM and Administrators full control, the installing user Modify on
   `data\` (SQLite needs to create the WAL `-shm` file) and read on `logs\`. The user's SID is captured BEFORE
   elevation, because an elevated session can belong to a different administrator account.
3. **Uninstall never deletes data**: it removes only the service registration; program files and data need explicit
   `-RemoveProgramFiles` / `-RemoveData`.
4. **Service-mode traps handled**: absolute paths required (the service starts in System32); no console handler in
   service mode; SCM Stop/Preshutdown map to the stop flag with StopPending and a wait hint longer than the watchdog; the
   watchdog reports Stopped (timeout code) before ending the process so the SCM does not read it as a crash.
5. **Console and service no longer collide**: different ETW session names and different default databases.
6. **Restart correctness**: processes that exited while the collector was down were left "running" forever. A startup
   reconciliation now closes them, flagged approximate.
7. **Build order**: queries and reconciliation, shared run function, service and scripts, then a user-run checkpoint,
   and only then the pipe and the UI, so nothing is built on an unverified read path.

## Built so far

| Area | Result |
|---|---|
| Core | `ProcessRecord.ended_approximate`; `ProcessRepository::reconcile_running`; `ProcessQueries::list_processes`; `FlowQueries::recent_flows` (with process name); `Pipeline` reconciles when a batch contains a startup snapshot |
| Store | migration 3; reconciliation via `json_each`; process listing with a LIKE-escaped, case-insensitive name search; `recent_flows` join |
| Host | `run_host` shared by console and service; console `main` is now thin |
| Service | `--service` entry (windows-service): SCM controls, rolling file logs (14 days), startup errors to `startup-error.txt` beside the exe; path rules in a tested pure function |
| Deploy | `install-service.ps1` (upgrade-safe, ACLs, delayed auto-start, restart-on-failure), `uninstall-service.ps1`, `smoke-test-service.ps1` + `smoke-elevated.ps1` (one UAC prompt), `check_db.py` |

## Tests (before the code they cover)

`verify.ps1` exit code 0: **155 Rust tests** and **47 frontend tests**. New this phase: reconciliation contract
(closes vanished, leaves live/new/ended alone, idempotent, exact exit replaces approximate), the restart scenario through
the pipeline, process-listing and recent-flows contracts for both stores, service path rules, the `--service` flag,
and three integration tests that run the real host on this machine without ETW (snapshot stored and readable back; a stop
flag ends a run promptly; two runs on one database keep the live test process "running").

**Live check I could run unelevated:** a short-lived `ping` was started, recorded as running by a first collector run,
exited while no collector ran, and a second run on the same database closed it as approximate (`ended=1 approximate=1`),
along with 9 other processes that came and went between the runs (10 in total). The v3 migration ran live.

## NOT verified (needs the owner's smoke run)

The Windows service itself cannot be exercised without installing it. The smoke test checks, from the ordinary user's
point of view: the service starts; the unelevated user can open the database read-only (`-shm` created by SYSTEM) and
sees data fresher than 30 s; the log is readable; the user CANNOT write to the program and config folders; the service
stops within 30 s and leaves no process; it restarts and writes again; and (with `-Uninstall`) the registration goes
while the data stays.

Run (normal, unelevated PowerShell): `powershell -ExecutionPolicy Bypass -File scripts/deploy/smoke-test-service.ps1`
Result: `aiChangeLog/service-smoke-<stamp>.txt`.

## First smoke run (2026-11-12 20:40, `aiChangeLog/service-smoke-20261112-204003.txt`)

13 of 14 checks passed, run by the owner as an ordinary user with one UAC prompt:

- Service installed, started, Running; stopped in **0.5 s** with no process left; restarted and wrote fresh data again.
- The unelevated user read the database read-only (WAL `-shm` created by SYSTEM) and saw data fresher than 30 s;
  schema v3; 390 running processes after the snapshot. The restart reconciliation closed 1 process that exited while
  the service was down.
- The user could NOT write into the program or config folders (the privilege-escalation guard holds).

**One failure, a real bug of mine:** no log files in `C:\ProgramData\Syntrac\logs`. The installed config said
`log_dir = ".../Syntrac/data/logs"`, so logs were written INSIDE the user-writable `data\` folder. Cause: PowerShell
variable names are case-insensitive, so the installer's local `$dataDir` silently overwrote its own `-DataDir`
parameter, and `logs` was then joined onto `data`. Effect: the service worked, but the logs sat where the user could
modify them, which defeats the point of a protected log folder.

Fix: the path rules moved into `scripts/deploy/config-helpers.ps1` (tested by `config-helpers.tests.ps1`, now a
`verify.ps1` step; it includes the exact buggy config), sub-folder variables renamed so they cannot collide, and the
installer now repairs a config with wrong paths by RENAMING it to a timestamped backup and regenerating it. The smoke
test also probes that the user cannot write into the logs folder.

Leftover from the buggy install, not touched (no deletions without asking): `C:\ProgramData\Syntrac\data\logs\` and the
one log file in it.
## Named pipe (built after checkpoint 1)

New crate `syntrac-ipc`. Pure and tested: the JSON-lines protocol (stable wire format, size caps), serving one
connection over any `Read + Write`, and the access-list builder (SID validation; injection attempts rejected). The
Win32 layer (overlapped I/O with timeouts, `FIRST_PIPE_INSTANCE`, remote clients rejected, explicit DACL) is thin and
every `unsafe` block states why it is sound. 31 tests, 10 of them on real pipes in-process.

Integrated into the shared host: a `StatusBoard` the loop publishes to and the pipe reads; optional (a pipe that cannot
start never stops collection); `--status` / `--ping` client with distinct exit codes. The installer writes
`pipe_allowed_sid` (the user's SID, captured before elevation) and treats a config without it as wrong, so the next
install regenerates it (backing the old one up). The smoke test now also checks, as an ordinary user, that the pipe
answers, that it disappears when the service stops, and that it answers again after the restart.

Live (unelevated): a real collector in one process answered `--ping` and `--status` from a second process; a wrong pipe
name and a stopped collector both exited with code 4.

Two process notes from this step: (1) a test hung because it used one synchronous Windows handle from a reader and a
writer thread at once, which serializes and deadlocks; that was a test bug, and the fix is a client that writes then
reads like any real one. (2) A malformed PowerShell here-string in one of my own edit commands pasted script text into
`service.rs`; it failed to compile at once, and was repaired by hand.

Still NOT verified: the pipe under the installed service. SYSTEM creates it, so only the smoke test as an ordinary user
proves the access list lets that user in.

## Smoke runs with the pipe (2026-11-12 21:0x)

- **Run 1:** the new check "ordinary user can read the collector status over the pipe" PASSED (the pipe created by
  SYSTEM lets the unelevated user in). The script then died: Windows PowerShell 5.1 turns any stderr text from a native
  program into a terminating error under `$ErrorActionPreference = 'Stop'`, and `--ping` reports "not running" on
  stderr on purpose. Fixed with an `Invoke-Collector` wrapper (exit code and message text captured safely; tested
  under strict mode for both the failure and success paths).
- **Run 2** (`aiChangeLog/service-smoke-20261112-210712.txt`): all checks passed except one,
  "pipe is gone once the service has stopped" (`--ping` answered). The service log shows why it was a test race, not a
  product defect: the service stopped at 02:07:18.35 and the elevated half restarted it 0.3 s later (pipe listening
  again at 02:07:18.82), before the unelevated side pinged, so the ping reached the NEW instance. The old process was
  gone (process-left check passed). Fix: the elevated half now waits for an acknowledgement (`phase2.ack`) from the
  unelevated side before restarting, so the stopped state is actually inspected.
- Also seen on run 2: 20 processes were closed as "exited while down" across the quick restart, and the pipe came back
  up with the service within 0.2 s of its start.
## UI (2026-11-12)

The smoke test passed 17/17 with the pipe (`service-smoke-20261112-211149.txt`). Then the UI, in two halves.

**Backend** (`app/src-tauri`, library `syntrac_app_lib`, 39 tests): the UI reads the SQLite file directly, read-only,
opened per call, and asks the service only for health over the pipe. Pure modules: `settings` (DB path and pipe name,
overridable with `SYNTRAC_DB` / `SYNTRAC_PIPE`), `names`, `series` (downsampling), `health` (running / stale / offline /
unknown), `views` (overview, processes, connections). `backend` wraps them in an explicit `Data<T>` state (`ok`,
`no_database`, `schema_mismatch`, `error`); `commands` are thin Tauri wrappers.

**Frontend** (`app/src`, 99 tests with the formatters and theme): `api` (interface + Tauri implementation, injected by
context), `format`, `hooks` (polling, debounce), `charts/cpuChart` (pure option builder; memory is a dashed line so it is
not distinguished by colour alone), `components` (status banner, data gate, range picker, chart wrapper), `views`
(Overview, Processes, Connections). Collector-health colours are their own `--status-*` tokens, not the detection-severity
colours, with contrast tests.

**Checked against the live installed service** through the WebView2 debug port: banner Running, chart drawn, CPU rows,
destinations, 500 processes and 200 connections listed, no console errors. Two defects found that way and fixed with
tests first: loopback traffic (16 MB of local IPC) outranked every real destination, so loopback is now excluded after
over-fetching; and processes named by pid showed it twice. One of my own edits then dropped the `pid N` detail from every
CPU row (a PowerShell escaping slip); typecheck could not see it because the field is optional, and it was found by reading
the file. Not re-checked in the running window after the fix.

**Verification gate bug (important):** the first `verify.ps1` run on the user's conda shell printed "ALL STEPS PASSED"
although every frontend step had failed with "pnpm is not recognized". A command that cannot be found does not set
`$LASTEXITCODE`, so `Invoke-Step` read a stale 0. Fixed: the exit code is reset before each step, "command not found"
errors are detected explicitly, and `scripts/dev/use-pnpm.ps1` finds node and pnpm like `use-cargo.ps1` finds cargo. Proven
with a missing command (exit 1), a failing native command (exit 1) and a passing one. Full gate: passed, 99 frontend tests.

## Finding for the CPU question (not yet acted on)

Over 15 minutes the system averaged 12.0% CPU; the listed processes plus the long tail explained about 5.4%, leaving about
4.9% to 7.7% "not attributed". Per-process CPU times exclude interrupt and DPC time, so the gap is probably driver and
interrupt work. The UI shows that gap as its own row instead of hiding it. Follow-up (Phase 2): split it using the Windows
processor interrupt/DPC counters.

## Next

Overhead-under-load measurement script; then Phase 2 (intelligence).
## Silent ETW (2026-11-12, found after the UI check)

**Finding.** The installed service had received ZERO ETW events since 02:11:55 UTC (over an hour), yet the pipe answered,
metrics kept flowing and the banner said Running. The network destinations and connection rows I first reported as
"verified live" were history from before that restart. An ordinary restart (30 s later: still 0 received) ruled out a
fast-restart race. The installed binary is byte-identical to the one that delivered 290,000 events earlier, so the
machine's ETW state changed, not the code. Root cause NOT yet established; `scripts/dev/etw-probe.ps1` (elevated)
separates "Windows ETW is stuck" from "our session is the problem".

**Made visible and self-healing (approved fixes 1-3):**
- `syntrac_core::silence` (pure, 9 tests): `SilenceWatchdog` turns "the received count stopped moving" into seconds of
  silence plus a decision (restart, with a capped number of attempts, then give up once). A restart never shortens the
  reported silence (found while testing: the first draft would have flipped the banner back to Running after a failed restart).
- Protocol: `EtwStatus.silent_secs` (optional on the wire; an older service's reply still parses).
- Service: `TelemetrySource::restart` (default: refuse), `Collector::restart_source`, a real `EtwSource` restart that feeds
  the same channel and counters; the host feeds the watchdog every pass. Config `etw_silent_restart_secs = 180`,
  `etw_max_restarts = 3` (0 = report only). `--status` prints "ETW has been SILENT for N s".
- UI: health state `Degraded` (own glyph, word and `--status-degraded-*` token, contrast and distinctness tested) when ETW has
  been silent for 120 s, shown before the service tries to repair it. Stale (old data) still outranks it.
- Smoke test: new checks that ETW receives events after the first start and after the restart.

Not yet run against the real service: the installed binary is still the old one. The new build needs reinstalling
(elevated) to get the watchdog; the real ETW restart code path has only been compiled and linted, not exercised.

### Root cause of the silent ETW (found 2026-11-12 22:4x local)

An ORPHANED trace session, `SyntracConsole`, was still running although no console collector existed (an ETW session
outlives its process when it is not stopped). While it existed, every other session on the machine lost all events:
the independent probe (one provider per session, generous buffers) captured 0 of 64 / 224 / 47 expected process /
network / DNS events with 66 / 394 / 47 lost. After `logman stop SyntracConsole -ets` and nothing else, the same probe
captured 64 / 224 / 47 with none lost, and the installed service, WITHOUT a restart, went from 0 received to ~5 events
per second. The service's own code and session were never at fault, and restarting its session would not have helped,
so the restart-on-silence watchdog is a safety net, not the fix for this case.

Not understood: the exact kernel mechanism by which one non-draining session starves the others. The cause is
established by a single-variable before/after test, not by mechanism.

Open: how the orphan came to exist (a console run that was ended abnormally, probably by the shutdown watchdog or a
kill), and how to stop it recurring. Proposed: at ETW start, look for other `Syntrac*` sessions; when no other
syntrac-service process is running, a leftover `SyntracConsole` session is certainly an orphan and can be stopped
(logged), otherwise only warn. `scripts/dev/etw-probe.ps1` now shows buffers and losses for every session and tests
each provider on its own.

### Orphan guard (2026-11-12)

Before a collector starts its trace, it now clears SYNTRAC's own leftover trace sessions (`syntrac_collector::orphan_guard`,
12 tests): a known collector session (`SyntracCollector` / `SyntracConsole`) other than its own is stopped only when no
other `syntrac-service` process is running; any other `Syntrac*` session is only reported with the `logman` command to stop
it; sessions of other programs are never touched; failing to list or stop never prevents the collector from starting.
Windows access is in `trace_sessions` (QueryAllTracesW, stop by name with a timeout, process check); the dev tool
`cargo run -p syntrac-collector --example list_trace_sessions` shows what it sees and would do, without stopping anything.
Unelevated it sees 0 sessions (no error), so the name-parsing path is verified only by an elevated run. The smoke test now
checks that the guard ran and could list sessions under SYSTEM.

## Overhead and storage (2026-11-12 22:5x, installed service, new build)

`scripts/dev/measure-overhead.ps1` (no elevation; performance counters + the collector's own status). Full run, 30 s idle
then 60 s of synthetic load (20 processes/s + 50 loopback connections/s), results in `aiChangeLog/overhead-20261112-225434.txt`:

| | collector CPU (share of whole machine) | memory | ETW events/s | dropped |
|---|---|---|---|---|
| idle | 0.01% avg, 0.18% max | 34 MB | 60 | 0 |
| load | 0.05% avg, 0.18% max | 34 MB | 409 | 0 |

The collector's cost is negligible and it keeps up with ~7x the idle event rate with zero drops. (A first short run
exposed a bug in the load generator itself, fixed: an unaccepted listener backlog made Windows refuse connections.)

**Storage is the real cost, and a risk.** Idle growth measured at ~52 MB/hour, and the real database independently implies
~50 MB/hour (55 MB for ~1 h of live data: 83k events, 30k flow rows, 39k process-metric rows). That is ~1.2 GB/day and
~37 GB at the default 30-day retention. Only ~15 MB of the 55 MB is event payload. The rest: the `flows` table duplicates
every network flow already stored as JSON in `events`; 36-character text UUIDs are repeated in every row and in three
indexes; one `process_exited` row carries a 78-byte payload but full row and index overhead. Options for a plan (none
applied; a schema change needs a migration and your approval): shorter default retention (7-14 days), UUIDs as 16-byte
blobs, drop the duplicated flow payload from `events`, roll old detail up into hourly aggregates (Phase 2), measure per-table
size with a real analyser before choosing.
