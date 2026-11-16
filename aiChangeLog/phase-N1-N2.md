# Phase N1 and N2: Top owners, and where one program connects

Slices N1 and N2 of `docs/ALERTS-PLAN.md` (approved 2026-11-16). Read-only; no schema change, no service change, no network use.

## N1: Top owners
- **Core (pure).** `owner_totals.rs`: `group_by_owner(rows, scope, limit, lookup)` groups the range's destination sums by the network
  that owns them (AS number), biggest first (ties: lower AS number), keeps the biggest five destinations of each owner, counts all of
  them, never looks up an address that is not on the internet, and puts every address with no owner (no dataset, outside every range,
  not on the internet) in ONE group with no owner, never guessed. A lookup failure is returned, not hidden. `AddressScope::includes`
  became public.
- **App backend.** `history_network.rs`: `OwnersView` / `OwnerRowView` / `OwnerDestinationView`, `rank_owner_range`, `owner_ranking`;
  `Backend::top_owners` (uses the same cached range sums as the destination ranking, so it costs one grouping pass), command
  `get_top_owners`.
- **Frontend.** `views/TopOwners.tsx` in the Network tab below "Top destinations": sortable table (owner, received, sent, flows,
  destinations); an owner opens in place to its biggest destinations (each opens the destination pop-up) and says "+N more
  destinations"; says plainly when no owner data is on the machine and that the group with no owner is "No owner found".
- **Deviation from the plan.** The plan said a row opens "the same pop-up"; it opens in place instead (the same pattern as a product
  group in Programs history), which needed no new pop-up state.

## N2: where does this program connect
- **Index decision (measured, plan step).** On a copy of the live database (32,388 hourly traffic rows; the 1.3 GB is the event log, not
  the rollups) the busiest program's destinations read in 5 ms without an index; an index on `(exe_id, hour_start)` built in 26 ms,
  grew the file by about 1 MB and made no difference (6 ms). So NO index and NO schema change: the plan's "v13 for the index" is not
  used, and the findings schema is v13 (identity history v14). Revisit if a ranking over years of rows gets slow (about 2.4 million
  rows a year at today's rate would scan in roughly 0.4 s).
- **Core.** `ProgramDestination` and `DestinationTrendRepository::program_destinations(path, from, to)` (matched without regard to
  case; loopback left out; one row per address).
- **Stores.** SQLite (one group-by joined to `executables` by `path_key`) and in-memory, both passing one new contract test.
- **App backend.** `program_destinations_view` (internet addresses by default; ranked by bytes; "first reached" is THIS program's
  permanent first sighting of the address; "New" = first reached after the baseline and within the range; the total count whatever
  the limit), `Backend::program_destinations`, command `get_program_destinations`.
- **Frontend.** `views/ProgramDestinations.tsx`, "Where it connected" inside a program's detail (Programs history and the Network
  pop-up): sortable, name and owner lines ("No name seen", "No owner found"), a "New" badge, "Showing the biggest N of M". In the
  Network pop-up an address opens that destination.

## Tests (written first)
Core `owner_totals` (7); testkit `destination_contract` (+1, runs against both stores); app `tests/history_network.rs` (+8: owners with
destinations and dataset, no dataset, scope, more destinations; a program's destinations, "new", case-insensitive path and unknown
program, limit); frontend `TopOwners.test.tsx` (7) and `ProgramDestinations.test.tsx` (8).

## Limits (as designed)
An owner is the organisation that runs a range, often a cloud or hosting company, not necessarily the service that was reached. A
program's destinations are in the hourly rollups only (no per-connection detail), and the program view leaves out addresses on this
network and multicast. Owner lookups need the downloaded owner data.
