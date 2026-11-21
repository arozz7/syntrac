# Phase T2: start with Windows (no schema change; app rebuild only)

Slice T2 of `docs/ALERTS-PLAN.md` (slice T), started on 2026-11-20 after the user had tried T1 ("T1 looks fine .. work on T2").

## What changed
- **A "Start with Windows" item in the tray menu**, a check mark, OFF until the user switches it on. Switching it on writes ONE value, `SYNTRAC`, in the signed-in user's own Run key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`): `"<this program>" --tray`. No administrator rights, nothing for other users, no service, no scheduled task. Switching it off deletes that value. The check mark always shows what the registry says (a refusal leaves it as it was).
- **`--tray` starts with no window.** `tauri.conf.json` no longer lists a window; the program builds the main window itself (`tray::open_main_window`, the same size and title as before), and only when it was NOT started with `--tray`. So a program started by Windows holds no web view in memory until you open it. Starting SYNTRAC again (single instance, T1) opens the window of the running copy.
- **Repair after a move** (`autostart::repair`, run at every start): when the entry is on but the program it names no longer exists, it is pointed at the program that is running now, so autostart keeps working after a rebuild elsewhere. It never switches anything on, and never touches an entry naming another copy that still exists (a copy the user chose).
- **Safe by construction**: a path containing a quote character gets no command at all (nothing is written); the value is read with a size cap and only as text; the real store is a thin adapter over a trait (`AutostartStore`), so the rules are tested with a fake.
- New module files: `autostart.rs` (pure rules), `autostart_registry.rs` (the registry, through the `windows` crate that the project already uses; no new package). The app crate gains `windows` as a direct dependency (features `Win32_Foundation`, `Win32_System_Registry`).

## Tests (written first)
- `tests/autostart.rs` (11): the start mode, the exact command, an unsafe path, reading the program back out of a command, off by default and off again, replacing a stale entry, a failed write changing nothing, the repair (rewritten only when the named program is gone), and a REAL round trip on a throwaway key `HKCU\Software\Syntrac\AutostartTest-<pid>-<time>` (removed afterwards; a helper that only accepts such keys). The test never writes the real Run key.
- **Smoke run** of a debug copy built with a different app identifier (so it could not touch the user's running copy), with its own temporary per-user folder: `--tray` runs with no window; a second launch exits at once and opens the window of the first; closing the window leaves the program running; a normal start shows the window. After the run no test key was left in the registry and the real Run key had no `SYNTRAC` value.

## By-hand checklist (the menu click cannot be automated)
1. Tray menu: "Start with Windows" is unticked. Tick it: Task Manager's Startup apps tab lists SYNTRAC, and `Get-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Run` shows the value `"...\syntrac-app.exe" --tray`.
2. Sign out and in (or restart): the tray icon appears and no window opens.
3. Untick it: the value is gone.

## Notes
- Task Manager's Startup tab can disable an entry without deleting it (Windows keeps that elsewhere); this program does not read that switch, so the check mark means "the entry exists".
- Not built: a switch in the page's sidebar (the tray is the one place), a machine-wide start for all users, a scheduled task.
