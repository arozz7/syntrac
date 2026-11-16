# Run-as account, services and launch records (I4): implementation plan

Status: **I4a APPROVED 2026-11-15** with the decisions at the end (polling worker, 30 seconds, account names stored locally), and
**built the same day** (`aiChangeLog/phase-I4a.md`; measured overhead about 0.05% of one core). I4b: **approved and built 2026-11-15** (`aiChangeLog/phase-I4b.md`, with its deviations). I4c: **approved and built 2026-11-15** (`aiChangeLog/phase-I4c.md`). I4 is the item `docs/IDENTITY-PLAN.md` deferred ("own plan later"). It is split into three slices
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

## Addendum 2026-11-15: I4b and I4c in detail (waiting for approval)

You asked for these to be planned. They share one design so the second is small: a **system records worker** that, every 10
minutes, asks a list of read-only *record sources* what the machine is configured to start or has installed, and writes what it
finds. I4b adds the launch sources, I4c adds the install source. Like I4a: own thread, own database connection, bounded work, a
failed source is logged and skipped (the others still run), switchable off in `config/service.toml`
(`records_enabled`, `records_interval_minutes = 10`). Pure rules in `syntrac-core`, Windows reads in `syntrac-collector`, fakes and
a shared contract suite in `syntrac-testkit`.

What this machine holds today (counts only, read-only): 107 automatic services; Run keys 8 (machine), 5 (32-bit view), 9 (this
user), RunOnce 2; one Startup-folder entry for all users and one for this user; Uninstall keys 331 (64-bit), 387 (32-bit) and 25 (this
user). Small enough that a full sweep is milliseconds to tens of milliseconds, to be measured.

### I4b: how a program gets started

**Sources (all read-only):**
1. **Services** (SCM): every service's start type and its command line (`QueryServiceConfigW`). Automatic and delayed-automatic
   count as "starts with Windows"; manual and disabled are kept as facts but quiet. A shared host (`svchost -k group`) is recorded
   against `svchost.exe` with the service name (I4a already records which services a running host carries).
2. **Scheduled tasks**: the task definition files under `System32\Tasks` (XML, UTF-16), from each `<Exec><Command>` and
   `<Arguments>`, with the trigger kinds (boot, logon, time, event, idle...). The service runs as SYSTEM and can read them; **my
   unelevated check of that folder read zero files, so the file format is confirmed first by an elevated probe on this machine
   (step 0 below) before any parser is written**, and the Task Scheduler's own COM interface is the fallback if the files prove
   awkward.
3. **Run and RunOnce keys**: machine (64-bit and the 32-bit view) and each **loaded** user hive under `HKEY_USERS` (the service's
   own `HKCU` is SYSTEM's, which is not yours). Users who are not signed in have unloaded hives and are not read; said on screen.
4. **Startup folders**: the all-users folder and each profile's folder; a `.lnk` shortcut is resolved to its target with the shell
   link interface, an `.exe` is taken as is.

**Mapping an entry to a program (pure, tested to death, because this is where the surprises are).** `launch_target(command,
expand, exists)`: quotes; an unquoted path with spaces (`C:\Program Files\X\x.exe /s`: try each prefix ending in `.exe`, and use
the file-exists port to pick the longest real one); environment variables (`%ProgramFiles%`, `%SystemRoot%`, `%windir%`;
`%USERPROFILE%` and `%APPDATA%` are expanded per the profile that owns the entry); `\??\` and `\\?\` prefixes; `System32` path
spelling; **proxies**: `rundll32`, `cmd /c`, `powershell`, `wscript`, `cscript`, `mshta`, `msiexec` are recorded as launching the
proxy itself, with the arguments shown (sanitised, length capped), because that is what really runs; we do not guess what the
proxy then loads. Anything that cannot be resolved is counted and kept as "unresolved: <text>" under the entry's location, never
dropped silently.

**Data (schema: next free version, additive).**
- `executable_launchers (exe_id, kind, location, detail, first_seen, last_seen, PRIMARY KEY (exe_id, kind, location)) WITHOUT
  ROWID`. `kind`: `service_auto`, `service_other`, `task`, `run_key`, `run_once`, `startup_folder`. `location`: the service name,
  the task path, the key and value name, the shortcut path. `detail`: trigger or arguments (sanitised, capped).
- `launcher_sweeps (kind PRIMARY KEY, completed_at)`: when each kind was last read **completely**. "Still configured" is
  `last_seen >= completed_at` for its kind; a source that failed this time does not move `completed_at`, so nothing is wrongly
  shown as removed. Only first and last seen are kept, as everywhere, so a repeat sweep writes nothing new except `last_seen` (the
  worker writes a changed or new entry at once, an unchanged one only at the refresh time, like I4a).
- Permanent; the purge of detail never touches them.

**App.** Identity panel: **Starts with Windows via** (service, scheduled task "<name>", Run key, Startup folder; first seen on the
machine; "no longer configured" when removed). Programs table: a quiet badge "Starts at boot" for automatic services, tasks and
Run keys only. A program with no launch record says "No start-up entry found", with the stated blind spots (below), never "does not
autostart".

**Honest blind spots, stated on screen and in the docs.** Not read: Winlogon `Shell`/`Userinit`, Image File Execution Options,
AppInit and other DLL hooks, WMI event subscriptions, COM hijacks, drivers, unloaded user hives, programs launched by another
program. So "no start-up entry found" means "not found in the places we read".

**Tests first.** `launch_target` (about 40 cases: every item above plus empty, only quotes, a trailing backslash, a 4,000-character
value, control and direction characters, mixed case, forward slashes); task-XML parsing from fixtures (one action, several actions,
no Exec action such as COM-handler tasks, disabled task, UTF-16 with and without a byte-order mark, malformed XML counted not
fatal); registry and folder sources against fakes; contract suite for the repository in both stores; migration test on a copy of the
live database; `launcher_sweeps` semantics (a failed source keeps old state); worker stops promptly and survives a failing source;
app and frontend with fake data (service, task, several, none, removed). Adapter tests run on this machine and assert only
machine-independent facts (it reads without error; counts are not negative; a made-up task folder with fixtures parses).

**Step 0 (before code, needs you):** an elevated probe prints counts and one anonymised sample of each source, so the parser is
built from real files. I will give you the one command to run in an elevated shell; it writes nothing.

### I4c: install records

**Source:** the Uninstall registry keys (machine 64-bit, 32-bit view, and each loaded user hive): `DisplayName`, `Publisher`,
`DisplayVersion`, `InstallLocation`, `DisplayIcon`, `UninstallString`, `InstallDate`, `SystemComponent`, `WindowsInstaller`.

**Data.** `install_records (id, hive, key_name, display_name, publisher, version, location, install_date, system_component,
first_seen, last_seen)`, unique on `(hive, key_name)`, `location` lower-cased and normalised. When `InstallLocation` is empty the
folder is derived from `DisplayIcon` or `UninstallString` and marked `derived`, because that is weaker evidence. Not stored per
program: one product has many executables, so matching is done **when shown**.

**Matching (pure, tested).** A program matches the record with the **longest install folder that contains the program's path**.
Guards: ignore folders that are too general (a drive root, `Program Files`, `Windows`, `System32`, a profile folder, `Temp`),
ignore records with no usable folder. Several equally specific records are all shown, not one picked silently.

**Use and honesty.** Shown in the identity panel as **"Registered by an installer as 'X' from 'Y' (version, date)"**, labelled a
clue: anything able to write the registry can claim any name, so it never replaces the signature and never changes a standing. One
good use: a program with no signer that lives in a folder an installer registered gives product grouping a name where the
version resource is empty. It is offered to the existing product grouping only as a **fallback label**, below signer and version
information, and never merges two products by itself.

**Tests first.** Parsing from a fake registry (missing values, wrong types, a value of 10,000 characters, control characters, a
duplicate key in both views), folder derivation, the matching guards and longest-prefix rule, ties, the product-grouping fallback
order, contract and migration tests, worker survival, frontend with fake data.

### Shared

- **Order proposed:** T3d (Network screen), T3c (owner data), then I4b, then I4c. Each is its own commit and its own reinstall.
- **Overhead:** one sweep every 10 minutes reads a few hundred small registry keys and files; I expect a few tens of milliseconds
  per sweep (about 0.005% of one core) and measure it, together with the database growth, before calling the slice done, as for
  I4a. The writes are first-seen only plus a refresh of `last_seen`.
- **Privacy:** the records name your programs and your user profile paths; they stay in the local database. Strings from the
  system go through the existing sanitiser. Nothing is written to the system, no key is created, no task is started.
- **Documentation:** `aiChangeLog/phase-I4b.md` and `phase-I4c.md`, `docs/SCHEMA.md`, `ARCHITECTURE.md`, `THREAT-MODEL.md` (the
  service reads the Task Scheduler folder and other users' hives), `IDENTITY-PLAN.md`, `README.md` for the new config keys.
- **Future (Phase 3, not planned here):** "something new will start at every boot" is an alert that can use `first_seen` of a launch
  entry, and "a known program started a new way" can use these records with the known-programs list.

### Decisions I need from you (I4b and I4c)

1. Approve the **shared records worker** design, I4b first and I4c after it.
2. Approve the **blind-spot list** as the honest scope of I4b (the places not read).
3. OK for the **elevated probe** (step 0): one command you run in an elevated shell, read-only, before I write the task parser.
4. Reading **other users' loaded hives** under `HKEY_USERS` is a new capability of the service (read-only); OK?
