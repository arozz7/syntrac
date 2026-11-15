# Network history (T3): implementation plan

Status: **APPROVED 2026-11-15** (T3-0, T3a, T3b; T3c waits for the source and licence). **T3a done 2026-11-15** (`aiChangeLog/phase-T3a.md`).
T3 is the third slice of
`docs/TRENDS-PLAN.md` (approved 2026-11-13); this document replaces its one-line T3 entry with a full design.

## Goal

Answer, over days and months and after the 14-day detail purge: *where does this machine talk to, who asked for it, and what is
new?* Read-only on the app side. The one thing that must be collected NOW, because it is lost after 14 days, is the link
between an address and the name a program asked for (DNS). Everything else already sits in the hourly rollups.

## What exists today (checked on the live database, 2026-11-15)

- `rollup_traffic_hour` (per hour, executable, address): 20,890 rows. Permanent ledgers `destinations` (1,992 addresses) and
  `exe_destinations` hold first and last seen.
- DNS: 62,946 `dns_query` events in `events` (29,162 with an answer, 702 distinct names, 4,385 distinct answer addresses in about
  two days). Each carries the asking PID, the name and the answer addresses. They are **purged after 14 days** and nothing
  summarises them. The app shows no names anywhere (`names.rs` only names processes).
- T2's pieces to reuse: `TrendRepository`, the pure `trend.rs`, the contract suite in `syntrac-testkit`, the per-range sums
  cache in the backend, `Data<T>` states, `RangePicker`, `DataGate`.

## Design

### 1. Permanent address-to-name table (schema v8, additive)

`address_names (addr TEXT, name TEXT, first_seen INTEGER, last_seen INTEGER, PRIMARY KEY (addr, name)) WITHOUT ROWID`, plus a
one-row `names_progress` marker for the backfill. One row per (address, name) pair ever seen in a DNS answer. No counters, only
min and max of the times, so writing is **idempotent** and order independent (same rule as the ledgers: re-running an hour or the
backfill never changes a result).

- **Pure core** `dns_names.rs`: turns the DNS events of one hour into (addr, name, first, last) rows. Names are lower-cased,
  a trailing dot is removed, and an empty or non-answer lookup (such as `wpad` with no answer) gives no row. Addresses use the
  same canonical form as flows (IPv4-mapped IPv6 becomes IPv4).
- **Write path**: the rollup runner that already closes each hour also stores that hour's names, in the same transaction as the
  rollup, so a crash cannot leave one without the other.
- **Backfill (once, on the first start after v8)**: the same pure function over the DNS events still retained (about two days
  now, up to 14), marked done in `names_progress`. It is a bounded read of the `(category, ts)` index.
- **Bound**: at most 50 names kept per address (the most recently seen win), so one CDN address cannot grow without limit.
  Expected size is tens of thousands of rows a year, negligible against the 14-day detail.
- The service is the only writer, as for every other table.

### 2. Queries (read only, a new trait next to `TrendRepository`)

`DestinationTrendRepository`: `destination_totals(from, to)` (one group-by over `rollup_traffic_hour`, loopback left out, the
same pattern and the same sums cache as T2), `new_destinations(since, limit)` (ledger `first_seen` at or after `since`, with the
program that first talked to it, from `exe_destinations`), `destination_programs(addr, from, to)` (who talked to it),
`names_for(addrs)` (the names, newest first), plus the existing `destination_hours` and `destination_ledger` for the drill-down.
Ranking, ties (by address) and merging are pure functions in `trend.rs`, so the in-memory and SQLite stores share the maths and
the same contract suite plus a parity test on a generated multi-week history.

### 3. Address class (pure core, `addr_class.rs`)

`Public`, `Private` (10/8, 172.16/12, 192.168/16, fc00::/7), `LinkLocal`, `Multicast`, `Other`. Loopback is already excluded.
The Network tab defaults to **Public** only, because the router, printers and mDNS are not what you ask about; a switch shows
everything. This is a property of the address, so it is computed when shown and never stored.

### 4. Tauri commands and screens (thin, off the UI thread, like T2)

`get_top_destinations(range, order, limit, scope)`, `get_new_destinations(since, limit, scope)`,
`get_destination_history(addr, range)`. Each returns `Data<T>`. The History entry gets its third tab, **Network**:

- *Top destinations*: address and its newest name (with "+3 other names"), class badge, bytes sent and received, flows, number of
  programs, first and last seen. Order by bytes, flows or programs. Click a row to open the drill-down.
- *New destinations*: first seen in the last 1, 7 or 30 days, with the program that first talked to it. Because the history is
  short, **everything first seen in the first 24 hours of history counts as the baseline** and is not called new; the screen
  says "history begins <date>" until a week of history exists, as T1 and T2 do.
- *Drill-down*: the hourly curve of bytes in and out, the programs that talked to it and when, all its names, first and last seen.
  A program name there opens that program's history (the existing link pattern).

**Honest labels (shown in the UI).** A DNS name is what some program *asked for*, not who owns the address: CDNs and cloud hosts
put thousands of names on one address and one name on many, so names are shown as "asked for", never as an owner. An address with
no recorded name says "no name seen" (a connection made by address, or before names were collected, which is before v8). The
unattributed bucket stays "Unknown program". Hours with no data are gaps.

### 5. IP owner levels (separate decision gate, see slice T3c)

Agreed earlier as suggestions, not yet approved as features:
1. **DNS names**: in this plan, local, free of any network use. (T3a/T3b)
2. **Reverse DNS** (PTR lookup): **opt-in**, off by default. It sends the address to your resolver, so it leaks what you talk
   to; the setting says so. Result cached permanently per address.
3. **Offline IP-to-ASN dataset**: a bulk file loaded locally, lookups never leave the machine. Candidate:
   [IPtoASN](https://iptoasn.com), free TSV files updated hourly; third-party sources describe it as PDDL (public domain), but I
   could not reach the site's own licence statement from here, so **you must confirm the source and licence before any
   download**. The owner is recorded at first sight, refreshed monthly with a manual refresh button (your earlier direction,
   still to be confirmed), and the file is shown as "downloaded <date> from <source>". All network use is opt-in and off by
   default; nothing in T3a/T3b needs it.

## Slices (each ends with `scripts/dev/verify.ps1` green, a rehearsal on a COPY of the live database, and a commit)

- **T3-0 Layout fix** (CSS only, no logic). Cards in several screens let text spill past their edge. Likely cause: a table or long
  value wider than its card, and `.card` has `min-width: 0` but no overflow handling (`app/src/theme/app.css`, `global.css`). Fix:
  tables sit in a horizontally scrollable wrapper, long text wraps (`overflow-wrap: anywhere`), and the narrow "Recorded in" and
  size headers get room. I want the **screens or a screenshot of each clipped card** before choosing, so I fix what you see and
  not what I guess. Test: render each view at a narrow width and assert the wrapper and wrap classes.
- **T3a Names** (v8). Migration and its test (a copy of the live DB), `dns_names.rs`, the repository write and backfill, the
  rollup runner change, contract tests in both stores, service tests. No UI. Installing it needs the service reinstall (you do
  that), and **from then on names are kept**; before it there is no past to recover beyond the 14 days of retained DNS events.
- **T3b Queries and Network tab**. `addr_class.rs`, `DestinationTrendRepository`, `trend.rs` additions, the three commands, the
  tab with drill-down. The app and service must both be on v8 (the strict schema check).
- **T3c Owner (opt-in)**: its own short plan after you choose the source and approve the licence. Not started by this approval.

## Tests first

- Core: DNS names from events (case, trailing dot, no-answer lookup, IPv4-mapped IPv6, several answers, the 50-name cap keeps
  the newest), idempotence (twice the same rows), address classes at the range edges (172.15/172.16/172.31/172.32, fe80::, ff02::),
  destination ranking and ties, "new" baseline rule, empty and short ranges.
- Contract suite for the new trait against the in-memory and SQLite stores, a parity test on several generated weeks, query plans
  checked (no scan of `rollup_traffic_hour` for new destinations; the ledger is used).
- Migration v8 on an empty database and on a copy of a v7 one; the backfill runs once and a second start does nothing.
- Backend: empty database, no rollups, schema mismatch (explicit states). Frontend: each Network view with fake data (normal,
  short history, empty, error), the class filter, the drill-down, the labels above.
- Rehearsal on a COPY of the live database with timings per query.

## Documentation and change log

Per slice: `aiChangeLog/phase-T3-0.md`, `phase-T3a.md`, `phase-T3b.md`; update `docs/SCHEMA.md` (v8), `docs/TRENDS-PLAN.md` (T3
status and deviations), `docs/ARCHITECTURE.md` and `README.md` as needed.

## Not in this plan

Reverse DNS and the ASN dataset (T3c), alerts on new destinations (Phase 3, which will use the known-programs list), GeoIP
country, exporting, names per hour (only first and last seen are kept), DNS events with no answer.

## Decisions I need from you

1. Approve T3-0, T3a, T3b as written (T3c waits).
2. Clipping: which screens, ideally a screenshot of each.
3. "New destination" baseline of the first 24 hours of history, and Public-only as the default scope: OK?
4. Cap of 50 names per address: OK?
