# Phase F2: the "new thing" rules (schema v14)

Slice F2 of `docs/ALERTS-PLAN.md` (approved 2026-11-16). The first detector. Each rule raises a finding with one plain sentence and the facts it used, and only looks at what appeared AFTER detection was first switched on.

## What changed
- **Core (pure).** `new_things.rs`: `new_thing_candidates(facts)` and `NewThingsDetector` (loads the facts through `IdentityRepository`, `LauncherRepository`, `ContextRepository`, `KnownProductRepository` and the new `IdentityChangeRepository`).
  - `new_program`: first seen running after the switch-on, not covered by a known product, and not an UPDATE (an older program with the same maker and install folder, so a self-updating app's new version folder is not news; an unsigned program has no maker, so it is never "an update"). Tier suspicious when its file changed after signing, when it looks like a known program without being it, or when it is unsigned (or untrusted) in a folder any user can write; otherwise unusual. A program whose file has not been read yet is unusual and says so.
  - `new_start_entry`: a current start entry first seen after the switch-on, for a program that was already here (a new program's finding already says how it starts). Suspicious on the same evidence.
  - `changed_file`: different content AND the signer changed, or a validly signed file stopped being valid. Always suspicious. New content from the same signer is an update and is not raised; the key names the new content, so a second change is a second finding.
  - `new_account`: a program that was already here now runs as an account it had not run as before, with the accounts it had. Suspicious only when it moves to SYSTEM without a valid signature.
  - At most 100 per rule per run, most serious first.
- **Folder scope for "always expected".** The backtest showed a developer machine makes new unsigned programs in new folders all the time (Rust tools, build scripts). `ExpectationScope::{ThisItem, ThisFolder}`: the second stores `folder:<install folder>` (worked out by the service from the finding's own subject with the same rule the product grouping uses, never a drive root, new programs only), and `reconcile` lets it cover findings of that kind whose subject is below that folder (a folder named like the start of another is not below it). Pipe: `FindingDecide.scope` (`this_item` default, `this_folder`). The Findings tab shows a "What should this cover?" choice that names the folder.
- **Stores.** Migration 14 (additive, no backup): `identity_changes`. `save_identity` adds a row when both the old and the new reading were hashed and differ (SQLite and in-memory, one contract suite `identity_change_contract`).
- **Service.** `detectors()` now returns the new-thing detector (own connection). `detect_enabled` is still false by default.
- **Backtest.** `crates/syntrac-store/examples/rehearse_detect.rs`: on a COPY of a database, "if detection had been switched on N days ago, what would the rules raise now?", counts by rule and tier plus sample sentences.
- **App.** `FindingView.folderScope`; `decide_finding` passes the scope.

## Backtest on a copy of the real database (2026-11-16; details are local, not stored here)
On a development machine the rules raised 114 to 200 findings for a switch-on one to five days earlier. Two readings: the start-entry count (100, the cap) is an artifact of replaying, because launch records only began being kept a day earlier, so every entry looks new to an earlier switch-on (in real use, entries recorded before the switch-on count as already here); and the suspicious programs were developer tools (unsigned programs in a user folder: language toolchains, image-editor helpers, installer remnants), which is what the folder scope and the "Review what is here today" screen (F4) are for. The rules ran in 6 to 23 ms.

## Deviations from the plan
The rule "new network owner for a program" is moved to F4 with the new-destination digest (it needs the per-program address list and the owner data).

## Tests (written first)
Core `tests/new_things.rs` (23) and `tests/findings.rs` (+4 for the folder scope); testkit `identity_change_contract` (both stores); store `migration_v14_tests` (1); ipc (+1); service `findings_admin.rs` (+2) and `new_things_detection.rs` (2); app `history_findings.rs` (+1); frontend `Findings.test.tsx` (+2).

## Limits (as designed)
A stolen certificate signing a replacement by the same signer is not caught by `changed_file`. A program that is never seen running is not a new program yet. The file-change history only starts at schema v14. The service must be reinstalled with this build (schema v14) and the app rebuilt together; detection stays off until `detect_enabled = true`.
