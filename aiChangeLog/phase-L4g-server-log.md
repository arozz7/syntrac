# Phase L4c follow-up: seeing why a started server ended (no schema change)

Found while trying the AI help with a real model server: the app showed "The server this app started has ended with code 1" and nothing else, because the server's output was discarded and the status did not show the working folder.

## What was found
Run from a folder without its `config.toml`, the real server prints "config file not found, using built-in defaults" and then "failed to spawn llama-server: program not found" and exits (code 1). Run from its own folder, through the app's own spawner, it starts and loads its model. The user's working folder was set, so that was not the cause of the reported exit; a likely one is the server's own start-up wait (a fixed number of seconds for its model to load), which a first cold load of a large model can exceed. Without the server's output this could not be seen.

## What changed
- `WindowsSpawner::with_log` and `with_optional_log`: what the program prints (output and errors) is saved to `%LOCALAPPDATA%\Syntrac\model-server.log`, a new file at each start (it cannot grow across starts); a log that cannot be written never stops the start.
- `AiControl::with_server_log`: the status gained `workingFolder`, `logPath` and `logTail` (the last 8 lines, each cleaned and cut to 200 characters, read from the last 32 KB), shown when the server ended or is slow to start.
- Page: the AI help tab shows the folder the command runs in (or that none is set and what that means), the last lines the server printed as plain text, and the log's path.
- `examples/try_launch.rs`: starts a command through the same spawner and job object and prints what happened and the log's last lines.
- Guide: "If the server will not start or ends".

## Tests (written first)
App `tests/server_process.rs` (+2, real programs): output and errors land in the log, the next start begins a new log, no log discards as before. `tests/ai_control.rs` (+4): the last lines and the path after an exit, cleaning and limiting, a running server or a missing log shows none, the working folder shown. Page: `AiHelp.test.tsx` (+2).

## Limits
The log of a server that runs for days grows for that run (it is only started fresh at the next start); a server with its own log files is the better place for long histories.
