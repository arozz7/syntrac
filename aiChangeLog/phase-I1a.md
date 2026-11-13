# Phase I1a: executable identity, core and store

Implements I1a of `docs/IDENTITY-PLAN.md` (approved 2026-11-13). No Windows code yet: the adapter that reads files is I1b.

## What changed

**Core** (pure, no I/O):
- `exe_identity.rs`: the facts measured from a file (`FileFacts`: size and time, SHA-256 as hex, `Signature` with status, signer,
  thumbprint and kind, `VersionInfo`, mark-of-the-web zone), `IdentityRecord` (facts plus when, the last attempt and a failure
  with its attempt count; a later failure keeps the last good facts), `IdentityEntry`, and the ports `FileInspector` (cheap
  `stamp`, full `inspect`) and `IdentityRepository` (`identity_entries`, `save_identity`, `identity`).
- `identity_run.rs`: `is_due`, `retry_after` (1 h, 6 h, 1 day, then weekly), and the bounded pass `inspect_due`. Never-looked-at
  executables first (most recently seen first); a recheck needs the program to have RUN since the last look and a day to have
  passed; a recheck reads the whole file only if size or time changed; a file that cannot be read is recorded and does not stop
  the pass.
- `exe_location.rs`: `classify_location`, `install_root`, `product_key` from the path (and signer or company) alone.

**Stores**: schema v6, table `executable_identity` (additive, no backup, permanent). In-memory (`memory_identity.rs`) and SQLite
(`identities.rs`) implementations; `FakeInspector` in the testkit.

## Tests (written first)
- Core `tests/exe_location.rs` (14: locations, prefixes, install roots, the four Acme programs as one product, an unsigned
  file beside signed ones is not folded in) and `tests/identity_run.rs` (6: back-off, due rules).
- Contract `identity_contract.rs` (13 scenarios run against both stores: empty database, listing without the unattributed row,
  inspect and save, cap and order, unchanged file not read again, a program that has not run is not rechecked, changed file read
  again, failure and back-off, failure keeps facts and success clears it, one failing file does not stop the rest, unknown
  executable refused, survives the purge, case-insensitive paths, every field round-trips).
- Migration v6 (3: additive, offered at once, idempotent).

## Deviations from the plan (recorded)
- The location class is NOT stored (the plan listed it in the table): it is derived from the path on demand, so it cannot go
  stale and the rules can change without a migration.
- The plan said a repository would "list executables that need inspection". The repository lists ALL executables with their
  identity and ledger; the due-rule is one pure function (`is_due`) so both stores agree by construction.

## Not in I1a
The Windows adapter (signature, version resource, hash with `sha2`, mark of the web), the service worker and backfill (I1b);
the UI (I2); known programs and the pipe write commands (I3).
