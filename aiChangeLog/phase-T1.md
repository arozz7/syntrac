# Phase T1: tray icon, close-to-tray and alerts without a window (no schema change; app rebuild only)

Slice T1 of `docs/ALERTS-PLAN.md` (slice T), approved by the user on 2026-11-20 together with the official single-instance plugin.

## What changed
- **One program, with or without a window.** Closing the window destroys it (the web view's memory is freed) and the program keeps running in the tray (`RunEvent::ExitRequested` without a code is refused; Quit exits with a code). The tray icon is the window's icon; left click or Open (re)creates the window from the configured one. Menu: Open SYNTRAC, Findings (N open), Notifications (check mark), Quit. The tooltip says how many findings are open, or that the collector is not running.
- **Alerts come from the program, not the page** (`toast_watch.rs`, `tray.rs`): a thread looks at the open findings every 30 seconds (the same view the Findings tab uses, up to 500) and sends the toasts through the official notification plugin's Rust side. The rules of `toasts.ts` are ported with the same tests: one toast for each new suspicious or confirmed finding (at most three, then "N more suspicious findings"), unusual ones as one summary, long text cut to 200 characters. The page no longer sends any toast, so there is exactly one sender.
- **What counts as new changed on purpose.** It is "an id above the last one announced", kept in a settings file, instead of a set of ids seen in this session. So a finding raised while the program was NOT running is announced at the next start (under the same caps), and the very first run ever announces nothing. A finding that is seen while notifications are off counts as seen. If the settings file cannot be written, the value stays in memory, so a finding is never repeated every 30 seconds. A snoozed finding that comes back keeps its id and is not announced again (before, it was).
- **Settings file** (`prefs.rs`): `%LOCALAPPDATA%\Syntrac\app.json` (the plan said `%APPDATA%`; this is next to the other per-user file, `database-path.txt`). A missing or damaged file means the defaults (notifications on); written through a temporary file. The notifications switch moved here from the page's `localStorage` (so the first run after this update has toasts ON, whatever the old switch said). The sidebar button and the tray menu item both use it; the sidebar reads it every 5 seconds, so a change from the tray shows there too.
- **One instance only**: the official `tauri-plugin-single-instance` (the only new package in the lock file; the `tray-icon` feature added none). Starting SYNTRAC again opens the window of the one that is running.
- **Tighter page permissions**: the page no longer has the notification permissions (`capabilities/default.json` is `core:default` only) and the JS package `@tauri-apps/plugin-notification` is removed (`pnpm-lock.yaml` shrinks by 10 lines). `Api.notify` is replaced by `notificationsOn` / `setNotificationsOn`.

## Tests (written first)
- `tests/toast_watch.rs` (13): the plan (first run, new suspicious and confirmed, the unusual summary, ids above the last, the cap of three and the singular, the cut, the id never going backwards) and the watcher with fakes (first tick, once only, notifications off, unreadable findings, the report for the tray, findings raised while the program was not running, a settings file that cannot be written).
- `tests/prefs.rs` (6), `tests/tray_text.rs` (4); `App.test.tsx` (+3: the switch says what the program says, a click asks the program, the page sends no toast).
- **Smoke run** of the built program with its own temporary `LOCALAPPDATA`: it started with its window, and after the window was closed the process was still running (the tray). Left in `%TEMP%`: `syntrac-smoke-*` (a web-view profile).

## By-hand checklist (the tray and window glue cannot be unit-tested)
1. Start the app: a tray icon appears; hovering shows "SYNTRAC: N open findings" (or "the collector is not running").
2. Close the window: the icon stays; Task Manager shows the program still running with little memory.
3. Left click the icon, or Open SYNTRAC: the window comes back. Start SYNTRAC a second time: no second icon, the window opens.
4. Notifications: untick it in the tray menu: the sidebar button says "Toasts: off" within a few seconds; click the sidebar button: the tray check mark follows.
5. With the window closed, a new suspicious finding raises a toast within about 30 seconds.
6. Quit from the tray menu: the program exits.

## Deleted afterwards (the user said yes)
- `app/src/toasts.ts`, `app/src/toasts.test.ts`, `app/src/hooks/useToasts.ts` and `useToasts.test.tsx` were unused once the page stopped sending toasts, and were deleted with the user's approval (their rules live on in `toast_watch.rs`). Also deleted: the rehearsal copy `<scratch folder>` and the smoke-test folder in `%TEMP%`.

## Not built (by plan)
- Start with Windows (slice T2), toast buttons, sound, opening the app from a toast click (the plugin has no click action on Windows), a tray program that outlives the app.
