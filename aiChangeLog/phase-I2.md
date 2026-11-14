# Phase I2: show executable identity in the Programs history

Implements I2 of `docs/IDENTITY-PLAN.md`. Identity is shown as evidence, never as a verdict.

## What changed

**Core** (pure): `program_group.rs`: `group_programs` folds the programs of one product (key from `product_key`) into one row and
`rank_groups` ranks the rows. What can be added exactly is added (CPU averages share the same samples, starts, bytes, flows); a peak
is the highest peak of any member, distinct destinations the largest member's count, "recorded in" the longest member's; first and
last seen are widened. `trend::all_programs` returns every program of the sums (with ledger) for callers that fold before ranking;
`ProgramSummary::value` is public.

**App backend** (`app/src-tauri`): `identity_view.rs` (`badge`, `detail`, `merged_badge`, `product_name`, `Identities`: all identity
records read once per request). Ranking rows carry `identity` (state `pending` / `known` / `unreadable`, signature, publisher,
product, location) and `members`; one program's history carries the full `identity` detail (signer, kind, thumbprint, company,
description, version, original file name, SHA-256, size, dates, download mark, install folder, last failure). `Grouping`
(`None` / `ByProduct`) is a new argument of the ranking; the command takes `groupByProduct`. Rules: a program whose file has not been
read is not folded by folder (a shared folder alone is a guess, and the row would come apart as files are read); two product rows with
the same name are told apart by their publisher (or folder). A product row has no path (nothing to drill into); a product with one
program is shown as that program.

**Frontend**: `identityLabels.ts` (the words: Signed, Signed, not trusted, Changed since signed, Unsigned, Not yet checked, Could not
read the file; a note only for temporary, user and network folders; `wasRenamed`), `IdentityBadge.tsx` (badge and table cell, with
tooltips saying what each signal can and cannot prove, and whether the publisher is a signer or only a company name in the file),
`IdentityPanel.tsx` (the drill-down panel), Programs tab: an Identity column, a "Group by product" switch, product rows that open to
list their programs. Only "changed since signed" is loud; unsigned is quiet because most developer tools and Store apps are unsigned.

## Tests (written first)
Core `program_group.rs` (7). App `identity_view.rs` (17: badge states, detail fields, rows carry badges, grouping and naming, the
combined badge shows the worst signature, unread programs are not folded, same-name products are told apart). Frontend:
`identityLabels.test.ts` (6) and `ProgramsIdentity.test.tsx` (14: badges, honest pending and unreadable states, location notes, the
loudest label, publisher hint, group switch and product rows, the panel in each state, a different built-in name as information,
the disclaimer). 154 to 176 frontend tests in all.

## Rehearsal (a fresh copy of the live database after an identity pass, release build)
- Top by network, not grouped: `acmetransmit64.exe` 20.3 GB sent, valid, "ACME SOFTWARE, INC.". Grouped: "Acme Backup", 7 programs, valid,
  the same 20.3 GB. Ranking takes 2 to 6 ms (a 200-row grouped ranking 6 ms); reading the sums of the week takes 6 ms.
- The real data led to the three rules above. Before them: Windows programs signed under two signer names ("Microsoft Windows" for
  catalog-signed files, "Microsoft Windows Publisher" for embedded ones) appeared as two identical rows; unread programs were folded
  into folder rows such as "SysWOW64" and ".local".
- Not verified: the screens in the running app (layout, colours). The service is still on schema v5, so the new app build cannot open
  the live database until the service is reinstalled (see `phase-I1b.md`).

## Not in I2
Known programs and the "looks like X but is not signed by X" flag (I3), run-as account (I4), network destinations (T3).
