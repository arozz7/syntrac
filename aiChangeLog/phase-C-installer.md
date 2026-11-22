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

## Not verified yet
- **The installer has not been run.** Windows Sandbox is not installed on this machine, and the installer must not be run on a machine whose service is in
  use. Until the sandbox test has passed, install, upgrade, rollback and uninstall are only checked in parts: the installer compiles, the PowerShell scripts parse, and
  every decision is unit-tested.
- The database-copy rollback (a release that raises the schema) is covered by the pure tests only; the sandbox test uses a stand-in with the same schema.
- Signing has never been run (no certificate).
