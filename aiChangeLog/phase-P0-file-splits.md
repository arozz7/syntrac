# Phase P0: make room before the next features (file splits, no logic change)

`app/src/api/types.ts` (644 lines) and `app/src/test/fakeApi.ts` (652) were over the 600-line soft limit, and the process tree, connection map and question box would have pushed them toward the 900 hard limit.

Method (refactoring protocol): tests green first (412), then whole top-level blocks (comment included) were moved by a script, nothing inside a block was touched, the original files keep their names and re-export the new ones so no importer changed, nothing was deleted. Proof: the lines of the old file and the lines of all new files, apart from imports and re-exports, are identical (0 differences); type-check, lint and all 412 tests pass after.

## File mappings
`api/types.ts` (now 186 lines: data states, health, overview, processes, connections, system history, storage, write outcome)
- -> `api/programTypes.ts`: ProgramMetric, LocationClass, IdentityBadgeView, RegisteredView, LauncherKind, LauncherView, AccountKind, AccountView, ServiceView, IdentityFailureView, IdentityDetailView, StandingView, ProgramMemberView, ProgramRowView, ProgramsView, ProgramHourView, ProgramHistoryView, KnownProductView, LookalikeView, KnownProgramsView
- -> `api/networkTypes.ts`: AddressClass, DestinationOrder, OwnerView, OwnerDataView, OwnerDataStateView, OwnerDataOutcome, DestinationRowView, RowProgramView, DestinationsView, OwnerDestinationView, OwnerRowView, OwnersView, ProgramDestinationRowView, ProgramDestinationsView, NewDestinationRowView, NewDestinationsView, DestinationNameView, DestinationProgramView, DestinationHourView, DestinationHistoryView
- -> `api/findingTypes.ts`: FindingStatus, FindingTier, FindingView, ExpectationView, FindingCountsView, DetectionView, DigestView, FindingsView, ReviewProgramView, ReviewGroupView, ReviewView, FindingDecision

`test/fakeApi.ts` (now 81 lines: `makeFakeApi`)
- -> `test/fakeHome.ts`: NOW, health, cpuProcess, overview, processRow, connectionRow, systemHour, systemHistory, gpuProcess
- -> `test/fakePrograms.ts`: identityBadge, identityDetail, standing, knownProgramsView, programRow, programsView, programHistoryView
- -> `test/fakeNetwork.ts`: ownerDataState, destinationRow, destinationsView, programDestinationRow, programDestinationsView, ownerRow, ownersView, newDestinationRow, newDestinationsView, destinationHistoryView
- -> `test/fakeFindings.ts`: findingRow, expectationRow, reviewGroup, reviewView, findingsView

New views and their fakes go into new files of their own rather than into these.
