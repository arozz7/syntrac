# Phase T3c: who owns an address (owner data, schema v10)

Slice T3c of `docs/NETWORK-PLAN.md`, design `docs/OWNER-PLAN.md` (approved 2026-11-15). Next to each destination address the Network
tab can say which network owns the range it sits in ("Network AS13335 Cloudflare, Inc., registered in US"), from a public-domain
dataset the user downloads on purpose. A clue, never a verdict, worded that way on screen.

## What changed
- **Core (pure).** `clean_text.rs` (the shared cleaner for text from outside; the collector's `sanitize` now calls it).
  `owner_dataset.rs`: `parse_ip2asn` (tolerant: header row, comments, tabs or spaces; unrouted AS 0 counted and not kept; bad,
  backwards, mixed-family and overlapping rows dropped and counted; org cleaned and capped), `lookup_in`, `addr_key` (IPv4 in
  IPv4-mapped IPv6 space). `owner_refresh.rs`: `OwnerRepository`, `DatasetFetcher`, `refresh_owner_data` (a file with fewer than
  1,000 ranges or more than 5% unreadable lines is rejected and the old dataset kept).
- **Stores.** Migration 10 (additive, no backup): `asn_ranges`, `owner_dataset`. SQLite replaces the dataset through a staging
  table in 20,000-row transactions and one swap transaction; a failure leaves the old data and no staging table. In-memory
  reference plus contract suite `owner_contract.rs` run against both.
- **Download.** `syntrac-collector/src/owner_fetch.rs`: WinHTTP (system certificates and proxy; no HTTP crate), compiled-in
  address `https://iptoasn.com/data/ip2asn-combined.tsv.gz`, redirects refused, download cap 64 MB, unpacked cap 256 MB, timeouts.
  Needed `flate2` as a direct dependency of the collector (already in the build through other crates, so no new crate arrives).
- **Service and pipe.** Requests `owner_data_status` and `owner_data_refresh` (no fields: a client cannot name an address),
  response `owner_data`. `owner_data.rs` runs the download on its own thread with its own connection and answers at once;
  `owner_data_allowed = false` in `config/service.toml` makes the service refuse.
- **App.** Rows, new destinations and the destination detail carry `owner`; `DestinationsView.ownerData` says when and where the
  data came from. Commands `get_owner_data_status` and `refresh_owner_data`. Frontend: `OwnerDataBar` (state, the confirmation
  dialog saying exactly what leaves the machine, progress, failure in the service's words, a note when the data is 30 days old),
  owner lines in the tables and the detail, wording that an owner is not necessarily the service and not where the machine is.

## Deviations from `docs/OWNER-PLAN.md` (and why)
- **No `address_owners` table.** An owner is looked up when a page is shown (one index seek per listed address, microseconds), so
  there are no stale rows, no resolve passes and no change to the collector. The visible behaviour is the same.
- **No size preview before the download.** A preview would have meant contacting the site before the user agreed. The
  confirmation says "a few MB up to tens of MB, never more than 64 MB" instead.
- **Overlapping ranges are skipped and counted, not a reason to reject the whole file** (the real file has none; one bad row
  should not block the refresh). Too many bad rows still rejects it.
- **"Top owners" view not built** (optional in the plan); say if you want it.

## Tests (written first)
Core: `owner_dataset.rs` (11), `clean_text.rs` (3). Testkit: `owner_contract` (both stores), `owner_refresh.rs` (6). Store:
`migration_v10_tests.rs` (2), plan and failed-swap tests (2). Collector: `owner_fetch.rs` (10, against a loopback server).
Service: `owner_data.rs` (8), config (1). Ipc: protocol (2). App: `history_network.rs` (+4), `owner_data_pipe.rs` (5). Frontend:
`OwnerData.test.tsx` (12), `NetworkHistory.test.tsx` (+4), `networkLabels.test.ts` (+4).

## Not yet done
A real download on this machine (the rehearsal with a copy of the live database), which needs the user's go-ahead because it is
the first network use; sizes and timings are measured then. The service must be reinstalled (schema v10 and the new pipe
requests) and the app rebuilt together.
