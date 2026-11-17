# Phase F4: review, digest, new network owners, and knowing what is new (no schema change)

Slice F4 of `docs/ALERTS-PLAN.md` (approved 2026-11-16). The toast part is NOT built: it needs the Tauri notification plugin (a new dependency), which the plan says to ask about first.

## What changed
- **Review what is here today.** Core `review.rs`: `review_groups` groups the programs seen running BEFORE the switch-on by product (the maker and the install folder, the same grouping as the product view), leaves out what a known product or an expectation already covers, lists groups with something to look at first (the same evidence as the "new program" rule: unsigned in a user folder, changed after signing), then bigger groups, and labels a group by the product name its files give themselves, else the signer, else the folder. Pipe request `ReviewConfirm { root }`: the service checks the folder (not empty, not a drive root, at most 600 characters, and a program it knows has exactly that install folder) and stores the folder expectation for `new_program`; `FindingsAdmin` now also reads identities. App: `review_view` (read-only), commands `get_review` and `confirm_review`. Frontend `ReviewToday`: a collapsed card (reads nothing until opened), each group with its size, location, what is worth a look and some program names, "Mark as expected" and "Mark as a known program" (the existing pipe command, using the group's first program).
- **New network owner.** Core `new_owner.rs`: a program that is not validly signed reaching an owner (AS number) it had not reached before the switch-on; skipped for a validly signed program (digest only), for a program that is itself new (its own finding), and for a program with no earlier owner to compare with; off without the owner data. One finding per program and owner. New read `DestinationTrendRepository::new_pairs(since)` (the permanent ledger of program and address pairs) for SQLite and in-memory, with a contract test. The service now has three detectors.
- **Digest.** `digest_view`: internet addresses first seen in the last 24 hours (never the first day of history) and networks no earlier address belonged to; says so when the owner data is missing. Shown on the Findings tab as counted, not raised.
- **Knowing what is new.** A count of open findings beside "Findings" in the sidebar (polled every 30 seconds), and "N new since you last looked" on the tab (the time it was last left is kept in the browser's storage; nothing depends on it, and it says nothing the first time).

## Tests (written first)
Core `tests/review.rs` (7) and `tests/new_owner.rs` (7); testkit `destination_contract` (+1, both stores); service `findings_admin.rs` (+3); app `history_findings.rs` (+3) and `history_network.rs` (+4 for the digest); frontend `ReviewToday.test.tsx` (7), `Findings.test.tsx` (+4) and `App.test.tsx` (+1).

## Not built
Windows toasts (waiting for the decision on the Tauri notification plugin). The usage and new-thing rules are unchanged.

## Limits (as designed)
The review lists only programs SYNTRAC has seen running; a program that never ran is not "here". "Mark as expected" covers new programs below the folder, not start entries, file changes or accounts. A validly signed program reaching a new network is counted in the digest and never raised. The service must be reinstalled with this build and the app rebuilt together (no schema change: still v15).
