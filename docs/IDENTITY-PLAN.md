# Executable identity: implementation plan (I1-I3)

Status: **APPROVED 2026-11-13** with the decisions at the end: `sha2` for hashing; known programs stored in the database and
written through the service pipe (changed from the proposal, because a profile file could be lost or corrupted); revocation
checks off; run-as account deferred to I4.

## Goal

Answer "what is this program?" for every executable SYNTRAC has seen, from evidence the machine already holds, and show that
evidence next to the usage numbers (T2) and, later, the network destinations (T3). Origin of the request (2026-11-13): the
user's Task Manager screenshot of Acme (`C:\Program Files (x86)\Acme\acme*.exe`, some as SYSTEM, some as the user) and
the question "can the path or other information tell us what the app is, knowing a rogue app may imitate others?".

Principle: **evidence, not a verdict.** Each signal is shown with its strength; SYNTRAC never calls a program "safe". A signal
that disagrees with another (a Acme name signed by nobody, in a user-writable folder) is the interesting finding.

## What exists today

- `executables(id, path_key, path, first_seen, last_seen)`: one row per lower-cased image path (schema v5, S3). Row 1 is the
  unattributed bucket. All rollups and trend views already key on `exe_id`.
- `ProcessRecord`: pid, parent, image path, command line, start/end time, exit code. No account, no file properties.
- Not collected: signature, version resource, hash, file size and times, "downloaded from the internet" mark, run-as account.
- `docs/PLAN.md` (Phase 2) already lists SHA-256 (cached by path, size, mtime) and Authenticode via `WinVerifyTrust`. This plan
  details and sequences that item and adds the rest.

## Signals, strongest first

| Signal | Source | Offline? | What it tells us | Limit |
|---|---|---|---|---|
| Signature: signer name, certificate thumbprint, valid / invalid / unsigned, embedded or catalog | `WinVerifyTrust`, plus the catalog lookup (`CryptCATAdmin*`) because most Windows files are catalog-signed, not embedded | Yes, with revocation checks off (D3) | "Signed by Acme Software, Inc." | Proves who signed, not that it is safe; a stolen certificate passes; Microsoft-signed tools can be misused |
| Version resource: company, product, description, original filename | `GetFileVersionInfo` | Yes | A readable label; original name differing from the real name means renamed | Anyone can write any text |
| Install location class: system (Windows, System32), program files, program data, user profile (AppData, Downloads, Temp), removable/network, other | Pure rule on the path | Yes | Locations under Program Files / Windows need administrator rights to write; user-profile and Temp do not | A program with admin rights can write anywhere |
| SHA-256, file size, modified time | Read the file once | Yes | "Same path, new binary" (update or tampering); basis for any later local allowlist or threat-intel list import | Useful only with the signature; hash alone says nothing about intent |
| Mark of the Web (`Zone.Identifier` stream) | Alternate data stream of the file | Yes | Downloaded from the internet rather than installed by an installer | Removable by the file's owner; often absent for installer-written files |
| Behaviour already collected: destinations, DNS names, CPU/memory, first and last seen | Rollups, T3 | Yes | Acme should talk to Acme; consistency over time | Needs T3; cannot be forged cheaply over time |
| Run-as account (SYSTEM / service account / user) and service name | Process token, Service Control Manager | Yes | "this is the Acme Service" | Collector change; deferred (I4, own plan) |
| Install records (Uninstall registry keys) | Registry | Yes | Maps a folder to product and publisher; groups `acme*.exe` as one product | Written by installers, forgeable; deferred |

Location class and version resource are the cheap signals that cover "most programs" (the user's expectation); the signature
is the one that resists imitation.

## Design

**Interfaces (Dependency Inversion; the logic never touches Windows directly).**

- Core, pure (new module `exe_identity.rs`, name chosen because `identity.rs` is the process-instance id):
  - `LocationClass` and `classify_location(path)`; case-insensitive, handles `\\?\` prefixes, 8.3 names left as unknown.
  - `ExecutableIdentity { exe_id, size, modified, sha256, signature (status, signer, thumbprint, kind), version info, mark
    of the web, location class, inspected_at, inspect_error }`.
  - `product_key(identity)`: signer name (else company name) plus the install root folder (the first folder below Program Files,
    ProgramData, `AppData\Local\Programs`, etc.). One key groups the four Acme programs into one product.
  - `needs_inspection(known, current size and mtime)`: re-inspect only when the file changed (cache by path, size, mtime, as in
    `docs/PLAN.md`).
- Ports: `FileInspector` (path to `InspectionResult`; the Windows adapter reads the file) and `IdentityRepository` (store
  identity rows; list executables that need inspection; read identities for a set of executables). Both get an in-memory
  fake first, so the whole pipeline is tested without Windows and without disk.
- Adapter: `WindowsFileInspector` in the collector crate (signature, version info, hash, Mark of the Web). Everything it
  returns is data; it never decides.
- Service: an **identity worker**, separate from the sampling and ETW loops (a slow disk or a huge file must never delay
  collection). It wakes on a timer, asks the repository for executables without an identity or whose file changed, inspects a
  few per tick with a pause between them, and writes the results. Failures are stored (`inspect_error`, retry later with a
  back-off), never fatal.

**Storage (schema v6, additive, no rewrite, no backup needed like v5).** (Recorded deviation in I1a: location class and product are derived from the path on demand, not stored.) One new table `executable_identity` keyed by
`exe_id` (the existing dimension table), holding the fields above. Permanent like the ledgers: a few hundred rows (470
executables today), well under a megabyte per year. Detail retention and the purge do not touch it. The storage guard (S4)
continues to apply. The worker writes only a handful of rows, so the guard does not pause it; it is skipped when a write
fails (read-only database, full disk), and the failure is only logged.

**Read side.** The app opens the database read-only (as in T1/T2). Identity rows come back with the program rankings and in
the program drill-down. Where the same `exe_id` has no identity yet the UI says "not yet inspected", never "unsigned".

**Safety of the inspector (the service runs as LocalSystem; the files are not trusted).**
- Open files read-only with full sharing; do not follow reparse points for the final component; regular files only; skip
  devices, pipes and network paths unless the user opts in; an overall size cap for hashing (default 512 MB, larger files are
  recorded as "not hashed (size)"); a time limit per file.
- Signature verification with revocation checking disabled and no network retrieval, so nothing leaves the machine and a
  malformed certificate cannot make the service wait on a remote server.
- Version resource parsing uses the operating system's own loader, not our own parser; run on the worker thread with the
  time limit. A later hardening option is a low-privilege helper process (noted in `docs/THREAT-MODEL.md` as a new row).
- Signer names, company names and descriptions are attacker-influenced text: stored as data, escaped in the UI, passed to the
  LLM (Phase 4) as quoted data only.
- Hash and signature are read from the same open handle where the API allows, to narrow the swap-in-between window.

## Slices (each ends with `verify.ps1` green, a rehearsal on a COPY of the live database, and a commit)

- **I1a Core and store.** Types, `classify_location`, `product_key`, `needs_inspection`, the two ports, in-memory fakes, the
  worker loop as a pure function over the ports, migration v6, SQLite `IdentityRepository`, contract suite run against both
  stores plus a parity test. No Windows code yet.
- **I1b Windows adapter and service wiring (done; see `aiChangeLog/phase-I1b.md` for the measurements).** `WindowsFileInspector`, the worker started by the service, backfill of the
  existing executables (throttled, one pass), a rehearsal that inspects the live executable list (counts of signed, unsigned,
  catalog-signed, errors, time per file, CPU of the worker, database growth) and a check against known files (a Microsoft
  binary, the service's own binary, Acme).
- **I2 Show it (done; see `aiChangeLog/phase-I2.md`).** Programs tab: publisher, a signed / unsigned / invalid badge, location class; drill-down gets an "Identity"
  panel (signer, thumbprint, version info, hash, file size and date, first seen, downloaded-from-internet, location) with plain
  labels for what each signal can and cannot prove. Rows gain a "Group by product" switch using `product_key`.
- **I3 Known programs (done; design in `docs/KNOWN-PROGRAMS-DESIGN.md`, log in `aiChangeLog/phase-I3.md`).** A list of products the user has confirmed, each with a label, a note, and the signer and install root
  that were confirmed. SYNTRAC then marks a program as "known", and flags an executable that matches a known product's name
  but not its signer or location ("looks like Acme but is not signed by Acme Software, Inc."). Starts from the entries already
  in memory (`project-known-programs`: Acme). This list later feeds the Phase 3 "unusual for this program" baselines.
  **Storage (decision 2):** a `known_products` table in the database (schema v7, additive), so it is backed up, integrity
  checked and migrated with everything else. The UI stays read-only on the database; changes go to the service over the named
  pipe, which gains its first write commands (`known_product_set`, `known_product_remove`). The pipe's existing protections
  (access list of SYSTEM, Administrators and the installing user; 4 KiB request cap; per-connection deadline; first-instance
  flag) stay, the new commands are validated (length limits, no control characters, label and note stored as text) and are
  the only writes the pipe allows. Their own tests come first (rejected oversize or malformed requests, a non-listed caller,
  a read-only ping still works). I3 gets a short plan addendum before its code, since it changes the pipe protocol.
- **I4 (own plan: `docs/RUNAS-PLAN.md`).** Run-as account per program and service-name mapping are done (I4a, no event-schema
  change after all: a polling worker). Autostart and scheduled-task mapping (I4b) is built (schema v11); Uninstall records (I4c) are next.

## Tests first (per slice)

- Core: `classify_location` over a table of paths (Program Files, Program Files (x86), System32, `AppData\Local\Temp`,
  Downloads, `\\server\share`, `D:\` removable, `\\?\C:\...`, mixed case, trailing spaces, empty); `product_key` groups the four
  Acme executables and separates two products from the same signer in different folders; `needs_inspection` (unchanged,
  size changed, mtime changed, never inspected, previous error with back-off); the worker pass with a fake inspector (inspects
  only what is due, bounded per tick, records errors, a failing file does not stop the pass, an unchanged file is not read
  again).
- Contract suite for `IdentityRepository` against in-memory and SQLite, plus parity: insert, update on file change, listing of
  executables needing inspection, unattributed row excluded, case-insensitive path keys, identities survive the purge of detail.
- Migration v6: from a v5 copy, additive, row counts unchanged, idempotent.
- Adapter (Windows, run on the real machine in the rehearsal and as a small number of integration tests against files that
  exist on every Windows install): a catalog-signed system file reports a valid Microsoft signature; an unsigned file we create
  in a temp folder reports unsigned and the Temp location class; a file with a changed byte changes the hash; a locked file
  and a missing file give errors, not panics; a reparse point is refused.
- Frontend (I2/I3): each badge, "not yet inspected", the grouped view, the known-product flag, the mismatch flag, with fake data.

## Threat-model and docs updates

New rows in `docs/THREAT-MODEL.md`: the privileged service reads attacker-chosen paths (mitigations above); signer text is
untrusted; the pipe's first write commands (I3) widen its surface, so they are validated and capped, and a program running as
the installing user can still mark a rogue program as known through them (accepted for a personal tool; every change is
logged with its time). Update
`docs/SCHEMA.md` (v6), `docs/ARCHITECTURE.md`, `docs/PLAN.md` (Phase 2 item split into I1-I4) and write
`aiChangeLog/phase-I1a.md`, `phase-I1b.md`, and so on.

## Not in this plan

Online lookups of any kind (certificate revocation, hash reputation, VirusTotal), sending hashes anywhere, blocking or killing
programs, an "is this malware" score (Phase 3 uses these signals for rules and baselines), kernel-driver and module
(DLL) inspection, per-process (rather than per-executable) identity.

## Decisions (resolved 2026-11-13)

1. `sha2` crate for hashing. 2. Known programs in the database, written through the service pipe. 3. Revocation checks off.
4. Run-as account deferred to I4. 5. Defaults below accepted. The original options follow for the record.

1. **Hashing:** add the `sha2` crate (small, widely used, testable with known-answer vectors, no unsafe code of ours;
   recommended) or use the Windows BCrypt API (no new dependency, but more unsafe code). The threat model says "minimal
   dependency set", so this is yours to choose.
2. **Where the known-programs list lives:** a file in your own profile that the UI edits and the read side matches against
   (simple, no new service commands; recommended) or through the service over the named pipe (tamper-resistant against other
   programs running as you, but needs the pipe to gain a write command, which today it deliberately lacks).
3. **Revocation checking off** (recommended): the signature is validated offline, so SYNTRAC works with no network and nothing
   leaves the machine, at the cost of not noticing a certificate that has since been revoked. Confirm.
4. **Run-as account** deferred to I4 (own plan, needs a collector change). Confirm, or pull it forward.
5. Defaults I will use unless you say otherwise: hash size cap 512 MB; inspect at most a few files per minute during the
   one-off backfill; location classes as listed above; "Group by product" off by default.
