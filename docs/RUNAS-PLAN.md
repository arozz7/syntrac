# Run-as account, services and launch records (I4): implementation plan

Status: **I4a APPROVED 2026-11-15** with the decisions at the end (polling worker, 30 seconds, account names stored locally), and
**built the same day** (`aiChangeLog/phase-I4a.md`; measured overhead about 0.05% of one core). I4b and I4c still need their own addenda. I4 is the item `docs/IDENTITY-PLAN.md` deferred ("own plan later"). It is split into three slices
so each can be approved, built and installed on its own.

## Goal

Add the two facts that the file alone cannot tell us and that a rogue program finds hardest to fake together: **what account a
program runs as** (SYSTEM, a service account, you) and **how it gets started** (a Windows service, a scheduled task, a Run key, the
Startup folder, an installer record). Shown next to the signature as evidence, never as a verdict. Example: the Acme backup
runs as the service `acmeserv` under SYSTEM from `C:\Program Files (x86)\Acme`; a program imitating it from your Downloads folder
runs as you and is not a service. Each signal alone proves little; together they make an imitation visibly different.

## What exists today

- Per executable (`executables`, schema v5): first and last seen; identity (`executable_identity`, v6): signature, version info, hash,
  location class; known products (v7); names (v8). Everything keys on the executable's lower-cased path, which is the right key here
  too: an account or a service is a property of "this program", and the same program can run as more than one account.
- A worker on its own thread and database connection (`identity_worker.rs`), configured from `config/service.toml`, with a pure
  scheduling core in `syntrac-core` and a Windows adapter in `syntrac-collector`. I4a follows that pattern exactly.
- Not collected: the account, the service name, any launch record. `ProcessRecord` has pid, parent, path, command line, times.

## Design decision: a worker, not a change to the event schema

`IDENTITY-PLAN.md` said I4 "needs a collector and event-schema change". On reflection it does not, and avoiding it is safer: the
ETW hot path stays untouched, no event payload changes (so no migration of the 600,000-row `events` table and no reprocessing of
stored events), and a failure here cannot disturb collection. The cost is that a program that runs for less than one polling
interval can be missed. That is acceptable because the account of a program is almost always the same every time it runs, so the
first observation of a long-lived or recurring program is what matters; the screen says "seen running as", with when and how often,
never "always runs as".

## Slice I4a: run-as account and service name (permanent per executable)

**Data (schema v9, additive).**
- `executable_accounts (exe_id, sid, account_name, account_kind, first_seen, last_seen, PRIMARY KEY (exe_id, sid)) WITHOUT ROWID`: one
  row per (program, account) pair ever observed. `account_kind` is `system`, `local_service`, `network_service`, `service_account`
  (`NT SERVICE\...`), `user` (an ordinary account of this machine or domain), or `other`.
- `executable_services (exe_id, service_name, display_name, first_seen, last_seen, PRIMARY KEY (exe_id, service_name)) WITHOUT
  ROWID`: the services a program has been seen hosting. One shared host can hold many (the Windows service host runs dozens), so
  this is a set, never a single name.
- Only first and last seen are stored (MIN and MAX, as the ledgers and names do), so observing twice changes nothing. Permanent: the
  purge of detail never touches them.

**Core (pure, in `syntrac-core`).**
- `exe_context.rs`: `classify_sid` (well-known SIDs: S-1-5-18 SYSTEM, -19 Local Service, -20 Network Service, S-1-5-80-... service
  accounts, S-1-5-21-... users; anything else `other`), `RunAs`, `ContextObservation`, the `ProcessContextSource` port (the Windows
  adapter and a test fake) and `ContextRepository` (record observations, read one program's accounts and services).
- `context_run.rs`: one bounded pass: ask the source for what is running now, drop observations with no usable path, cap the number
  recorded per pass, record the rest in one transaction.

**Windows adapter (`syntrac-collector/src/process_context.rs`).** One enumeration of running processes (the Toolhelp snapshot the
startup snapshot already uses), then for each: open it with the least right that works (`PROCESS_QUERY_LIMITED_INFORMATION`), read
its token's user (`OpenProcessToken`, `GetTokenInformation(TokenUser)`), turn the SID into text and a name (`LookupAccountSidW`). The
service map comes from one `EnumServicesStatusExW` call (running services with their process ids). A process that cannot be opened
(protected system processes) is counted as unreadable and skipped, never an error and never a retry storm. No new network use, no
writes, no process is touched beyond a query.

**Overhead (the user asked: "does 30 seconds cause too much overhead?").** Three measures keep it negligible, and the cost is
measured before the slice is called done (see the rehearsal below).
- One pass is one process snapshot, one token read per process and one service listing. Looking a SID up to a name is the only
  call that can be slow (a domain account may be asked of a domain controller), so each SID is resolved once and kept in memory.
- A program that is seen again is NOT written again every pass. The worker remembers what it has stored and writes a program only
  when it is new to it, when its set of services changed, or when ten minutes have passed (to move "last seen"). With 400 running
  programs that turns about 1.1 million row updates a day into a few thousand.
- It runs on its own thread with its own database connection, so it can never delay draining ETW, and a failed pass is skipped.

**Service worker (`context_worker.rs`).** A thread of its own with its own database connection, like the identity worker. Polls every
`context_interval_secs` (default 30). Stops promptly on a stop request. Configured by `context_*` keys in `config/service.toml`
(enabled, interval, per-pass cap). A failed pass is logged with a stable message and retried at the next interval.

**App.** The identity panel gets **Runs as** ("SYSTEM, seen 14 times between 12 Nov and today") and **Service** ("acmeserv: Acme
Service") facts, with plain hints: a service running as SYSTEM from Program Files is ordinary for system software; a program in a
temporary or download folder running as SYSTEM is worth a look. The Programs table gets a quiet badge only for the services case
(Service). No loud colour: this is evidence. When nothing has been observed yet it says "not observed yet", not "none".

**Tests first.**
- Core: `classify_sid` against well-known and made-up SIDs (S-1-5-18, -19, -20, an `S-1-5-80-` service SID, an `S-1-5-21-` user
  SID, malformed text, empty); a pass with a `FakeProcessContextSource` (new pair recorded, repeat changes nothing and only moves
  last seen, unusable path skipped, per-pass cap, an unreadable process counted, a source error does not stop the loop).
- Contract suite for `ContextRepository` run against the in-memory and SQLite stores; migration v9 test on a copy of a v8 database.
- Service: the worker records observations, stops on request mid-pass, survives a failing source (like `identity_worker.rs`).
- Adapter: runs on this machine and reports the current process with its real account (the test accepts SYSTEM or a user, because the
  machine that runs it may be either) and a service count that is not negative; a pid that does not exist is unreadable, not a panic.
- App backend and frontend: the identity panel with fake data (observed, never observed, several accounts, a shared service host).
- Rehearsal: the adapter run on this machine unelevated and elevated (counts: readable, unreadable, services), then on a copy of the
  live database; timings of a pass.

## Slice I4b: how it gets started (launch records)

Scheduled tasks (the task files under `System32\Tasks`, readable by SYSTEM), the Run and RunOnce registry keys (machine, user and
the 32-bit view), the Startup folders, and auto-start services that are not running, all read-only, mapped to the executable they
launch (quotes, arguments, `%ProgramFiles%`-style variables, and proxies such as `rundll32` handled and tested). Stored as
`executable_launchers (exe_id, kind, location, first_seen, last_seen)`. Polled slowly (every 10 minutes: this changes rarely). Shown
as **Starts with Windows via: ...**. Its own plan addendum before code, because parsing task files and registry values is where
the surprises are, and because a new autostart entry is the start of a Phase 3 alert ("something new will start at every boot").

## Slice I4c: install records

The Uninstall registry keys (display name, publisher, install location, uninstall command) as extra evidence for product grouping:
a folder that an installer registered as "Acme" by "Acme Software, Inc.". Forgeable by anything that can write the registry, so
it is shown as a clue and never replaces the signature. Own plan addendum before code.

## Privacy and safety

- The account name contains your Windows user name. It stays in the local database like every other fact; nothing is sent anywhere.
- The SYSTEM service opens other processes only to query them (limited-information right), and never writes or injects. Strings read
  from the system (account names, service display names) go through the same sanitiser as strings from files (control and direction
  characters removed, length capped), and the number of rows per pass is capped. A line is added to `docs/THREAT-MODEL.md`.
- Known blind spot, stated in the screen: protected processes cannot be read, so they have no account; and a program that starts and
  ends between two polls is not seen. Both are counted and logged, not hidden.

## Documentation and change log

Per slice: `aiChangeLog/phase-I4a.md` (and b, c), `docs/SCHEMA.md` (v9), `docs/ARCHITECTURE.md`, `docs/THREAT-MODEL.md`,
`docs/IDENTITY-PLAN.md` (I4 status), `README.md` if the configuration changes. Each slice ends with `scripts/dev/verify.ps1` green, a
rehearsal on a COPY of the live database, and a commit. Installing needs a service reinstall (done by you), and the app and service
must both be on the new schema.

## Not in this plan

Using the account in lookalike rules or baselines ("a known program running as a different account than usual"): a natural next
step for Phase 3, and it needs the account history this plan starts collecting. Per-process-instance accounts (the account is per
program here). Token elevation level and integrity level. Network use of any kind.

## Decisions I need from you

Resolved 2026-11-15:
1. **I4a approved** as written (a polling worker, no change to the event schema). I4b and I4c wait for their own addenda.
2. **Poll interval 30 seconds**, with the user's question about overhead answered by the three measures above and a measurement.
3. **Storing the account name locally: yes** (it stays on the desktop; nothing is sent anywhere).
