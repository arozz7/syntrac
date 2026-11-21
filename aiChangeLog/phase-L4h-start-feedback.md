# Phase L4c/L4d follow-up: the Start button answers at once (no schema change)

The user, after the first successful load of their real model server: "There is a delay of a few seconds when you click on start that leaves the user unsure if they clicked it before anything indicates its loading."

## Causes and fixes
- **The page said nothing until the program answered.** `AiHelp` now shows "Starting the model server..." (or "Stopping...") the moment the button is pressed, with the button turned into a disabled "Starting..." / "Stopping...", and reads the status every second while that is pending instead of every 3 s. The note ends when the program reports that the server is starting, ready or loading (for a stop: no longer stoppable), when the command is refused (with the reason), or after 20 s at most.
- **A Start or Stop could wait behind a status probe.** `ServerManager::status` and `start` made their network probe while holding the manager's lock, so a slow probe (up to a few seconds) held up a button press. Both now probe before taking the lock.

## Tests (written first)
Testkit `tests/server_manager.rs` (+1): a status call stuck in its probe does not hold up a start (it failed before the fix). Page `AiHelp.test.tsx` (+3): the starting note and the disabled button appear before the program has answered, a refused start goes back to normal with the reason, the note ends when the program reports the server is starting.

## Also found while testing
Single backslashes in strings written through the shell collapse; the lint rule `no-useless-escape` caught new test lines written that way.

## Found with the real 27B model on the demo (first words 135 s, answer done at 204 s)
The answer stopped in the middle of a sentence and the page said "Finished.". The model thinks before it answers and that thinking counts against the length limit (1,200 tokens), so the limit was reached. Also, during the two minutes of thinking the page only said "The model is answering..." with nothing visible.
- `StreamEvent::CutOff`: produced when the stream's `finish_reason` is `length` (core `parse_stream_line`, tests in `tests/llm.rs`; adapter and `MockLlm::playing` covered). The investigation carries `truncated`; the page says "Finished, but cut off." with a note, and never "Finished." for such an answer.
- The limit is now 3,000 tokens (Stop ends an answer sooner).
- While the model has produced thinking but no answer, the page says "The model is thinking... (N characters of thinking so far). A model that thinks first can take a minute or two before its first words."
- Tests: core (+2), adapter (+1), testkit (+1), app `ai_investigate` (+2), page (+3).
