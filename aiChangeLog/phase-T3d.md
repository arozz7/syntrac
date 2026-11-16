# Phase T3d: seeing the programs on the Network tab

Slice T3d of `docs/NETWORK-PLAN.md` (addendum approved 2026-11-15). Problem found by the user: the Network tab only showed a
NUMBER of programs, and the detail card that names them opened at the bottom of a long page, where it was not noticed. Its
program names were also plain text (the plan had promised links; they were not built).

## What changed
- **Names in the row.** Each destination row names its three biggest programs by traffic (each a link to that program's history;
  the unidentified bucket is plain text) and "+N more", which opens the destination. New destinations: the program that was first
  to talk to an address is a link as well.
- **A pop-up for the detail.** `components/Dialog.tsx` (labelled modal dialog; Escape, Close and a click outside close it;
  focus moves in and goes back to what opened it; the body scrolls inside). `views/NetworkDialog.tsx` stays one dialog while its
  content changes: a destination, or a program reached from a row or from the destination (with "Back to <address>").
  `ProgramDetail` was split into `ProgramDetailBody` (no frame, used in the pop-up) and the old framed `ProgramDetail`
  (unchanged for the Programs tab and Known programs).
- **One read for the whole page.** `DestinationTrendRepository::programs_of_destinations(addrs, from, to)` (in-memory and
  SQLite, contract-tested in both) reads the programs of every listed address at once through `idx_rollup_traffic_addr` (plan
  test); `top_destination_programs` keeps the biggest and counts the rest. `DestinationRowView` gained `topPrograms` and
  `morePrograms`. No schema change: only the app needs rebuilding, no service reinstall.

## Tests (written first)
Core `destination_trend.rs` (+2), contract `destination_contract.rs` (+1, both stores), store plan test (+1), app
`history_network.rs` (+2), frontend `Dialog.test.tsx` (3) and `NetworkHistory.test.tsx` (+7, 3 updated for the dialog).

## Rehearsal (copy of the live database, `examples/rehearse_network.rs`, copy at `<scratch folder>\network-t3d`, to be deleted by the user)
Ranking of 25 rows including the new program read: 9 to 11 ms over 7 days and over all history (71 hours of data), new
destinations 1 ms.

## Not in this slice
The opposite direction ("where does this program connect", in the program's history): idea only, ask if wanted.
