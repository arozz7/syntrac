# Phase F5a: the pattern engine and the name, path, signature and start-entry patterns (no schema change)

Slice F5a of `docs/ALERTS-PLAN.md` (the user approved the F5 list "as written" on 2026-11-17). Full description: `docs/PATTERNS.md`.

## What changed
- **Pattern pack as data.** `config/patterns/rogue-software.toml` (pack 2026.11.1, nine patterns), embedded in the build with `include_str!` (`BUILT_IN_PACK`). Core `patterns.rs`: `parse_pack` with serde `deny_unknown_fields` and a closed vocabulary; refuses an unknown condition or word, a duplicate id, a bad id or technique id, an empty or empty-list condition, `= false` flags, the tier `confirmed`, an `entry_kind_is` on a non-start-entry pattern, texts that are empty, too long or contain control characters.
- **Matching.** Core `pattern_run.rs`: `pattern_candidates` (pure), `osa_distance` (a swap of two letters is one edit), `PatternDetector` (loads identities, known products and start entries through read ports). Programs seen in the last 30 days only; known products never; files not read yet not judged by a pattern that needs the file; one kind per pattern (`pattern_<id>`); the pack version is in the evidence, not in the key; at most 50 per pattern per run. Evidence carries path, location, signature, the technique id, "Often harmless: ...", and the pack version; a direction-control character is named (U+202E) and never repeated.
- **Patterns that duplicate F2 are limited to what F2 does not see:** `already_here` (first seen before the switch-on) on the unsigned-download and start-entry patterns, so a new program or entry is not raised twice.
- `LocationClass::parse` (`exe_location.rs`); `signature_line` is now `pub(crate)` in `new_things.rs`.
- **Dependency:** `toml` added to `syntrac-core` (it was already in the lock file through the service: no new package).
- **Service:** `patterns_enabled` (default `false`, in `config/service.toml` with a comment) gates the detector in `host.rs`; a pack or database failure logs a warning and detection carries on without it.
- **Backtest:** `rehearse_detect` prints, for the copy, the matches per pattern (seen in 30 days, and all time for comparison), the first 40 matches by path, and the two branches of the start-entry pattern counted separately.
- **Findings tab:** a `pattern_*` kind is shown as "Known pattern: <name>" (finding cards and expectation rows).
- **Docs:** new `docs/PATTERNS.md` (vocabulary, shipped patterns, MITRE notice and licences), `docs/ALERTS-PLAN.md`, `docs/ARCHITECTURE.md`, `docs/THREAT-MODEL.md`.

## Backtest on a copy of the live data (2026-11-17; 1872 programs)
Seen in the last 30 days: 1 match (`remote_access_tool`: a remote-access tool's service, unusual). Everything else: 0. All time: the same. Branches of the start-entry pattern (not shipped): 3 unsigned targets anywhere, 9 signed targets in user-writable folders. The machine has 1376 unsigned and 433 validly signed identities, and NO file carries the download mark, so `unsigned_download_in_user_folder` cannot fire here; 31 unsigned programs seen in the last 30 days live under a user folder and are left to the new-program rule and the review screen.

## Tests (written first)
Core `tests/patterns.rs` (25): the shipped pack loads and every pattern explains itself; a small pack loads; the closed vocabulary refuses regex, code, unknown places, states and kinds, `false`, empty conditions and empty lists; `confirmed`, bad ids and techniques, a missing benign text, duplicate ids; the distance (swap, change, missing, added); each pattern with a match and a harmless look-alike (system folders including SysWOW64 and a `\Device\HarddiskVolume` path, a Microsoft-signed copy, a file not read yet, generic names such as `service.exe`, short names, build outputs, signed apps starting from AppData, unsigned programs in Program Files, ordinary dotted names); programs and entries first seen after the switch-on are left to F2; direction control is not echoed after cleaning; known products; recency; never-seen programs; the per-pattern cap; keys without the pack version. Frontend: `Findings.test.tsx` (+1, the pattern label).

## Not built (planned)
F5b (parent and child processes, first network use of script helpers; raw rows, 30 days), F5c (hash-list import, beaconing after its own backtest), "this folder" scope for patterns, the pack version on the Findings tab header.

## Limits (as designed)
Weak evidence by nature (see `docs/PATTERNS.md`). The user did not name remote-access, tunnel or bulk-copy tools they use, so nothing is pre-expected: each such tool will raise once and one click handles it. Reinstall the service with this build to get the new config key; set `patterns_enabled = true` only after reading the backtest.
