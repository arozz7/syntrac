# Phase L4c: the optional AI help, step three: starting and stopping the model server (no schema change)

Slice L4c of `docs/PHASE4-PLAN.md`. The commands exist; the screen that uses them comes with the settings section in L4d.

## What changed
- **Core `server_launch.rs` (pure):** `LaunchCommand` (full path to an `.exe`, an argument list, an optional working folder) and `check()`: a full local drive path only (no relative or network path), no `..`, no characters Windows forbids, only `.exe` (a script would need a shell), never a temporary, download or network folder, at most 64 plain-text arguments of at most 1024 characters. `display()` shows the command the way Windows reads it. `AiSettings` gained `launch`, `start_on_investigate` and `stop_on_quit` (all off or empty by default; older files load).
- **Core `server_manager.rs`:** `ServerManager` over a `ProcessSpawner` port: `start` (refused when the command breaks the rules, when this app's own server still runs, or when a server already answers), `status` (`NotRunning`, `Starting`, `SlowToStart` after 15 minutes, `Ready { started_by_this_app }`, `Loading`, `Exited { code }`, `Failed`), `stop` (only the process this app started), `shutdown(stop_it)`.
- **App `server_process.rs`:** the real spawner: direct start (no shell), no window, no input or output, the working folder, the file and folder checked first, and a Windows job object so Stop ends the program AND what it started (a server that fronts another one). The job has no kill-on-close: when the app quits the server keeps running unless the user chose to stop it.
- **App `ai_control.rs` and `ai_commands.rs`:** `AiControl` (settings in and out with every check, the status for the page with the exact command shown, start, stop, shutdown) and five commands: `get_ai_settings`, `set_ai_settings`, `get_ai_status`, `start_ai_server`, `stop_ai_server`. The server is asked only when the page asks for the status, and never while the help is off or not set up. At exit the server is stopped only when `stop_on_quit` is set.
- **Testkit:** `FakeSpawner` (a fake process the test controls) and `MockLlm::set_state`.
- No new packages. The app crate now depends on `syntrac-llm`.

## Tests (written first)
Core `tests/server_launch.rs` (9). Testkit `tests/server_manager.rs` (11): start and the status path, slow start, refused commands, never a second server, stop only its own, exit code, a failed start and a failed stop, and the quit setting. App `tests/ai_control.rs` (9): off and not set up ask nothing, refused settings keep nothing, the exact command is shown, start and stop, a server already running is never started again or stopped, an ended server, quit. App `tests/server_process.rs` (5, REAL processes): the exit code, the working folder, a missing program or folder, the rules checked again, and that stopping ends the program and the program it started (`cmd.exe` and the `ping.exe` it starts, counted in the job).

## Limits (as designed)
- The job is joined just after the program starts: a program that starts its own helpers in its first moments could escape it. A server that loads a model first is not affected in practice.
- A server started by an earlier run of this app (or by hand) is shown as running but cannot be stopped from the app.
- The server's own output is discarded; its own log files, if it writes any, are the place to look when it does not start.

## Next
L4d: the Investigate button on findings and programs, the streaming panel with cancel and the error states, the "copy a prompt for an outside model" box with its editable preview, the AI help settings section with the server status, and the user guide.
