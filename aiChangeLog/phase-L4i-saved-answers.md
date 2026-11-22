# Phase L4i: saved answers, and the program click (no schema change)

The user, testing the release app: "when we click on the program in history it should take the user to the details, right now it seems nothing happens. I scrolled down manually to see. Should we store the output so in the future we do not need to leverage the AI if we have already investigated in the past? Maybe we can note that up by the program list."

## 1. Choosing a program now shows its details
The detail card opens below the whole ranking table, so with a long list the click seemed to do nothing. `ProgramDetail` now scrolls its card into view and moves focus to its heading when it opens (or when another program is chosen). The Known programs tab uses the same card and gets the same behaviour. Test (written first, failed): `ProgramsHistory.test.tsx`.

Found only by driving the real app (the page tests cannot see it): the card is short while its data loads, so the scroll was clamped near the bottom of the page and the browser's scroll anchoring then kept the page pinned to the bottom as the card grew (the card ended 1,100 px above the window). The card now has a minimum height of one screen (`.program-detail`), so it can be scrolled to at once. The heading is scrolled to first and focused after (with `preventScroll`), and needs no focus ring since it is not a tab stop. The demo check measures the card's position in a 700 px high window.

## 2. Saved answers
- **Where:** `app/src-tauri/src/investigation_store.rs`. The `InvestigationStore` interface (`get`, `put`, `forget`, `list`) and `JsonInvestigationStore`, a JSON file next to `app.json` (`investigations.json`). One answer per subject: a program (path, compared without letter case) or a finding (id). The newest 500 are kept. A missing, damaged or BOM-prefixed file reads as expected, and a file that cannot be written leaves the answers in memory for the run. Not in the database on purpose: the app opens the database read-only and the service owns every write, so that would have needed a migration and a service command for a few short texts. The interface lets it move later without touching the screens.
- **What is kept:** the answer, whether it was cut off, the model name, the detail level, the evidence block that was sent and the time. Only an answer that finished with text is kept: failed, stopped and thinking-only runs keep nothing.
- **When:** `Investigator` keeps the answer on the job's thread *before* the job reads as done, so a page that sees "done" and asks for it always finds it (tested).
- **Commands:** `ai_saved_investigation`, `ai_saved_list` (subjects and dates only, no answers) and `ai_forget_investigation`.
- **Page:** Investigate shows a kept answer at once ("Saved answer from <date>", model, the same notes and *What was sent*), with *Ask again* and *Forget the saved answer*; nothing is asked of the model until Ask again. A small *AI note* beside programs (History, Programs, including a grouped product when any of its programs has one, and its members) and findings shows what was investigated before. `AiEnabledProvider` reads the marks once for the window, only while the help is on.

## Tests (written first)
Store `tests/investigation_store.rs` (12): round trip, case-insensitive path, replace, program and finding kept apart, forget, the list holds no answers, the limit, restart through the file, missing file and folder creation, damaged file, byte-order mark, unwritable file. Investigator `tests/ai_investigate.rs` (+5): a finished answer is kept with its details before "done", cut-off answers are kept and flagged, failed / stopped / empty runs keep nothing, asking again replaces, programs and findings are separate and forgetting removes one. Page: `InvestigatePanel.test.tsx` (+6), `SavedNote.test.tsx` (3), `Findings.test.tsx` (+1), `ProgramsHistory.test.tsx` (+2, including the scroll and focus test).

## Demo check
`scripts/dev/demo-ai-check.ps1` now also checks, in the real app on the fictional machine: the finding is marked, reopening shows the saved answer and asks the model nothing (by the stand-in's request log), a program from the list opens in view with focus on it, a program investigation is marked in the list, and Forget removes the mark. It also takes `ai-saved-answer` and `history-programs-ai-note` pictures.

## Also
Single backslashes in strings written through the shell collapse (again): `no-useless-escape` and a failing test caught them; such text is now written with the editor tools.

## Pictures and README
The README screenshots were taken again from the demo app (`run-demo.ps1 -Capture`; the page now has the AI help tab in its menu and the program details picture scrolls back to the top first). New: `ai-help.png` (the AI help tab) and `history-programs-ai-note.png` (the AI note beside a program), both from the demo check with the stand-in model. `ai-investigate.png` is kept: it is a real small model's answer, and no model server was running to take it again. The README's AI help section now covers starting the server from the app, the thinking and cut-off notes, and saved answers.
