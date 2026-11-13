# Phase T2: programs (second slice of the trend views)

Implements T2 of `docs/TRENDS-PLAN.md`. Network ranking by program was added at the user's request (2026-11-13: "are we planning
to give like top 10 process (app) usage of different resources").

## What changed

**Core** (`crates/syntrac-core/src/trend.rs`, pure): `TrendRepository` (read side, three methods that only group and sum:
`system_sample_total`, `process_totals`, `traffic_totals` without loopback), `merge_programs` (one summary per executable; the CPU
average is the weighted sum divided by the samples of the whole range), `rank_programs` (highest first, programs with nothing for
the metric left out, ties by path, unattributed last), `collect_totals`, `rank_totals`, `top_programs`. `ProgramMetric`: CPU
average, memory peak, starts, network bytes.

**Stores**: in-memory (`memory_rollup.rs`) and SQLite (`syntrac-store/src/trends.rs`: one `GROUP BY` per table, loopback is
`127.%` and `::1`, the same set as `IpAddr::is_loopback`). Contract suite `trend_contract.rs` run against both, plus a parity test
(`rollup_parity.rs`, floats to 1e-9).

**App backend** (`app/src-tauri`): `history_programs.rs` (`range_totals`, `rank_range`, `program_ranking`, `program_history`), the
`Backend` keeps each range's sums until the rollup watermark changes, commands `get_top_programs` and `get_program_history`.
Program names come from the image path; the unattributed bucket is "Unknown program" and has no drill-down.

**Frontend** (`app/src`): Programs tab (range picker, "Rank by" CPU / Memory / Starts / Network, a table with CPU average and
peak, memory peak, starts, received, sent, "recorded in N of M h", last seen), `ProgramDetail` (first and last seen, starts,
CPU chart with average solid and peak dotted, memory-peak bars). `charts/bucketing.ts` is the generic hour-to-bucket grouping
(extracted from `historyBuckets.ts`, which now uses it; its tests did not change), `programBuckets.ts`, `programChart.ts`;
`historyChart.ts` builders are generic over the bucket type. Shared ranges in `components/historyRanges.ts`.

## Tests (written first)
Core `tests/trend.rs` (8); contract `trend_contract.rs` (7 scenarios, both stores); parity (1); app `tests/history_programs.rs`
(9, including the cache being invalidated by a newly rolled-up hour); frontend `programBuckets.test.ts` (4),
`programChart.test.ts` (2), `ProgramsHistory.test.tsx` (11).

## Rehearsal (read-only on the live database; a one-year copy on O:)
- Live database (about 20 hours of rollups): a ranking takes about 8 ms in a debug build. Top by CPU average: WmiPrvSE.exe
  0.36%, MsMpEng.exe 0.28%, unknown program 0.23%, chrome.exe 0.20%; by network: acmetransmit64.exe (20.3 GB sent), music-player.exe,
  chrome.exe, remote-desktop-updater.exe, assistant-client.exe.
- A copy inflated to one year (`examples/inflate_rollups.rs`, `<scratch folder>\trend-year.db`, to be deleted by the user):
  summing the range takes 2.3 s in a release build; ranking the sums, under 1 ms. That is why the backend keeps the sums.
- Query plans: the process query uses the primary-key range on `(hour_start, exe_id)` and primary-key lookups for the joins.

## Observations from the live data (not changes)
- "Unknown program" has the third-highest CPU average and the second-highest memory peak: 6.9 GB in one hour. These are processes
  whose image path could not be resolved when they were sampled; worth a look in Phase 2.
- `acmetransmit64.exe` sent 20.3 GB in about 20 hours: the largest network sender by a wide margin.

## Not in T2
Network destinations (T3); program network per destination; any per-program disk figure (not collected).
