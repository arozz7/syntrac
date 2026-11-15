# Phase T3b: the Network tab

Slice T3b of `docs/NETWORK-PLAN.md` (approved 2026-11-15). Read-only: destinations ranked over a range, destinations that are new, and
one destination in detail, with the names kept by T3a.

## What changed

**Core** (pure): `addr_class.rs` (`classify_addr`: Public, Private incl. shared 100.64/10 and IPv6 fc00::/7, LinkLocal, Multicast,
Other; IPv4-mapped IPv6 is classified as the IPv4 address). `destination_trend.rs`: `DestinationTrendRepository` (three methods that
only group, sum and list), `rank_destinations` (bytes, flows or programs; the scope is applied BEFORE the limit; ties by address;
byte sums saturate), `rank_destination_programs`, `new_destination_cutoff` (never inside the first 24 hours of history, the
baseline; no history means nothing is new), `select_new` (newest first).

**Stores**: in-memory (`memory_destinations.rs`) and SQLite (`destinations.rs`: one group-by per question; loopback left out as in
the Programs ranking). Migration 8 gained the index `idx_exe_destinations_addr` (migration 8 had not been installed yet). Contract
suite `destination_contract.rs` run against both; two query-plan tests (the first program is found through the index, the totals read
only the range's primary-key rows).

**App backend**: `history_network.rs` (`destination_ranking`, `new_destinations_view`, `destination_history`), `Backend` keeps each
range's sums until the rollup watermark moves, commands `get_top_destinations`, `get_new_destinations`, `get_destination_history`
(`all_addresses` switches the scope).

**Frontend**: History gets a Network tab: toolbar (range, order by bytes / flows / programs, "Include this network and multicast"),
Top destinations, New destinations (last day / 7 / 30 days, with the baseline note), and a detail card (names asked for, programs,
traffic chart). `networkLabels.ts`, `charts/destinationBuckets.ts`.

## Tests (written first)
Core `addr_class.rs` (9), `destination_trend.rs` (14); contract `destination_contract.rs` (6 scenarios, both stores); store plan tests
(2); app `history_network.rs` (15 views + 2 with a real database, including the kept sums being refreshed by a new hour); frontend
`networkLabels.test.ts` (3), `destinationBuckets.test.ts` (3), `NetworkHistory.test.tsx` (21).

## Rehearsal (copy of the live database, `app/src-tauri/examples/rehearse_network.rs`, copy at `<scratch folder>\network`, to be deleted by the user)
Copy 5.6 s, upgrade to v8 0.02 s, backfill 0.25 s (6,508 pairs). Ranking 25 rows: 4 ms over 24 hours, 8 ms over 7 days and over all
history (62 hours of history so far); new destinations 1 ms; one destination's detail under 1 ms. The 7-day ranking by bytes on the
real data: a backup service's upload servers (about 2.4 to 5.1 GB each), a music-streaming CDN, a remote-desktop vendor's plugin
server and an AI API host, each with the name that was asked for.

## Limits (as designed)
- A name is what a program asked for, never an owner. An address reached by address, or before T3a, has none.
- Everything first seen in the first 24 hours of history is the baseline, so with a short history "new" is conservative.
- The tab shows the 25 highest by default. A year of sums takes longer to read once (see TRENDS-PLAN T2), then is kept.
- The app and service must both be on v8.
