# Phase T3a: permanent address names (schema v8)

Implements slice T3a of `docs/NETWORK-PLAN.md` (approved 2026-11-15). No UI: this only starts keeping, for good, the link between a
destination address and the DNS names that were asked for it. The raw DNS events are purged after 14 days; from now on a pair
survives that.

## What changed

**Core** (`syntrac-core`, pure):
- New `dns_names.rs`: `DnsAnswerRow`, `AddressName`, `normalize_name` (trim, lower case, no trailing dot), `address_names` (pairs from
  answers; no-answer lookups and blank names give nothing; IPv4-mapped IPv6 becomes the IPv4 address), `merge_names`, `cap_newest`
  (at most `MAX_NAMES_PER_ADDRESS` = 50 per address, the most recently seen win, a tie keeps the name that sorts first),
  `backfill_names` (one-time pass over the retained DNS events, at most 30 days back, hour by hour).
- `rollup.rs`: `HourInput.dns` and `HourRollup.names`; `roll_up_hour` fills `names` (rows outside the hour are ignored, like every
  other row).
- `RollupRepository` gained `names_backfilled`, `load_dns_answers`, `store_backfilled_names`, `address_names`.

**SQLite** (`syntrac-store`): migration 8 (additive, no backup): `address_names (addr, name, first_seen, last_seen)` primary key
`(addr, name)`, and the one-row `names_progress` marker. `names.rs` moves the rows (MIN/MAX upsert, so storing is idempotent; the cap
is applied to every touched address). `store_hour` stores the hour's names in the SAME transaction as the rollup, ledgers and
watermark. `load_hour` loads the hour's DNS lookups from the `(category, ts)` index.

**In-memory** (`syntrac-testkit/src/memory_rollup.rs`): the same behaviour, using the same `cap_newest`.

**Service** (`collector.rs`): the first rollup pass runs `backfill_names` before rolling hours (hours already rolled cannot be rolled
again, so this is the only way their names get in). A failure is logged and counted, and retried at the next pass.

## Tests (written first)
Core `tests/dns_names.rs` (14); contract `names_contract.rs` (9 scenarios) run against BOTH stores; `migration_v8_tests.rs` (2: a
v7 database upgrades with nothing lost, no backup, no names until the backfill; the backfill finds an old lookup once and survives
reopening); service `collector_loop.rs` (the first rollup pass keeps the retained lookups' names).

## Rehearsal (copy of the live database, `examples/rehearse_names.rs`, copy at `<scratch folder>\names`, to be deleted by the user)
- Copy (`VACUUM INTO`, source only read): 17 s for 873 MB. Upgrade to v8: 0.02 s. Backfill: 0.25 s, 6,498 pairs on 4,452 addresses,
  the widest address has 15 names (under the cap). A second backfill does nothing.
- Independent check: `COUNT(DISTINCT address, name)` straight from the DNS events in SQL gives 6,498, the same as stored.

## Limits (as designed)
- Names are what a program ASKED for, never an owner. Lookups with no answer are not stored. Before this phase there is no history
  beyond the 14 days of retained DNS events. A service down for more than 14 days loses the names of those days.
- Installing needs the service reinstall (done by the user); the app and service must both be on v8 (strict schema check).
