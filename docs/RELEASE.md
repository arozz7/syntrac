# Building and releasing the installer

SYNTRAC ships as one Windows installer (`SYNTRAC_<version>_x64-setup.exe`, NSIS, built with Tauri). It installs the app and the background service, and
it uses the same scripts in `scripts/deploy` that the manual install uses.

## Build it

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release\build-installer.ps1
```

The script
1. checks that the version is the same in `Cargo.toml`, `app/src-tauri/tauri.conf.json` and `app/package.json` (change all three together);
2. builds the service in release mode and stages the payload in `app/src-tauri/installer-stage/setup/` (not tracked by git);
3. runs `tauri build` with `app/src-tauri/tauri.installer.conf.json`;
4. prints the installer's path, size and SHA-256.

The build uses its own cargo target folder, `target/installer`, so a program running from `target/release` (the app in use) does not block it. The first
build compiles everything again and downloads the NSIS tools once. `-StageOnly` stops after step 2.

**Building does not install anything.** Do not run the installer on a machine whose SYNTRAC service you want to keep as it is: it replaces the service.
Try it in Windows Sandbox or a virtual machine (see [Testing the installer](#testing-the-installer)).

## What it installs

| Where | What |
|---|---|
| `C:\Program Files\SYNTRAC\SYNTRAC.exe` | the app (Start menu entry) |
| `C:\Program Files\SYNTRAC\setup\` | the payload the installer runs from: the service program, its config template, the deploy scripts |
| `C:\Program Files\SYNTRAC\bin\syntrac-service.exe` | the running service program (made by the install script) |
| `C:\Program Files\SYNTRAC\config\service.toml` | the service's configuration (made once, then kept) |
| `C:\ProgramData\Syntrac\` | the database and logs (or wherever the config says the data was moved to) |

The folder must be inside Program Files: the service runs as SYSTEM, so a folder ordinary users can write to would let them run code with the highest
rights. The installer refuses any other folder.

### Who runs the app

The service's pipe and the data folder are opened to one user: the person who runs the app. The installer decides, in this order:
1. `/USERSID=<SID>` on the command line (silent installs);
2. on an upgrade, the user already in the installed config (so a different administrator running the upgrade changes nothing);
3. the user signed in on the desktop (not the administrator account that the elevation prompt may have used).

If none gives a valid user it stops. It never guesses. `whoami /user` shows your SID.

## Upgrade

Run the new installer over the old one. The service is upgraded in place and rolled back if that fails:
1. the new program says which database schema it uses. If that is newer than the data's, the data is **copied first** (`syntrac.db.before-upgrade`), because
   an older program refuses a newer database. Without room for the copy (database size plus 10% plus 100 MB on the data drive) nothing is changed and the
   installer says so;
2. the service is stopped; the old program (`bin\syntrac-service.exe.previous`) and config (`config\service.toml.before-upgrade`) are copied aside;
3. the service is installed and started. If any of that fails, the saved program, config and (if copied) data are put back and the old service is started
   again;
4. when the new version has opened the data and it passes its check, the database copy is removed. If that cannot be confirmed, the copy is **kept** and the
   log says where.

What is **not** rolled back: the app's own files (`SYNTRAC.exe`), which the installer replaces before it sets up the service. The app only reads, so an
older service with a newer app is a cosmetic mismatch at worst; run the previous installer to put the whole old version back.

The log of an install is `%TEMP%\SYNTRAC-setup.log` (of the administrator account that ran it).

## Uninstall

Settings, Apps, SYNTRAC, Uninstall (or `uninstall.exe`). It stops and removes the service and the program (`bin\`) and the app. It **keeps** the
configuration (`config\`) and your data (database and logs). To remove those as well, run `scripts\deploy\uninstall-service.ps1 -RemoveProgramFiles
-RemoveData` from an elevated PowerShell. The log is `%TEMP%\SYNTRAC-uninstall.log`.

The uninstall page has an unticked box, **Delete the application data**. It removes only the app's own settings (the folder named after the app's identifier
under the signed-in user's `AppData`), **never** the database or logs in `C:\ProgramData\Syntrac`, which only the script above removes.

When a newer installer replaces an older one, the old uninstaller runs first. It recognises that and leaves the service alone.

The app's "start with Windows" entry is a value named SYNTRAC in the signed-in user's `Run` key. The uninstaller removes it for the account that runs the
uninstaller; if that is a different account, the entry stays and Windows reports that the file is missing. Remove it in Settings, Apps, Startup.

## Windows may warn: "Windows protected your PC"

The installer is **not signed**. Windows SmartScreen therefore shows "Windows protected your PC" for a file downloaded from the internet. Choose **More
info**, then **Run anyway**, after you have checked the SHA-256 that the release lists against the downloaded file:

```powershell
(Get-FileHash .\SYNTRAC_0.1.0_x64-setup.exe -Algorithm SHA256).Hash
```

### Signing (not enabled)

Signing needs a code-signing certificate that the owner has to buy and hold; none is used. When there is one, in the certificate store of the build machine:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release\build-installer.ps1 -CertificateThumbprint <thumbprint>
```

This runs `signtool sign` on the finished installer (SHA-256, with a timestamp). It has not been run, so check it on the first real certificate. Even a
signed installer shows the warning until it has built up reputation with SmartScreen, unless an EV certificate is used.

## Testing the installer

The behaviour above is tested in two layers:
- `scripts/deploy/installer-helpers.tests.ps1` (run by `scripts/dev/verify.ps1`) tests every decision: the reading user, the allowed folder, whether an upgrade
  needs a database copy, and what a rollback puts back.
- A run in a throw-away Windows (Windows Sandbox) checks the real thing: a first install, a reinstall, the in-place uninstall of an upgrade, a failing upgrade
  that rolls back, a refused folder, and an uninstall that keeps the data. It needs Windows Sandbox (`Enable-WindowsOptionalFeature -Online -FeatureName
  Containers-DisposableClientVM -All`, then restart) and a closed Sandbox window:

  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts
elease\sandbox
un-installer-test.ps1
  ```

  It builds the installer, runs 28 checks and writes `target\installer-test
esults
eport.txt` and the installers' logs. It passed on 2026-11-22. What it
  does not cover: a release that raises the database schema (the copy-and-restore path), and the installer's pages used by hand.

The programs are built with the Visual C++ runtime linked in, and the build refuses to produce an installer if either still imports it: a clean Windows does not
have that runtime.
