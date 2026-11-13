# Phase 01b: Service, permissions and UI

Date: 2026-11-13. Status: **checkpoint 1 built and offline-verified; waiting for the owner's service smoke run. The
named pipe and the UI come after it.**

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
## Next

After the smoke run: the named pipe (status/ping, explicit DACL, first-instance flag, remote clients rejected, request size
cap, per-connection timeout), then the UI (dashboard, process list, connection list, collector status), then the
overhead-under-load script.