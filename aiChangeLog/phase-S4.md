# Phase S4: retention gate, storage guard, space recovery

Implements S4 of `docs/STORAGE-PLAN.md`. Two decisions were taken with the user on 2026-11-13: per-process downsampling is
**dropped** (measured: those samples are about 8% of the database, so about 9% saved over 14 days, for a weight column and a
change to the CPU breakdown maths), and the installer **rewrites** the old default `retention_days = 30` to
`detail_retention_days = 14` (with a timestamped copy of the config). Status: implemented and rehearsed on a copy of the live
database; **not yet applied to the live service**.

## What changed

**Pure core** (`crates/syntrac-core`):
- `retention.rs`: `purge_cutoff` (the gate: `min(now - retention, rolled_up_before)`, nothing without a watermark),
  `trim_cutoff` (one hour past the oldest detail, never past the watermark), `purge_step` (a purge pass is at most an hour of
  detail), `assess` (levels `Ok`/`Trimming`/`Paused`, with a 10% hysteresis margin so a database at its cap does not flap, and
  free space counted as drive free plus the database's own free pages).
- `storage_guard.rs`: `check_storage` (measure; while not `Ok`, trim one hour of rolled-up detail, hand the pages back, measure
  again; stops when `Ok`, when nothing eligible is left, when a step frees nothing, or after one step per check) and the
  traits `StorageStats`, `FreeSpace`, `DetailStore`.

**Store** (`syntrac-store/src/storage.rs`, `connection.rs`): `StorageStats` for SQLite (file, live and reclaimable bytes from the
page counts; oldest detail and oldest rollup); `reclaim_space` (steps `PRAGMA incremental_vacuum` to exhaustion, since each
step frees one page, then `wal_checkpoint(TRUNCATE)`); the one-time switch to incremental auto-vacuum in `open` (a VACUUM,
about 2.4 s on a 195 MB copy). The switch is **not fatal**: if it cannot run (another connection holds a lock) a notice is
recorded (`take_notices`, logged by the host) and it is retried at the next start.

**Service** (`syntrac-service`): `guard.rs` (schedules the check every minute, logs level changes), collector: the purge uses
the gate and runs in one-hour passes a second apart, then reclaims space (non-fatal); while `Paused` the collector keeps process
events only (so traffic after the pause is still attributed), discards the rest and skips metric sampling, then resumes by
itself; config keys `detail_retention_days` (default 14, `retention_days` still accepted, both together refused),
`max_database_gb` (10), `min_free_space_gb` (20), `hard_floor_free_gb` (5, must be below the minimum). `drive_space.rs` in
`syntrac-collector` measures the data folder with `GetDiskFreeSpaceExW`.

**Status and UI**: the pipe's status report has an optional `storage` section (older clients and services keep working); the
text status prints database size, drive free space, and says TRIMMING or PAUSED; the app's health turns `Degraded` for paused
(outranks a silent ETW) and trimming, with plain-words messages; the banner shows database size, drive free space, how far back
detail and summaries go, and hours trimmed early.

**Installer**: `Update-RetentionDefault` (tested) rewrites exactly `retention_days = 30`; any other value, a commented line, or
a config that already uses the new name is left alone. `install-service.ps1` copies the config aside first.

**Diagnostics**: `examples/rehearse_retention.rs`.

## Tests (written first)
core `retention.rs` (18), `storage_guard.rs` (10, fake store and volume), contract `storage_contract.rs` (both stores),
store `storage_tests.rs` (real files: conversion, the file shrinking, an idle reader, a reader mid-read, a conversion that cannot
run now), service config (+3), status board (+3), collector (+8: gate, guard states, pause and resume, trimming bounded by the
watermark, chunked purge), ipc protocol (+2), app health (+6), banner (+5), PowerShell `config-helpers` (+9),
`drive_space` (real Windows call).

## Rehearsal on a copy of the live database (aiChangeLog/s4-rehearsal-output.txt)
195 MB copy, a reader connected throughout. Switch to auto-vacuum 2.4 s, reads 2 afterwards. The gate: a 1-minute retention is
held back to exactly the watermark. A purge of detail older than 6 hours removed 315,714 events (8.1 s in one go, hence the
one-hour passes) and the file went from 196.5 MB to 104.7 MB after reclaim (1.25 s) with the reader still connected. Events at or
after the cutoff, events not yet rolled up and all rollup rows were untouched. The guard, with the cap set to a quarter of the
size, trimmed one hour per check (about 1.5 to 2.5 s each at the time, so one hour per check keeps the loop free; ETW has a
65,536-event queue, roughly two minutes at the usual rate) until it was within the margin, never past the watermark. Integrity ok.

## Operating notes
- The first start of the upgraded service converts the database to incremental auto-vacuum (a few seconds) and the installer
  rewrites the config to 14 days. Nothing will actually be deleted for about 13 days (the database holds about a day of detail).
- Default limits: cap 10 GB live data, trim below 20 GB free, pause below 5 GB free (decimal GB). All in `service.toml`.
- A trimming guard deletes only detail that is already in the hourly rollups. If rollups ever stall, nothing is deleted, and the
  disk can still fill: the hard floor then pauses detail collection.
- The UI shows sizes in binary units (GB = 1024^3), the config uses decimal GB; the small difference is cosmetic.
- Not in S4 (as agreed): per-process downsampling; UI views of the trends.
