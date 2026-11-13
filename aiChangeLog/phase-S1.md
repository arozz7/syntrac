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

**E. The UI follows the database.** `db_location` (10 tests) and a backend that asks the service where the database is
(app: 62 tests, up from 45). Precedence: an explicit `SYNTRAC_DB` (pinned, never overridden) > the path the running service
reports over the pipe > the last path remembered in `%LOCALAPPDATA%\Syntrac\database-path.txt` > the default. If the database
is not where the app expects, the first data call asks the service before showing "No data", so a moved database is found
immediately. A reported path is only believed if it is absolute, ends in `.db` and has no `..`.

**F. Scripts follow the data root.** Found while thinking through the sequence: after a move, re-running the installer or
smoke test with the default data folder would have seen a "wrong" config, regenerated it and pointed the service back at the
stale copy on C:. The installer, smoke test and uninstaller now default to the data root recorded in the installed config
(`Get-DataRootFromConfig`, 6 checks), then to the standard location on a fresh install; an explicit `-DataDir` still wins. The
smoke test passes the root to the elevated half and checks the same things on any root.

## Still to do
The real move on this machine (elevated, by the user): install the current build, `move-data.ps1 -DryRun`, `move-data.ps1`,
then the smoke test, which follows the new root. Then delete the old files on C: by hand when satisfied.

Known gap: the database path is also what `-RemoveData` of the uninstaller deletes; it now follows the config, but it was
not exercised. A pipe peer that squats the pipe name could still tell the UI to open some other `.db` file read-only
(mitigated by the plausibility checks; the pipe itself is created by SYSTEM with FIRST_PIPE_INSTANCE).