# Phase S3: hourly rollups and permanent ledgers (schema v5)

Implements S3 of `docs/STORAGE-PLAN.md` as approved on 2026-11-13: no port in the traffic key (with `top_port` and
`distinct_ports`), hourly for everything, UTC. Status: implemented and rehearsed on a copy of the real database; **not yet
applied to the live database** (applied when the upgraded service is installed).

## What changed

**Pure core** (`crates/syntrac-core`):
- `rollup.rs`: `roll_up_hour(&HourInput) -> HourRollup`, all aggregation maths, deterministic (rows sorted before they are
  summed), rows outside the hour ignored. `hour_start` (a boundary timestamp belongs to the NEW hour, also before 1970),
  `executable_key` (lower-cased trimmed path; empty means unattributed).
- `rollup_ledger.rs`: `HourRollup::ledger_observations()`, the one list both repositories apply (MIN/MAX widening).
- `rollup_run.rs`: the `RollupRepository` port and `roll_up_due` (hours due once ended + 5 min settle; first pass starts at
  the hour of the earliest detail; at most `max_hours_per_pass` (2) per pass; empty hours still advance the watermark).

**Migration v5** (`migrations.rs`, additive, no backup needed): `executables` (dimension and executable ledger; id 1 =
unattributed), `destinations`, `exe_destinations`, `rollup_system_hour`, `rollup_process_hour`, `rollup_traffic_hour`,
`rollup_progress`, plus two indexes the history queries use.

**Repositories**: `syntrac-store/src/rollups.rs` (SQLite: `store_hour` replaces the hour's rows, widens ledgers and advances
the watermark in ONE transaction, and refuses an hour at or below the watermark) and `syntrac-testkit/src/memory_rollup.rs`
(in-memory reference). Both show an executable's path as first seen.

**Collector** (`collector.rs`, `host.rs`, `config.rs`): `roll_up_if_due` runs before the purge; a backlog is worked off with a
1 s pause between passes, otherwise every `rollup_interval_minutes` (new key, default 5); failures are counted and retried at
the usual interval. `config/service.toml` documents the key.

**Diagnostics**: `examples/sql_query.rs` (read-only SQL), `examples/rehearse_rollup.rs`.

## Tests (written first)
- `syntrac-core/tests/rollup.rs` (19): hour boundaries, rows outside the hour, empty and partial hours, summed instances,
  absent-as-zero averaging, case-insensitive executables, unattributed rows, starts without samples, a process spanning an
  hour boundary, top-port ties, saturation, order independence, ledger widening.
- `syntrac-testkit/src/rollup_contract.rs` (14 cases) run against BOTH repositories: nothing due in the first hour, only
  completed and settled hours, re-running changes nothing, a backlog in capped passes, gaps, first pass at the earliest detail,
  PID reuse attributed by instance, starts from `process_started` events (not process records), traffic by executable and
  destination, ledgers surviving a purge of the detail, a rolled-up hour cannot be rolled again, earliest detail, inclusive
  history ranges.
- `syntrac-store/tests/rollup_parity.rs`: the same scenario in SQLite and in memory gives identical rollups; a read-only
  connection reads history while the writer works. `migration_v5_tests.rs`: v4 data survives, no backup, the existing detail
  rolls up straight away.
- `syntrac-service/tests/collector_loop.rs` (+3): a completed hour is rolled up, the hour in progress is not, the interval and
  the backlog pause.

## Rehearsal on a copy of the live database (aiChangeLog/s3-rehearsal-output.txt)
16 hours of real data rolled up in 4 passes of 4 hours (now 2 per pass), 0.83 s in total, slowest pass 241 ms. Cross-checks
against the raw detail, all identical: system samples 10,840; network bytes received 3,100,055,306; flow bytes sent
21,265,331,919 and received 2,567,953,729; flow rows 289,122; process starts 52,849; destination addresses 913;
attributed executable-destination pairs 1,220.

Stored: 16 system rows, 2,064 process rows, 4,039 traffic rows, 470 executables, 913 destinations, 1,220 pairs: 0.79 MB for
16 hours. **That is about 1.2 MB a day, roughly 430 MB a year**, higher than the 150 MB estimated in the plan (a traffic row is
about 100 bytes with its index). Still small next to the detail it replaces (about 550 MB a day), and permanent.

## Operating notes
- The first collector pass after the upgrade rolls up the whole backlog (hours of detail) two hours at a time, one second apart.
- Nothing is deleted yet. The existing 30-day purge is unchanged; gating the purge on the watermark and `detail_retention_days`
  is S4. Until then rollups stay well ahead of it.
- Limitation: the stored remote port of loopback or inbound traffic is the peer's (often ephemeral) port.
- An executable is described while it is among the heaviest processes recorded per sample (default 25).
- Service and UI must be upgraded together (schema v5); an older UI shows the schema-mismatch state.
