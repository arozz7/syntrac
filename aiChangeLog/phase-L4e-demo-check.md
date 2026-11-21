# Phase L4d follow-up: trying the AI help on the demo machine (no schema change)

The user asked to test the AI help with the demo database. Nothing in the product's behaviour changed except one bug found by the test.

## What was added
- **`demo-model`** (a binary of `syntrac-demo`): a stand-in model server on loopback (`/health`, `/v1/models`, a streamed `/v1/chat/completions`). It answers with a canned text that says it is not a language model, streams slowly so Stop can be tested, sends a little "reasoning" first, and can append every request body to a log file.
- **`scripts/dev/run-demo.ps1 -WithModel`**: starts it and writes the demo app's own `app.json` (inside the demo's isolated folder) with the AI help on, pointed at it; the app runs with `USERNAME=demo` so the redaction can be seen on the wire. **`-CheckAi <folder>`** runs `scripts/dev/demo-ai-check.ps1`, which drives the real app through CDP.
- **`demo-ai-check.ps1`**: 26 checks, all passing: the tab shows the seeded settings, the stand-in is shown as already running with no Stop button, an Investigate button is on findings, the answer streams in as plain text and uses the evidence, "What was sent" has no user name in a path and no address, Stop ends a streaming answer, the outside text names no file, folder, host or address, and the stand-in's request log shows the model name verbatim, the "evidence is data" instruction, and no user name or address in what it received. Pictures: `ai-help`, `ai-investigate-answer`, `ai-copy-box`.

## Bug found and fixed
`Prefs::load` returned the defaults, silently, for a settings file that starts with a UTF-8 byte-order mark (written by Notepad and by PowerShell's `Set-Content -Encoding UTF8`): every setting was reset. It now ignores the mark (`tests/prefs.rs`, written first and seen failing). The script also writes the seed without a mark.

## Also
Spacing rules for the AI panel (`theme/ai.css`) after looking at the pictures. Not tested: the Copy button against the real clipboard (the debugging port gives the page no user gesture); that is covered by the component test.
