# Trend views: implementation plan (T1-T3)

Status: **APPROVED 2026-11-13.** The user chose: all three slices in order (T1, T2, T3), local-time days beyond 7 days, and one
"History" sidebar entry with tabs.

## Goal

Show what the hourly rollups (schema v5, live since S3) already hold: how the machine, each program and each network
destination behave over days, weeks and months, even after the detail has been purged (14 days). Read-only, local, no new
collection. The data today is about one day of rollups, so the views must say plainly when history is short ("history begins
13 Nov 2026") instead of drawing a misleading flat chart.

## What already exists

- Core `RollupRepository` can read `system_hours`, `executable_hours`, `destination_hours`, and the three ledgers (first and
  last seen) for one named executable, destination or pair. SQLite and in-memory implementations pass one contract suite.
- The app opens the database read-only on every call (`backend.rs`) and returns `Data<T>` states; the UI has a range picker,
  a polled `DataGate`, ECharts (`Chart.tsx`), and the light earthy theme tokens.
- Missing: anything that LISTS or RANKS (top programs by CPU over a month, destinations first seen this week). Those are new
  read queries.

## Design

**Interfaces (separate trait, per interface segregation).** A new core trait `TrendRepository` (read only), not more methods on
`RollupRepository`, which stays the write path of the collector:
- `system_trend(from, to)`: the hourly system rows (already available; re-exposed so the UI depends on one trait).
- `top_programs(from, to, metric, limit)`: per executable over the range: CPU averaged over the system's samples (hours with no
  samples are NOT counted as zero, they are gaps), peak CPU, peak memory, starts, hours present, first and last seen
  (from the permanent ledger). `metric` is CPU, memory peak or starts.
- `top_destinations(from, to, limit)`: per destination address: bytes sent and received, flows, number of programs that talked to
  it, first and last seen (ledger).
- `new_destinations(since, limit)`: destinations whose ledger `first_seen` is at or after `since`, with the program(s) that
  first talked to them. This is the "something new appeared" view and the first step towards Phase 3 baselines.
- `program_trend(path, from, to)` and `destination_trend(addr, from, to)`: the per-hour rows for one program or destination,
  for the drill-down (already available).
The ranking and weighting arithmetic is a pure core module (`trend.rs`) used by BOTH implementations (the SQLite one pushes the
grouping into SQL for speed but is held to the same contract suite plus a parity test against the in-memory one), so the two
cannot disagree.

**Time buckets.** Rows are stored hourly in UTC. Up to 7 days the UI shows hours; beyond that it shows LOCAL-time days, regrouped
in the UI from hourly rows (a UTC daily row could not be regrouped, which is why S3 stored hours). A year of system rows is
8,760 points, small. Program and destination rankings are computed in the store, never by pulling a year of traffic rows into the
UI.

**Honest labels (limits of the data, shown in the UI).**
- A program's CPU is recorded only while it is among the 25 heaviest processes in a sample, so light programs read as "not
  present", and their average is the average over the hours in which they WERE recorded, shown with "seen in N of M hours".
- Traffic is keyed without the port; the stored port is the peer's. Destinations are addresses, with names only where the app
  already knows them.
- The unattributed bucket is shown as "Unknown program", never hidden.
- Hours with no data (the collector was down or paused by the storage guard) are gaps in the chart, not zeros.

**Tauri commands (thin, off the UI thread, like the existing ones):** `get_history_system(range)`, `get_history_programs(range,
metric, limit)`, `get_program_history(path, range)`, `get_history_network(range, limit)`, `get_new_destinations(since, limit)`,
`get_destination_history(addr, range)`. Each returns `Data<T>`; a database without rollups yet gives an "empty" state with the
reason.

**UI.** One new sidebar entry "History" with three tabs, each independently shippable: *System* (CPU, memory, network and disk
curves; range 24 h / 7 d / 30 d / 90 d / 1 year / all available), *Programs* (ranked table, click a row for its curve and its
first and last seen), *Network* (top destinations, a "new this week" list, click a row for who talked to it and when). Same
range picker component and chart wrapper, no new dependencies, light theme only.

## Slices (each ends with `verify.ps1` green, a rehearsal on the live data, and a commit)

- **T1 System history** (done). Deviation from the design above, recorded: the bucketing into local days and the gap handling
  live in the UI (`charts/historyBuckets.ts`, which has the user's time zone), not in a core `trend.rs`; and `TrendRepository` is
  NOT introduced yet, because T1 needs nothing beyond `RollupRepository::system_hours` (a trait with one wrapper method would be
  noise). `TrendRepository` arrives with T2, where ranking over a range is new. Delivered: `app/src-tauri/src/history.rs`
  (`system_history`), command `get_system_history`, the History entry with its System tab (CPU and memory, network, disk).
- **T2 Programs.** `top_programs`, `program_trend`, the ranked table and the drill-down.
- **T3 Network.** `top_destinations`, `new_destinations`, `destination_trend`, the Network tab.

## Tests first (per slice)

- Core: ranking order and ties, weighting by samples, gaps are not zeros, local-day regrouping across a UTC midnight and across
  a daylight-saving change, an empty range, a range longer than the history, the unattributed row, case-insensitive paths.
- Contract suite for `TrendRepository` run against the in-memory and the SQLite stores, plus a parity test on a generated
  history of several weeks; SQLite query plans checked (no full scans of the traffic table for a one-year range).
- Backend: empty database, database without rollups, schema mismatch, all as explicit states.
- Frontend: each tab with fake data (normal, short history, empty, error), range switching, the drill-down, the labels above.
- Rehearsal on a COPY of the live database with timings for each query.

## Not in this plan

Baselines and "unusual for this program" alerts (Phase 3), exporting, per-process downsampling, monthly archives, comparing two
periods side by side.

## Decisions (resolved 2026-11-13)

1. Scope: all three slices, in order.
2. Day boundaries beyond 7 days: local time.
3. Placement: one "History" entry with tabs.
