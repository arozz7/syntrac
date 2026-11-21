# Phase L4d: the optional AI help, step four: the Investigate button and the AI help settings (no schema change)

Slice L4d of `docs/PHASE4-PLAN.md`. This completes the first release of Phase 4 (L4e, a free-form question box, stays optional and later).

## What changed
- **App backend `ai_investigate.rs`:** `Investigator`: `preview` (the prompt as it would be sent, or the reduced `external` text; asks no server and does not need the help turned on), `start` (checks the settings, reads the evidence, builds the prompt at the user's detail level, refuses when the server is not running unless the user chose "start it when I press Investigate", then streams the answer on its own thread), `poll`, `cancel`. The answer is kept as plain text; reasoning from the model is counted, never shown. The last 8 investigations are kept. `ai_control.rs` gained `ensure_started` (a server that already answers is not a failure).
- **App `investigate_evidence.rs`:** `evidence_for_subject` (a finding by number, or a program by path); `Backend::investigation_evidence` reads it from the database. Commands: `ai_preview_prompt`, `ai_start_investigation`, `ai_poll_investigation`, `ai_cancel_investigation` (in `ai_commands.rs`).
- **Page:** a new **AI help** tab (`views/AiHelp.tsx`): the on switch, server address, model name, how much is sent, the optional launch command (one argument per line), the two start and stop choices, the exact command, the server status with Start and Stop. An **Investigate** button on every finding and on a program's detail (`components/InvestigatePanel.tsx`): "Investigate with my model" (streams, Stop, "What was sent"), and "Copy a prompt for an outside assistant" (an editable box with exactly what will be copied, a Copy button, and the statement that SYNTRAC sends nothing). Nothing AI-related is shown while the help is off (`components/AiEnabled.tsx` reads the switch once for the whole window). Styles in `theme/ai.css`; types in `api/aiTypes.ts`; test defaults in `test/fakeAi.ts`.
- **Docs:** user guide section "AI help (optional)", privacy bullet, README.
- No new packages.

## Tests (written first for the backend)
App `tests/ai_investigate.rs` (10): the streamed answer and what was sent (no user name), nothing runs while off or not set up, a server that is not running is reported and not started unless chosen, starting and waiting for the server, a server that ends while awaited, a failing server, cancel (the model is told to stop and the partial answer is kept), a missing subject, the preview without the help on (and no server asked), old investigations dropped. `tests/investigate_evidence.rs` (+1). Page: `InvestigatePanel.test.tsx` (7), `AiHelp.test.tsx` (5): hidden while off, nothing asked until pressed, the answer as text (markup is not interpreted), failures, Stop, the editable copy text and the clipboard, settings saved and refused, the exact command shown, Start only on press, Stop only for its own server.

## Limits (as designed)
- Cancel takes effect between pieces of the answer; a model still reading a long question holds the connection until its first word or the timeout.
- The conversation-log risk of a model server cannot be asked of the server generically, so the page says it in words.
- The evidence for a finding about one program starting another brings the parent program's facts (see L4b).
- The page polls an investigation every 0.7 s while it runs, and the server status every 3 s while the AI help tab is open; nothing polls otherwise.
