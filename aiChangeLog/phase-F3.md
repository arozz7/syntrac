# Phase F3: usage above what is expected (schema v15)

Slice F3 of `docs/ALERTS-PLAN.md` (approved 2026-11-16). Explainable statistics, no machine learning: a program using clearly more than it usually does, judged against its OWN last days.

## What changed
- **Core (pure).** `usage.rs`: `usage_candidates(facts)` and `UsageDetector`. Per program and metric (CPU as a share of the whole machine, memory peak, bytes sent, bytes received), the last two completed hours are compared with the program's earlier active hours (hours with any use) in the 14 days before: the 95th percentile (nearest rank) and the highest. A finding needs ALL of: more than `max(3 x p95, 1.5 x highest)`, over an absolute floor (CPU: half of one core of the machine, `cpu_floor_for_cores`; memory: at least 500 MB above the highest peak; network: at least 100 MB in an hour), for BOTH of the last two hours. One exception: one hour of uploading more than ten times the highest hour is raised alone. A program with fewer than 48 active hours is not judged. Hours that already raised a finding (their span, widened) are left out of the baseline. Rollups that stopped more than four hours ago raise nothing. The same thing all day is one finding (the key has the UTC day); the next day is another. The sentence carries the numbers, and the evidence lists the last two hours, the usual, and the limit that was crossed.
- **Raise the limit.** `Finding.level` and `Candidate.level` (what the rule measured); `Decision::RaiseLimit` stores it as the `ceiling` of an expectation for that subject; `reconcile` lets an expectation with a ceiling cover levels up to it and raises the finding again above it. Only a usage finding that measured a level can have its limit raised. Pipe decision `raise_limit`. `Candidate`, `Finding` and the write types lost `Eq` (they carry a float).
- **Stores.** Migration 15 (additive, no backup): `findings.level REAL`. `UsageRepository` (two bounded reads over the hourly rollups: per program and hour CPU and memory, and traffic added up over addresses with loopback and the unattributed bucket left out) for SQLite and in-memory, one shared contract suite (`usage_contract.rs`).
- **Service.** The detect worker now has both detectors. Config `usage_window_days`, `usage_min_active_hours`, `usage_cpu_floor_percent` (0 = half of one core), `usage_memory_floor_mb`, `usage_net_floor_mb` (documented in `config/service.toml`).
- **Frontend.** A usage finding offers "Raise the limit to this level" instead of "Always expected"; the list of what you told SYNTRAC shows "Up to 40.0%" or "Up to 3.0 MB" for a raised limit.
- **Backtest.** `rehearse_detect` replays the usage rules at every sixth hour of the last days, as shipped and with much more sensitive settings for comparison.

## Backtest on a copy of the real database (2026-11-16)
97 hours of program history, 181 programs present in the last two hours, 89 with enough history to be judged. Replayed 20 times over 5 days, the usage rules raised nothing as shipped and nothing with much more sensitive settings (floors 1%, 50 MB, 10 MB; multipliers 1.5 and 1.1): no judged program was above its own highest hour in both of the last two hours. So this machine is steady and the thresholds are conservative; they can only be tuned further against real incidents. It also showed that the first CPU floor (25% of the whole machine) could never fire: the busiest program's busiest hour was 4.4% of the machine. Hence the core-aware floor.

## Deviations from the plan
No machine-wide disk finding (per-program disk was never possible; machine-wide disk stays a later idea). The level on a stored finding is the one measured when it was first found, not updated by later hits of the same day.

## Tests (written first)
Core `tests/usage.rs` (12) and `tests/findings.rs` (+4 for the ceiling and raising the limit); testkit `usage_contract` (both stores) and `findings_contract` (+1 for the level); store `migration_v15_tests` (1); service `findings_admin.rs` (+2); frontend `Findings.test.tsx` (+2).

## Limits (as designed)
CPU is a share of the whole machine, so on a many-core machine a heavy program can look small: the floor adapts to the core count. A program that grows slowly for a week becomes its own baseline (see the threat model); the new-entry and changed-file rules matter more. The service must be reinstalled with this build (schema v15) and the app rebuilt together; detection stays off until `detect_enabled = true`.
