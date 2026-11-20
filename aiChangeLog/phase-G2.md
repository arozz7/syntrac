# Phase G2: GPU detection (no schema change, still v16)

Slice G2 of `docs/ALERTS-PLAN.md`: the user wanted capture AND detection ready now, even though there is no GPU history yet (2026-11-20). Built on G1 (`aiChangeLog/phase-G1.md`).

## What changed
- **Usage rule** (`syntrac-core/src/usage.rs`): two new metrics, GPU (the hour's average, a share of ONE engine) and GPU memory (the hour's peak), judged exactly like CPU and memory: against the program's OWN earlier active hours, above `max(3 x p95, 1.5 x highest)`, over a floor, sustained for the last two hours; the same flagged-hours rule and expectation ceiling. Floors: `gpu_floor_percent` 20 (a flat share of one engine, never scaled by cores) and `gpu_memory_floor_bytes` 500 MB above the highest peak. Subjects end `|gpu` and `|gpu_memory`. Hours rolled up before v16 hold 0, which is an inactive hour, so a program with no GPU history is not judged on the GPU (tested).
- **Rows** (`ProcessHourRow.gpu_avg / gpu_memory_peak`): read by both stores (`syntrac-store/src/usage.rs`, `syntrac-testkit/src/memory_usage.rs`); the usage contract suite covers the GPU on both.
- **Config**: `usage_gpu_floor_percent` (default 20, must be above 0 and at most 100) and `usage_gpu_memory_floor_mb` (default 500, above 0), in `service.toml` and `host.rs`.
- **Pattern condition `sustained_gpu = { percent, hours }`** (closed vocabulary; percent 1 to 100, hours 1 to 24; unknown keys and any other shape refused; programs only): the program's hourly GPU average was at least `percent` in EACH of the last `hours` completed hours in a row (a missing hour counts as zero). "The last hours" are counted back from the newest hour that has rows, and rollups older than the usage rule's limit (4 hours) match nothing. The read is gated (`PatternPack::uses_sustained_gpu`): the hourly rows are read only when a pattern asks for them, through the existing `UsageRepository` (so the contract suite from the usage rule covers it on both stores).
- **Pack 2026.11.3**: `unsigned_program_keeps_gpu_busy` (T1496.001, Compute Hijacking; the id and name checked on attack.mitre.org), unsigned/untrusted/altered, in a user-writable folder, 30% for 3 hours. The evidence line carries the hourly numbers.
- **UI**: "raise the limit" now reads "Up to 45.0% of one GPU engine" for a GPU level and bytes for GPU memory (it would have read "45 B": the screen branched on the end of the subject).
- **Backtest** (`rehearse_detect`): a "GPU use" section that says "no GPU history yet" when no hour has GPU use recorded, instead of printing zeros that look like a result; with data it counts the programs the usage rule can judge (48 or more active GPU hours) and the busy program-hours. Run on a COPY of the live database (28,388 program-hour rows, migrated v15 to v16): "no GPU history yet", the new pattern matches 0, the usage replay raises 0.
- **Docs**: `docs/PATTERNS.md` (the condition, the pattern, its limits), `docs/SCHEMA.md` (the level's unit), `docs/ALERTS-PLAN.md`, `docs/THREAT-MODEL.md`.

## Decisions to note
- **Tier is unusual and NOT lifted.** The plan said "unusual, lifted by `raise_if_concerning`". But the pattern already requires unsigned/untrusted/altered in a writable folder, which is exactly what the lift tests, so it would have made every match suspicious, and only suspicious findings give toasts. The user's local AI server is expected to match once. Unusual is the honest label for "busy GPU alone".
- The pattern reuses `UsageRepository` instead of a new read port: the same hourly rows, already under a contract suite on both stores.

## Tests (written first)
- Core `tests/usage.rs` (+7: steady, two hours raised with the numbers, one hour or under the floor, no GPU history is not judged, GPU memory, floor from settings, flagged hours left out), `tests/patterns.rs` (+6: shipped pattern and its technique/tier, validation of the condition, match with numbers in the evidence, harmless look-alikes including a gap hour, another program's hours and a covered program, stale rollups).
- Service: `tests/config.rs` (+2), `tests/gpu_pattern_detection.rs` (2, end to end through the rollups and `PatternDetector`).
- Screens: the "raise the limit" unit for GPU and GPU memory. Contract: usage rows carry the GPU on both stores.

## Not verified yet
- GPU capture was measured from a USER session only. The service runs as SYSTEM: after the reinstall, look for the warning "The GPU counters are not available on this machine" in the service log, and check that recent `system_metrics.gpu_percent` values are not NULL.
- There is no GPU history yet, so neither GPU rule has been judged on real data; run `rehearse_detect` again after about two weeks of capture and read the GPU section before relying on it.
