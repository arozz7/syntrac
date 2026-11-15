# Phase I4a: run-as account and services (per executable, schema v9)

Slice I4a of `docs/RUNAS-PLAN.md` (approved 2026-11-15: a polling worker, 30 seconds, account names stored locally). Adds the two
facts a file cannot tell us: which account a program has been SEEN running as, and which Windows services it hosts. Evidence, never
a verdict.

## What changed

**Core** (pure): `exe_context.rs`: `AccountKind` and `classify_sid` (S-1-5-18 SYSTEM, -19 Local Service, -20 Network Service,
S-1-5-80-... service accounts, S-1-5-21-... users, everything else and malformed text `Other`), the `ProcessContextSource` port,
`merge_snapshot` (instances of one program under one account become one observation, services added together, path spelling =
smallest, no-path or no-SID processes skipped and counted), `ContextTracker` (a program is written when new, when its services
changed, or after the refresh time; something never marked written stays due, so a failed write is retried), and the
`ContextRepository` port. `context_run.rs`: `observe_context`, one bounded pass.

**Stores**: in-memory (`memory_context.rs`) and SQLite (`contexts.rs`). Migration 9 (additive, no backup): `executable_accounts` and
`executable_services`, first and last seen only (MIN and MAX), permanent. A program the rollups have not met yet gets its
`executables` row from here, with the spelling first seen. Contract suite `context_contract.rs` runs against both.

**Windows adapter** (`syntrac-collector/src/process_context.rs`): one Toolhelp snapshot, one token read per process (opened with
`PROCESS_QUERY_LIMITED_INFORMATION` and `TOKEN_QUERY`, query only), one `EnumServicesStatusExW` listing, SIDs resolved to names once
and cached; text from Windows goes through the existing sanitiser. A process that cannot be opened is counted, never an error.

**Service**: `context_worker.rs` on its own thread with its own connection, wired into the host like the identity worker; config
keys `context_enabled`, `context_interval_secs` (30), `context_refresh_minutes` (10) in `config/service.toml`.

**App**: the identity badge carries `runsAs` (account kinds, most recent first) and `service`; the identity detail carries the
accounts (name, kind, first and last seen) and services. Programs history shows a quiet "Service" badge; the identity panel shows
"Runs as" (or "Not observed running yet", never "none"), "Services", and a "worth a look" note only for a system or service account
running from a temporary folder, a user folder or a network share. `accountConcern` is wording for the screen, not a rule that
flags anything.

## Tests (written first)
Core `exe_context.rs` (20); testkit `context_run.rs` (9, pass logic with a fake source), contract `context_contract.rs` (10 scenarios,
both stores); store `migration_v9_tests.rs` (2); collector `process_context.rs` (5, against the machine running the tests);
service `context_worker.rs` (5) and config (2); app `context_view.rs` (6); frontend `ProgramsContext.test.tsx` (7) and
`identityLabels.test.ts` (+4).

## Overhead (the user asked: "does 30 seconds cause too much overhead?")
- Unelevated probe (`examples/probe_context.rs`): about 14 ms of CPU per pass, 237 to 264 processes readable, 171 not openable:
  0.047% of ONE core at one pass every 30 s.
- The installed service (SYSTEM, running this build): about 167 programs per pass, only 4 processes unreadable. Collector status
  lines show the whole collector (ETW, storage, rollups, both workers) at 0.09 to 0.11% of the machine, so the worker is a small part
  of it.
- Writes: after the first pass a program is written only when new or changed or every 10 minutes; the log shows passes writing 1 to 6
  programs, and the ten-minute refresh writing all of them (about 158) in one transaction.

## Rehearsal (copy of the live database, `examples/rehearse_context.rs`, copy at `<scratch folder>\context`, to be deleted by the user)
Upgrade to v9 in 0.02 s. Two real passes with the real adapter: the first 36 ms, the second 23 ms and writing nothing. The live
database (already running this build) holds 200 account rows over 189 programs: system 63, user 120, local service 9, network
service 3, service account 1, other 4; and 160 service rows over 45 programs (Acme `acmeserv.exe` as SYSTEM, hosting `acmeserv`).

## Limits (as designed)
- A program that runs for less than one interval can be missed. "Seen running as" is not "always runs as".
- Protected processes cannot be opened and have no account (4 under SYSTEM today).
- The account name contains the Windows user name and stays in the local database.
- I4b (launch records) and I4c (install records) are not started and need their own plan addenda.
