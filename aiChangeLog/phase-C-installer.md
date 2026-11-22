# Phase 5, slice C: the Windows installer

One NSIS installer (Tauri, per machine) that installs the app and the background service, reusing `scripts/deploy` as the single source of truth.
Behaviour and the SmartScreen note are in `docs/RELEASE.md`.

## C1: the build
- **`scripts/release/build-installer.ps1`**: checks the version agrees (Cargo.toml, tauri.conf.json, package.json), builds the service in release mode in
  its own target folder (`target/installer`; a program running from `target/release` cannot be overwritten, which blocked the first attempt),
  stages the payload (`app/src-tauri/installer-stage/setup/`, git-ignored), runs `tauri build` and prints the path and SHA-256. `-StageOnly`;
  `-CertificateThumbprint` (signing, off, never run); `-ServiceExe` and `-CopyInstallerTo` for the installer test only.
- **`scripts/release/release-helpers.ps1`** (+ self-test, run by verify): reading and comparing versions, and which deploy scripts ship (no self-tests,
  no smoke scripts).
- **`app/src-tauri/tauri.installer.conf.json`**: passed with `--config`, so `tauri.conf.json` and the normal builds are unchanged.

## C2: install, upgrade, uninstall
Read from the generated NSIS script first (`target/installer/release/nsis/x64/installer.nsi`), which settled these design points:
- An upgrade runs the **old uninstaller first**, in place (`_?=`) or with `/UPDATE`. The uninstall hook therefore removes the service only for a real uninstall
  (`$UpdateMode <> 1` and `$EXEDIR <> $INSTDIR`), so a rollback has something to go back to.
- The Tauri uninstaller removes only files it installed and `RMDir`s non-recursively, so the service's `bin\` and `config\` (siblings of the payload folder
  `setup\`) survive it. The payload sits in `setup\` precisely so the config template cannot overwrite the live `config\service.toml`.
- NSIS is a 32-bit program: PowerShell is started from `Sysnative`, and every path is passed explicitly (otherwise `Program Files (x86)` is seen).
- The folder must be inside Program Files (the service runs as SYSTEM): refused in the `PREINSTALL` hook and again by the script.
- Added **`syntrac-service --version`** (program version and newest database schema), test first. An older program refuses a newer database
  (`SchemaMismatch`), so a rollback that only restored the binary could leave a dead service.
- **`scripts/deploy/installer-helpers.ps1`** (+ self-test, run by verify): who the reading user is (override, then the installed config, then the signed-in desktop
  user, else refuse; never SYSTEM), the allowed folder, reading a schema number, the upgrade plan (a newer schema copies the database first and does not start
  without room), and the rollback steps (only what was saved is restored; a service that was stopped stays stopped).
- **`installer-install.ps1`**: plan, stop, save the old program/config/database, `install-service.ps1`, and on failure restore and restart the old service.
  A failure before anything changed rolls nothing back (it would otherwise delete a service registration that was there). The database copy is removed only once
  the migrated database passes its check, else kept and logged.
- **`installer-uninstall.ps1`** + `uninstall-service.ps1 -RemoveBinaries`: removes the service and `bin\`, keeps config and data.
- **`app/src-tauri/installer/hooks.nsh`**: thin hooks; each runs one script and checks the exit code.

## C3: test in a throw-away Windows
`scripts/release/sandbox/run-installer-test.ps1` builds the real installer and one whose service cannot start (`crates/syntrac-demo` `broken-service`, tested) and
runs `sandbox-test.ps1` in Windows Sandbox: first install, install again, the in-place uninstall of an upgrade, a failing upgrade that must roll back, a
folder outside Program Files, and a real uninstall that keeps the data.

## C4: version, docs, tag
- Version **0.7.0** in `Cargo.toml`, `tauri.conf.json` and `package.json` (the build script checks they agree). The crates' path dependencies no longer
  carry a `version = "0.1.0"` of their own (the crates are never published), so a release changes only those three places.
- The app shows **Version x.y.z** at the bottom of its menu (`app/src/version.ts`, filled from `package.json` by `vite.config.ts` at build time, so it needs no permission and
  cannot drift from the installer). Test first in `App.test.tsx`.
- `docs/RELEASE.md`: how to release a version (no hosted CI); `docs/USER-GUIDE.md`: installing with the installer.

## Found by the sandbox test (and fixed)
The first runs in Windows Sandbox failed, and showed two real problems that no test on the development machine could:
- **The programs needed the Visual C++ runtime** (`VCRUNTIME140.dll`), which a clean Windows does not have, so the service could not even print its version. The installer build now links the runtime in (`-C target-feature=+crt-static`) and **fails if either program still imports it** (`Find-DynamicRuntimeImports`, tested). The install had stopped itself before changing anything, as designed.
- **The data-folder check said "Drive C: is not available"**: `Get-Volume` knows nothing about the drive in a sandbox or some virtual machines. `Get-VolumeFacts` now falls back to the plain .NET drive information (`ConvertTo-VolumeFactsFromDriveInfo`, tested); a removable or too-small drive is still refused.
- The test also copies the installer logs out of the sandbox (step logs) so a failure can be read, and the launcher stops with a message if a Sandbox is already open (a second launch quietly joins it and the test never starts).

## Verified
`scripts/release/sandbox/run-installer-test.ps1` passed all 28 checks in Windows Sandbox (2026-11-22): first install (service running, config names the user, database appears, the user reaches the service over its pipe); installing again keeps config and data and the previous program; the in-place uninstall an upgrade runs leaves the service running; an upgrade to a service that cannot start fails and puts the old service, program and config back; a folder outside Program Files is refused with nothing written; a real uninstall removes the service, `bin\` and the app and keeps the config and the data.

### The pages, by hand-like clicking (2026-11-22)
`run-installer-test.ps1 -Pages` (`pages-test.ps1`) opens the real installer in Windows Sandbox, presses the default button on every page and takes a picture of each.
Looked at: Welcome, License (MIT), Choose Install Location (`C:\Program Files\SYNTRAC`, 21.1 MB), the progress, Completing Setup (Run SYNTRAC, Create desktop
shortcut), then the app opening on the clean Windows with "Running" and live data. A second run shows **Already Installed** (Add/Reinstall components is the default;
Uninstall SYNTRAC is the other option) and finishes with the service running. The uninstaller shows **Delete the application data** (unticked), and after it the service is
gone and the config and data are kept. The checkbox removes only the app's settings under the user's AppData, not ProgramData (read in the generated NSIS script).

### First upgrade on the developer's own machine (2026-11-22)
The v0.7.0 installer was run over the hand-installed service (same schema, v16). The service was stopped, replaced and started again; config, user and data stayed.
Two findings:
- **The running app was not closed.** The app in use was started from the build folder, not from `C:\Program Files\SYNTRAC`; the installer closes only the installed file
  (Windows Restart Manager, registered against that path). Expected behaviour, now tested for the installed app (below).
- **The schema read took two minutes** on a 3.7 GB database, because the installer ran `--check-db` (integrity check and a row count of every table) just to learn the
  schema number. New: `syntrac-service --db-schema <file>` prints only `schema:    vN` (read-only, 0.25 s on the same database), and the installer reads the schema
  with the **new** program (any older schema is readable by it), falling back to the installed program's `--check-db` only if that prints nothing. The post-upgrade
  confirmation keeps the full `--check-db` (the integrity check is its point). Because the old installed program is what a first upgrade would otherwise ask, the speed-up
  is only there for installers that carry the new program, i.e. from the next release.
- Safety: a database file that exists but whose schema could not be read now stops the upgrade before anything changes (before, "no schema" was treated as "no database
  yet", so a schema-raising upgrade could have migrated with no copy). `Get-UpgradePlan -DatabaseExists`, with tests.
- The sandbox test has five more checks (33): the install log names the database's schema; an app opened from `C:\Program Files\SYNTRAC` is closed by a silent upgrade, which exits 0
  with the service running. The pages test now leaves the app open during the second install, so the dialog that asks to close it is photographed.
- `docs/RELEASE.md` had text damaged by an earlier edit (`scripts\deverify.ps1`, the sandbox command split over lines) and stale names (`SYNTRAC.exe`, a `0.1.0` hash example): repaired.

## Not verified
- The database-copy rollback (a release that raises the schema) is covered by the pure tests only (the real upgrade on the developer machine had the same schema); the sandbox test's stand-in has the same schema.
- Only one clean Windows build (the sandbox's) and a fresh machine were tried; no upgrade from an older real release exists yet.
- Pages were stepped through with the default button only; the Back, Cancel and Browse paths, a different language and a high-DPI screen were not tried. A real person on a real machine has not used it.
- Signing has never been run (no certificate).
