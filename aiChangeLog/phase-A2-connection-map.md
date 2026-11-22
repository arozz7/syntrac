# Phase A2: the connection map (no schema change)

Part of the approved plan. The hourly traffic rollups already hold bytes per program and address; nothing showed which programs talked to which
networks, only one program's destinations or one destination's programs.

## What was built
- **Store read:** `DestinationTrendRepository::traffic_pairs(from, to)`: one row per (program, address) over a range, in ONE read (a single group-by
  over `rollup_traffic_hour`), loopback left out, the unidentified bucket as no program. SQLite and in-memory implementations, shared contract
  `the_traffic_of_every_program_to_every_address_comes_back_in_one_read` in `destination_contract.rs` (both pass).
- **Core, pure:** `crates/syntrac-core/src/connection_map.rs`. `build_map(pairs, scope, limits, lookup)` groups addresses by owner (an address is
  looked up once, only internet addresses are looked up, no owner is ONE node and never a guess), ranks programs and networks by bytes (ties by
  name / AS number) and folds everything beyond the limits into one "others" node per side, so the links always add up to what was given. Each
  network names its five biggest addresses so a click can open one. Tests first: `tests/connection_map.rs` (17).
- **App:** `app/src-tauri/src/history_map.rs` (`connection_map`, command `get_connection_map`): names, the owner and country, the name asked for
  each top address, the owner dataset line. At most 30 nodes per side. Tests first: `tests/history_map.rs` (9).
- **Page:** History, **Connection map** tab. ECharts **sankey** (not the graph series named in the plan: a sankey draws exactly this, programs to
  networks with bands as wide as the traffic, and lays itself out), order kept as ranked. Clicking a program opens it (the Network tab's pop-up), clicking
  a network shows its owner and busiest addresses, each opening the destination pop-up. A table of the same links is below the chart, with buttons
  that do the same, so nothing depends on the canvas. Tests first: `charts/mapChart.test.ts` (9), `views/ConnectionMap.test.tsx` (12). The `Chart`
  wrapper gained `onSelect` and `height`.
- **Docs and pictures:** README row and picture, user guide, `history-connection-map` shot in `demo-capture.ps1`.

## Files
New: `connection_map.rs` (+ test), `history_map.rs` (+ test), `api/mapTypes.ts`, `charts/mapChart.ts` (+ test), `test/fakeMap.ts`,
`views/ConnectionMap.tsx` (+ test). Changed: `destination_trend.rs`, `destinations.rs` (store), `memory_destinations.rs`, `destination_contract.rs`,
`lib.rs` (core, app), `backend.rs`, `commands.rs`, `history_network.rs` (two helpers made crate-visible), `api/api.ts`, `api/types.ts`,
`test/fakeApi.ts`, `components/Chart.tsx`, `views/History.tsx`.

## Limits worth knowing
Only the busiest dozen programs and networks are drawn (the table has the same). The range is read from the hourly summaries, which are kept long
after the detail. Owners come from the downloaded owner dataset; without it every address is "No owner found".
