# Phase I3: known programs and the pipe's first write commands

Implements I3 of `docs/IDENTITY-PLAN.md`; design and decisions in `docs/KNOWN-PROGRAMS-DESIGN.md` (written first, as promised).

## What changed

**Core** (pure): `known.rs`: `KnownProduct`, `NewKnownProduct`, the `KnownProductRepository` port, `clean_label` / `clean_note`,
`new_known_product` (builds a product from the identity record the service measured; refuses unread, unreadable and altered files),
`KnownIndex` and `Standing` (`Known`, `Lookalike` with reasons, `Unlisted`). Lookalike rules: a program names a known product in its
version information (unless it is in the same folder and validly signed: Windows signs its own files under two names), or it has the
file name of a known program and no valid signature (a valid signature by someone else on a generic name such as `setup.exe` is not
flagged). A file that has not been read is never judged.

**Stores**: schema v7, table `known_products` (additive, permanent); in-memory and SQLite implementations pass one contract suite.

**Pipe** (`syntrac-ipc`): `Request::KnownProductAdd { path, label, note }`, `KnownProductRemove { id }`; `Response::KnownProductSaved { id }`,
`Done`. **Service**: `known_admin.rs` (`KnownProductsAdmin`: validates path length, builds the product from the stored identity, stores it,
logs each change; `ServiceHandler`: ping and status go to the status board). The host gives the pipe its own database connection; if it
cannot open one the pipe still answers status.

**App**: `known_view.rs` (`StandingView`, `known_programs`), `Identities` now carries the known index; ranking rows, product rows, members and
one program's history carry `standing`; commands `get_known_programs`, `mark_known_program`, `unmark_known_program` (the app sends
write requests to the service and shows its refusal in its own words; "collector not running" is said plainly).

**Frontend**: a "Known: label" badge and a loud "Looks like label" badge in the Programs table; in the drill-down a "Mark as known" form (label
suggested from the product name, optional note, the service's refusal shown, cancel) or "Known program: label" or an explained lookalike
warning; a History > Known programs tab (products with their programs, note and date, removal with a question first; the lookalikes with
why).

## Tests (written first)
Core `known.rs` (23). Contract `known_contract.rs` on both stores (7) and migration v7 (2). Protocol (6 new). Service handler `known_admin.rs` (11: every
refusal, update on repeat, case-insensitive paths, a storage failure is reported) and `known_over_pipe.rs` (2: a real pipe, a real SQLite file read by
a second connection). App `known_view.rs` (10). Frontend `ProgramsKnown.test.tsx` (12) and `KnownPrograms.test.tsx` (8). Existing exhaustive
`match`es on `Request` in test handlers gained a catch-all.

## Rehearsal on a copy of the live database (identities read, migrated to v7)
- First version of the rules: after marking Windows, a browser, a backup tool and Git as known, **31 genuine Microsoft files appeared as lookalikes**
  (they carry the product name "Microsoft Windows Operating System" but sit in SysWOW64, SystemApps, Defender under ProgramData, with
  other Microsoft signer names). That would have made the feature useless. Fix: a valid signature by the same MAKER (first significant
  word of the signer, ignoring "the") is not a lookalike; an altered or untrusted file, and a different maker, still are. Tests added
  first; afterwards the same real data gives **0 lookalikes** with the four products known. Acme folds 7 programs, Windows 93, Git 25.
- The detection itself (an unsigned `acmeserv.exe` in a temp folder, an imposter claiming a product name) is proven by the unit and view tests,
  not by the real data, which has no such file.

## Deviations and things to know
- The request carries the PATH, not the product key: a client cannot invent "signed by Acme". (Design decision 1.)
- For an unsigned product with no company, "known" covers the unsigned files of its folder; the form says it confirms "the same maker in the same
  folder".
- Residual risk, unchanged and accepted: a program running as the installing user can send these commands and mark itself known. It cannot forge a
  signer, and an altered file is refused. Recorded in the threat model.
- Not verified: the screens in the running app. Nothing is installed yet: the service is still on schema v5, and this commit takes it to v7 in one step
  (the I1b notes on reinstalling the service and rebuilding the app together apply, once).

## Not in I3
Pre-seeding Acme (mark it from the screen), baselines and alerts (Phase 3), hash-pinning of unsigned products.
