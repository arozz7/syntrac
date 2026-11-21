# Phase L4a: the optional AI help, step one: a client that talks to a model server on this machine only (no schema change)

Slice L4a of `docs/PHASE4-PLAN.md` (approved 2026-11-21). Nothing in the program uses it yet: the Investigate button comes in L4d.

## What changed
- **Core `llm.rs` (pure):** `AiSettings` (off and empty by default, `DetailLevel` summary or full, `ready_to_use()` as the one gate), `LoopbackEndpoint::parse` (plain `http` only; host `127.x.x.x`, `localhost` or `[::1]`; no user name, query, fragment, backslash, space or `..`; `localhost` is stored as `127.0.0.1` so no name is ever resolved), `ChatRequest` and `chat_body`, `parse_stream_line` (answer text and reasoning text kept apart, end marker, errors inside the stream), and the `LlmService` port with `ServerState`, `LlmError`, `StreamEvent` and `Flow`.
- **New crate `syntrac-llm`:** `HttpLlm`, the adapter for `LlmService` on a small blocking client. It follows no redirects, uses no proxy from the environment, sends no credentials, treats a status such as 503 as data, and bounds the connect time, the wait for the first word (long: a big model reads a long question first), the whole answer, one line and the answer size. Readiness is `/health` at the server root, else `/models` under the API path; 503 means loading; nothing listening means not running.
- **App settings:** `Prefs` gained `ai: AiSettings` (kept in `%LOCALAPPDATA%\Syntrac\app.json`, an older file without it loads with the defaults); `PrefsHandle::ai_settings` and `set_ai_settings`.
- **Testkit:** `MockLlm`, a scripted model server for later slices.
- **New packages:** `ureq`, `ureq-proto`, `utf8-zero` (no TLS, no compression: it only ever talks to loopback).

## Tests (written first)
Core `tests/llm.rs` (15): accepted and refused addresses (including `127.0.0.1.evil.example`, `localhost.evil.example`, `0.0.0.0`, `@` tricks, `https`, port 0), localhost as the numeric address, settings gate, request body, stream lines. `syntrac-llm/tests/http_llm.rs` (14, against a scripted server on loopback): ready, ready by model list, loading, not running, a probe that times out, a streamed answer with reasoning apart, the request path and body and no credentials, stop, 503 with `Retry-After`, other statuses, a redirect that is never followed (the target is never contacted), an error in the stream, the answer-size cap, an end without the marker. App `tests/prefs.rs` (+2). Testkit `tests/mock_llm.rs` (2).

## Limits (as designed)
Cancel takes effect between pieces of the answer; a model still reading its question can hold the connection until the first word or the first-word timeout. The conversation-log warning (a server that stores prompts) is not built here: no generic way to ask a server about it exists, so it comes with the settings screen in L4d as a plain notice.

## Next
L4b: the evidence bundles and the three detail levels.
