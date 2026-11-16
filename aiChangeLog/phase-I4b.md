# Phase I4b: how programs get started (launch records, schema v11)

Slice I4b of `docs/RUNAS-PLAN.md` (addendum approved 2026-11-15). Adds the second fact a file cannot tell us: HOW a program gets
started (a Windows service, a scheduled task, a Run key, the Startup folder). Evidence, never a verdict: "no start-up entry found"
means "not found in the places read".

## Step 0 (the elevated probe the user ran, `scripts/dev/probe-launchers.ps1`)
All 286 task files are UTF-16 LE with a byte-order mark; 124 tasks have an `Exec` command and 163 only a COM handler; 52 are
disabled; one file cannot be read even elevated (counted, not an error). Commands use `%windir%`-style variables heavily (96 of
124), 13 are unquoted paths with spaces and arguments, 13 go through a proxy (`rundll32`, `cmd`...). Run values: 22, of which 8 are
unquoted with spaces. 108 automatic services. These shapes are the test cases of `launch_target`.

## What changed
- **Core (pure).** `mini_xml.rs` (a small tolerant XML reader, bounded depth and node count, no dependency), `task_xml.rs` (a task's
  commands, triggers, enabled state; COM-handler tasks counted), `launch_target.rs` (a command line to program and arguments:
  quotes, unquoted paths with spaces settled by asking the file system, `%VAR%` expansion, `\??\` and `\SystemRoot`, bare names
  looked up in System32, proxies recorded as themselves), `launchers.rs` (kinds, the `LaunchSource` port, the
  `LauncherRepository` port), `launch_run.rs` (`sweep_launchers`: read every source, map, record; a failed source is reported and
  its kinds are not marked as read).
- **Stores.** Migration 11 (additive, no backup): `executable_launchers` (one row per program, kind and place; first and last seen
  only, MIN and MAX) and `launcher_sweeps` (when each kind was last read COMPLETELY: an entry whose last_seen is older is "no
  longer configured"). In-memory reference plus contract suite `launcher_contract.rs` run against both.
- **Windows adapters (collector).** `registry.rs` (read-only registry wrapper, also for I4c), `launch_services.rs` (service manager:
  start type and command of every service), `launch_tasks.rs` (the Task Scheduler folder; UTF-16 or UTF-8; a switched-off task and a
  COM-only task are left out; a folder that cannot be listed is an ERROR, never "no tasks"), `launch_registry.rs` (HKLM Run and
  RunOnce in both views, and each LOADED user's under HKEY_USERS with that user's profile folder), `launch_startup.rs` (all-users and
  per-profile Startup folders; shortcuts resolved through the shell's own interface), `launch_env.rs` (the service's environment
  without the per-user variables, which would name SYSTEM's folders).
- **Service.** `launch_worker.rs` on its own thread and connection, every `launch_interval_minutes` (10); config keys
  `launch_enabled` and `launch_interval_minutes` in `config/service.toml`.
- **App.** The badge carries `startsAtBoot` (an automatic service, task, Run key or Startup entry that is still there); the identity
  detail carries `launchers`. Frontend: a quiet "Starts at boot" badge, "Starts with Windows via" in the identity panel (kind,
  place, triggers, arguments, since when, "no longer configured"), "No start-up entry found in the ... that were read" when empty,
  and a note of what is not read.

## Deviations from the plan (and why)
- **Unresolved entries are counted and logged, not stored** (storing them needs a program to hang them on).
- **Every sweep writes every current entry** (about 355 rows in one transaction) instead of a throttled tracker: "no longer
  configured" is `last_seen < the kind's last complete read`, which a throttle would break. Cost is measured below.
- **Switched-off tasks are left out**, so a task that is switched off later shows as "no longer configured".

## Tests (written first)
Core: `launch_target.rs` (16), `task_xml.rs` (8). Testkit: `launcher_contract` (both stores), `launch_run.rs` (8). Store:
`migration_v11_tests.rs` (3). Collector: `launch_sources.rs` (11: task files in a temp folder, Startup folders with a fake
resolver, a real shortcut written and read back through the shell, this machine's services and registry). Service:
`launch_worker.rs` (4), config (+1). App: `launch_view.rs` (5). Frontend: `ProgramsLaunchers.test.tsx` (6),
`identityLabels.test.ts` (+2).

## Rehearsal (copy of the live database, `examples/rehearse_launchers.rs`, copy at `<scratch folder>\launchers-i4b`, to be deleted by the user)
Run UNELEVATED (the Task Scheduler folder cannot be read, so the tasks source failed as designed and the other three worked):
upgrade to v11, then two sweeps of about 190 ms each: 357 entries, 355 recorded, 2 unresolved, 1 file missing, the second sweep new
0. 116 programs have an entry, 68 of them start with Windows; 108 automatic services (the probe also counted 108), 21 Run keys, 4
RunOnce, 2 Startup entries, 220 on-demand services. At one sweep per 10 minutes that is about 0.03% of one core.
The elevated run (what the SYSTEM service sees, including the tasks) is for the user to do:
`cargo run --release -p syntrac-store --example rehearse_launchers -- D:/Syntrac/data/syntrac.db <scratch folder>/launchers-elevated`
in an elevated shell, or simply to install and look at the log.

## Limits (as designed)
Not read: Winlogon `Shell`/`Userinit`, Image File Execution Options, AppInit and other DLL hooks, WMI subscriptions, COM hijacks,
drivers, users who are not signed in, Task Manager's "disabled" startup list, programs started by another program. The service must
be reinstalled with this build (schema v11) and the app rebuilt together.
