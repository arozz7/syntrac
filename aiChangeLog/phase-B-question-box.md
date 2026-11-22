# Phase 5, slice B: the question box (L4e)

A free-form question for the optional AI help. The model server on this machine may look things up in what SYNTRAC recorded, with four tools that
only read. Off unless the AI help is on; nothing runs until **Ask** is pressed.

## What was added
- **`crates/syntrac-core/src/tool_chat.rs`**: a chat turn that may ask for tools. A turn is sent whole and answered whole (not streamed), because
  the pieces of a streamed tool call are what local servers get wrong most often. The request body in the common chat API's tool form, and the
  reading of one reply (missing ids get one, a call with no name is dropped, arguments given as an object become JSON text, reasoning is counted,
  a length stop is marked cut off).
- **`LlmService::chat_turn`** (default: `ToolsUnsupported`), **`HttpLlm::chat_turn`**: the reply is read through a byte limit; a refusal whose text
  mentions tools or the template becomes the distinct `LlmError::ToolsUnsupported`, but only for a request that offered tools.
- **`crates/syntrac-core/src/question_loop.rs`**: the `ReadOnlyTool` trait (read only by construction), `ToolRegistry`, and the bounded loop.
  - At most 6 calls (refused and repeated calls count, so a looping model still ends), a time budget, a conversation budget in bytes, and Stop.
  - Then one last turn **without tools** asks for an answer from what was gathered.
  - A tool that is not registered is never run and its name is not echoed back. Arguments must be a JSON object, within a size.
  - Each result is a marked block; angle brackets in it are neutralised so it cannot close its own block; the user name is replaced; it is cut to a size and counted against the conversation, and a cut is marked.
  - The same call written differently is run once.
- **`config/ai/question.toml`** (+ `question_config.rs`): the wording and the tool descriptions, outside the code.
- **`investigate::safe_text`**: the evidence's own cleaning, shared with the tools (no second redactor).
- **`app/src-tauri/src/question_tools.rs`**: four tools: `program_facts`, `list_findings`, `top_programs`, `program_family`. Rendered at the person's detail level. Below *Full* no addresses, hashes, account names or **command lines**; at *Full* command lines are cleaned, cut, have the values of arguments that name a secret (`--token=`, `/password:`, `--api-key x`) replaced by `[hidden]`, and the user name replaced. A path is only looked up in what the store recorded, never read from the disk.
- **`question_data.rs` / `question_backend.rs`**: the facts, through the same read-only views as the screens (generic over the repositories, so they are tested on the in-memory store).
- **`question_job.rs`** + three commands: start, poll, cancel. The page lists every tool call, with its status.
- **Frontend**: `QuestionBox` on the AI help tab (only while the help is on); `hooks/useJobView` extracted from the Investigate panel and shared.
- **Demo stand-in model**: a turn with tools and no tool result gets a tool call; a turn with a result answers with names copied from it. `demo-ai-check.ps1` asserts the page lists the call, the answer repeats program names from the demo database, the model was asked twice, and the request offered exactly the four read-only tools. Picture: `ai-question-box`.

## Decisions
- **Non-streamed tool turns.** Slower to show text, but robust across servers.
- **Server without tool support.** llama-server answers an error when started without its template option; the page says so in generic terms and names that option. A server that ignores `tools` and answers in text is simply a final answer.
- **Tools are not saved.** Unlike Investigate, answers to questions are not kept.

## Review fixes (after the first commit)
- Stop shows **at once**: a non-streamed turn cannot be interrupted, so `cancel` sets the job to cancelled immediately and a late answer is dropped (test with a model that blocks).
- `chat_turn` waits for the whole answer, not 10 minutes for headers (the first-byte timeout only covers the headers, which a non-streamed reply sends last).
- The `<user>` and `<address>` placeholders the tools write reach the model unchanged; any other angle-bracket text is still neutralised.
- The stand-in model's text no longer has stray indentation.

## Found on the user's own server
A real server answered with the call as text, `<tool_call>{"name": ..., "arguments": ...}</tool_call>`, in the answer instead of a structured call (the server was not parsing tool calls), so nothing ran and the page showed the raw text as the answer. `parse_turn_reply` now reads tagged calls (at most 8 per reply, only when the server gave no structured ones; a block that is not a call stays in the text). Tests first in `tests/tool_chat.rs`.

## Learned from the owner's other agent project (same local server)
The owner's separate coding-agent project (its tool-calling notes and parsing code) records how this server behaves: its non-streaming reply is the raw text with `<tool_call>` tags (no structured field), it runs with server-level thinking (traces of ~1,500 tokens are ordinary, and a turn that runs out of room inside its thinking returns nothing), and it passes `role: "tool"` straight to the chat template. Applied here, tests first:
- `<think>` traces are removed from the reply and counted, so a call merely mentioned while thinking is never run, and an unfinished trace leaves no answer.
- A call the model wrote as text goes back as text in the model's turn (tags included) with no structured `tool_calls`, so the template shows the model its own call.
- A `<tool_call>` tag that cannot be read (broken JSON, or cut off) is told to the model (`bad_call_nudge` in `question.toml`), at most twice, instead of being shown as the answer.
- The per-turn answer cap is 6,000 tokens (was 1,500), for the same reason that project raised its own to 8,192.

## Checked against a real server
After the fixes above, the owner asked a real question against their own local server: one `top_programs` lookup was made and listed on the page, and the answer stayed within what the result showed (and said what it had not looked up). Only one server and one question so far.

## Also fixed
- Process tree (A1): the window is read newest first up to 20,000 processes; the view now says when more existed (commit `563c50d`).

## Tests (written first)
`tool_chat` (15), `http_turn` (10), `question_loop` (24: scripted calls, unknown tool, bad arguments, caps, cancel, repeat, size and context cuts, user name, a hostile result), `question_config` (5), `safe_text` (3), `question_tools` (19), `question_data` (7), `question_job` (10), demo model (5), `QuestionBox` (11), AI help page (1).
