# Phase T1: system history (first slice of the trend views)

Implements T1 of `docs/TRENDS-PLAN.md`, approved by the user on 2026-11-13 (all three slices in order, local-time days, one
"History" entry with tabs).

## What changed

**App backend** (`app/src-tauri`):
- `src/history.rs`: `system_history(repo, now, range_secs)` builds `SystemHistoryView` (the stored UTC hours in the range, oldest
  first, as percentages and millisecond times, plus `historyStartsMs`, the oldest rollup, whatever range was asked for). The
  range is clamped from 1 hour to 10 years; "all" is simply longer than the history. A zero memory total gives 0%, not a
  division by zero. Missing hours are not invented.
- `backend.rs` (`system_history`, opened read-only like every view), `commands.rs` (`get_system_history`), `lib.rs` (module and
  handler).
- `examples/history_probe.rs`: reads the view models from a real database, read-only, with timings.

**Frontend** (`app/src`):
- `charts/historyBuckets.ts` (pure): hours into buckets by hour (up to 7 days) or LOCAL day (beyond), averages weighted by the
  samples behind each hour, gaps between buckets with data (never before the first or after the last), `summarise`. The time zone
  is injected (`DayStart`), so 23- and 25-hour days are tested without depending on the test machine. A day-start rule that does
  not advance cannot hang the loop.
- `charts/historyChart.ts` (pure): option builders for the load chart (CPU average solid and filled, CPU peak dotted, memory
  dashed, fixed 0-100% axis) and the bytes charts (grouped bars). Category axis, so a gap is an explicit null and spacing is even.
- `charts/readPalette.ts`: the palette reader extracted from `SystemChart` (DRY), used by both.
- `views/SystemHistory.tsx`, `views/History.tsx` (tab control, System only for now), nav entry in `App.tsx`, `RangePicker` takes
  optional `options` (24 hours, 7 days, 30 days, 90 days, 1 year, All), `Chart` registers bar charts, `format.ts` `formatDay`.
- Notes in the screen: "History begins <date>" when the range reaches further back than the data; only completed hours are shown
  (the latest appears about 5 minutes after the hour ends); a break in a line is time with no data, not zero.

## Deviations from the plan (also recorded in `docs/TRENDS-PLAN.md`)
- Bucketing lives in the UI rather than a core `trend.rs` (the UI knows the time zone and daylight saving).
- `TrendRepository` is not introduced in T1 (nothing new to read); it arrives with T2.

## Tests (written first)
Rust `app/src-tauri/tests/history.rs` (8: conversion, range, history start, empty, gap, zero total, clamp, real database and a
missing one); frontend `historyBuckets.test.ts` (13: hour/day, gaps, local midnight, 23- and 25-hour days, non-advancing rule,
summary), `historyChart.test.ts` (6), `SystemHistory.test.tsx` (10), `App.test.tsx` (+1).

## Rehearsal on the live database (read-only, `history_probe`)
19 completed hours of system history (the first at 2026-11-12 ~22:00 UTC); each range query takes under 1 ms. Longer ranges
will not look different until more days have been rolled up: today only about a day exists, and the screen says so.

## Not in T1
Programs (T2) and Network (T3); anything that ranks or lists executables or destinations.

## Follow-up: the disk numbers were checked (2026-11-13)
The user asked whether the disk figures can be trusted and split by drive. They are the sum of every live process's read and
write transfer counters, not physical disk traffic (details and measurements in `docs/TRENDS-PLAN.md`, "Known limits"). The
History card is now titled "Program file I/O" and says so. No per-drive data exists; that needs a separate, approved plan.
