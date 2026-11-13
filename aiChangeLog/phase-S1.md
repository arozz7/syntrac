# Phase S1: relocatable data root (storage plan)

Plan: `docs/STORAGE-PLAN.md` (approved 2026-11-13). Decisions: data on `D:\Syntrac`, detail retention 14 days and
configurable, rollups kept, no raw archives for now.

## Done so far

**A. Database inspection.** `syntrac_store::inspect_database` (7 tests) and `syntrac-service --check-db <file>`: read-only;
schema version, SQLite integrity check, rows per table, one stable line per fact so a database and its copy can be compared
as text. Never creates a file; a missing, non-database or damaged file is an error, not a pass.

**B. Missing data drive.** `check_data_drives` (4 new tests): if the database or log folder is on a drive that is not there,
the service stops at once with a message naming the drive (written to `startup-error.txt`) instead of "cannot create folder".
The installer now sets the service to restart after 5 s, 30 s, then every 60 s indefinitely, so it starts working by itself
when the drive returns.

**C. Installer and helpers.** `data-root.ps1` (22 + 8 checks): `Test-DataRootChoice` is pure and refuses USB / FireWire /
iSCSI disks, removable, network and optical drives, UNC paths, relative paths, missing drives and drives with too little free
space, listing every problem. Checked against this machine: O: and G: accepted; L:, M:, R: refused as USB even though
Windows calls them "Fixed"; J: refused for space (4 GB free). `install-service.ps1` runs this before changing anything (free
space is not re-checked for an upgrade that stays on its current root) and shares the permission code with the move script
(`Set-DataRootAcls`). `service-common.ps1` holds the shared elevation, wait-for-service and run-collector helpers.
`Set-ConfigPaths` rewrites only the two path lines of a config (13 checks): comments, other settings, the pipe user and line
endings are kept.

**D. `scripts/deploy/move-data.ps1`.** Checks the target and the current database, stops the service, COPIES the database
(never touches the old files), proves the copy (same schema, same rows in every table, integrity ok), switches the config
(old config kept as `service.toml.bak-<time>`), starts the service and waits until it reports the new path and is writing. Any
failure after the service was stopped restores the config and restarts the service on the original database. `-DryRun`
(works unelevated) was run against the real database: 256 MB, healthy, plan printed, nothing changed. Refused correctly for a
USB drive and a UNC path.

Found while testing: the installed build predates `--check-db`, so the move script refuses with an explicit message; the
current build must be installed first (or `-CollectorExecutable` pointed at a newer exe).

## Still to do in S1
E. The UI learns the database path from the service. F. The smoke test runs against a custom data root. Then the real move
(elevated, by the user).