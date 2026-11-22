# Phase 5 plan (approved 2026-11-21)

Four pieces, in this order. Each slice: tests first, a change log in `aiChangeLog/`, `scripts/dev/verify.ps1` green, and the staged diff scanned
before the commit. Pushed once per phase, when asked.

| Step | What | State |
| --- | --- | --- |
| 0 | Split `api/types.ts` and `test/fakeApi.ts` by topic (no logic change) | done, `0ca0530` |
| A1 | History, **Process tree**: who started whom (`aiChangeLog/phase-A1-process-tree.md`) | done, `991e183` |
| A2 | History, **Connection map**: which programs talked to which networks (`aiChangeLog/phase-A2-connection-map.md`) | done, `48eb720` |
| B | **L4e** free-form question box for the AI help, with read-only tools (`aiChangeLog/phase-B-question-box.md`) | done, `16e2754` + `445c878` |
| C | Windows **installer** (NSIS via Tauri) that installs the app and the service (`aiChangeLog/phase-C-installer.md`, `docs/RELEASE.md`) | done: C1-C3 built, sandbox test passes (28 checks, 2026-11-22), C4 docs and version 0.7.0 (`v0.7.0`) |

Out of scope for now: a day timeline and a global search.

## B: the question box
- **Memory:** the read-only readers the app already has plus the saved-answers store. **Tools (read-only functions):** a program's facts, the
  findings, the busiest programs, a program's destinations, the process tree. **Reasoning loop:** bounded (about six tool calls, a time budget, a
  Stop button). A `MockLlm` scripts tool calls and a `MockTool` answers them, so the loop runs offline.
- **Safety:** no tool can write, start or stop anything. Arguments are validated; unknown tools are refused. Results get the same reductions as the
  current evidence (user name replaced, detail level respected), are cleaned and size-capped, and are marked as data in the prompt. Loopback only.
  The page lists every tool call that was made.
- **As built:** four tools, not five: a program's destinations are part of `program_facts`. Tool turns are not streamed. Answers to questions are
  not saved.
- **Dependency:** OpenAI-style native tool calling, which some servers need a flag for (for example `--jinja` on `llama-server`). If the server does
  not support it, the box says so.

## C: the installer
- A Tauri NSIS per-machine installer that installs the app and the service, reusing `scripts/deploy` as the single source of truth. The service's user
  id must belong to the person who runs the app. Uninstall keeps data and configuration by default. An upgrade runs in place and rolls back on failure.
- Slices: C1 the build script `scripts/release/build-installer.ps1`; C2 install, upgrade and uninstall behaviour; C3 a smoke test in Windows Sandbox;
  C4 docs and version tagging.
- Signing needs a certificate the owner must supply: the installer is unsigned for now, with a documented SmartScreen warning and a signing step that
  stays off.
