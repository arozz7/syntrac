# Phase F5b: programs started by other programs, and a helper's first use of the network (no schema change)

Slice F5b of `docs/ALERTS-PLAN.md` (F5 approved as written on 2026-11-17; the user said "proceed" after F5a). Full description: `docs/PATTERNS.md`.

## What changed
- **Vocabulary.** `applies_to = "process_start"` (a program started by another program), `parent_is` (the file name of the parent) and `first_network_use`. Validation (`patterns.rs`): `parent_is` only for process_start; a process_start pattern must name `parent_is` AND `name_is` (so the read is bounded by the pack); no `already_here` on a started program; `first_network_use` only for a program; `= false` refused as before.
- **Pack 2026.11.2:** `office_starts_shell` (T1059, suspicious), `browser_starts_shell` (T1059, unusual), `script_helper_first_network_use` (T1218, suspicious). The evidence names the parent and the program and says how often and how recently ("Started N time(s) ... the last X days ago"); it never claims a command line.
- **Engine** (`pattern_run.rs`): started-by targets from `SpawnPair`s whose last start is in the recent days; the subject is `parent key|child key` (one finding per pair of programs, so the pack version and the count do not change the key); the child's file facts are looked up by its key. The detector reads only what the pack asks about (`uses_first_network_use`, `started_program_names`).
- **Two new read ports, each behind a contract suite run against BOTH stores:** `SpawnRepository::spawn_pairs(since, child_names)` (core `spawns.rs`; SQLite `spawns.rs` is one join and group-by over the raw process rows with the name compared by equality, never LIKE, so `_` and `%` mean themselves; in-memory `memory_spawns.rs`; contract `spawn_contract.rs`) and `DestinationTrendRepository::first_online_since(since)` (programs whose EARLIEST non-loopback sighting is at or after the time; contract added to `destination_contract.rs`).
- **Backtest** (`rehearse_detect`): prints the number of start pairs and of programs first online, and the new patterns' matches.
- No service config change (the existing `patterns_enabled` gates everything), no schema change (still v15), no new dependency.

## Findings about the data that changed the plan
- SYNTRAC stores NO command lines (0 of 711,209 process rows have one), so the plan's "parent, command line" is parent only.
- The raw process rows are kept 14 days (detail retention), not 30.
- "From a new parent" needs a baseline of normal parents; 14 days cannot give one, so it is not built. "First network use" reads the permanent ledger instead and covers all history.

## Backtest on a copy of the live data (2026-11-17; 1,994 programs)
The read of 55 start pairs (cmd, PowerShell, pwsh, wscript, cscript, mshta) took about 650 ms on 711,000 process rows (it runs every detect interval, on the detect worker's own connection). No office, PDF or browser program started a shell in the 15 days of rows. 4 programs came online for the first time in the last day and none is a helper. All three new patterns: 0 matches; the whole pack: 1 match (a remote-access tool's service, unusual), as before.

## Tests (written first)
Core `tests/patterns.rs` (+8, now 33): office and browser parents with the evidence and key; ordinary parents (explorer, node) and children (not a shell) are quiet; the recent-days window; the first-network-use pattern for a helper that was not online before, and quiet for a helper that was online earlier and for a program that is not a helper; the new vocabulary's refusals. Testkit `spawn_contract` (6, both stores): grouping with count and first and last start, the time bound, whole-file-name matching without regard to case with `_` and `%` meaning themselves, children with no known parent left out, no names reads nothing; `destination_contract` (+1, both stores): first online after a time, loopback is not the network, never-online programs.

## Not built (planned)
F5c: the user-imported hash list (the only route to "confirmed") and beaconing (only after its own backtest). A "this folder" scope for patterns; the pack version on the Findings tab header.

## Limits (as designed)
Parent and child names are weak evidence (a renamed program, or a parent that was not resolved at the start, is missed), and only 14 days are visible. Reinstall the service to get this build; `patterns_enabled` is still the switch.

## F5c decision (2026-11-17)

The user skipped F5c (hash-list import and beaconing): they know of no hash list to import. Nothing was built. The tier "confirmed" therefore stays unused by patterns. It can be reopened with a chosen source.
