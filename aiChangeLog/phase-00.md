# Phase 00: Foundations

Date: 2026-11-12. Status: **complete except the ETW spike measurement, which needs an elevated run by the user.**

## Goal

Feature-free foundation: repo and folder convention, Cargo workspace, tests-first core types and interfaces,
SQLite store, Tauri + React shell with the earthy theme, local verification gate, docs, ETW spike.

## Plan changes made at the start of the phase

- UI theme: light, earthy canvas instead of a dark interface. Later in the phase the owner added the SYNTRAC Brand
  Kit (`docs/branding/SYNTRAC_Brand_Kit/`); its palette, logos and icon were adopted, but its charcoal app canvas was
  not, because it conflicts with the earlier non-dark request. **Owner to confirm this reading.**
- AI: local only, via the user's llama-server (OpenAI-compatible HTTP API). Ollama and cloud providers dropped.
  llama-server host/port/model recorded as an open Phase 4 question (not found on PATH or listening).

## What was built

| Area | Result |
|---|---|
| Repo | `git init` (local, no remote, no commits yet), mandatory folders, `.gitignore`, `rustfmt.toml` |
| Workspace | 5 crates + `app/src-tauri`: `syntrac-core`, `-store`, `-collector`, `-service`, `-testkit`, `syntrac-app` |
| Core | `Timestamp`, `Clock`, `ProcessInstanceId` (UUIDv5 of pid + creation time), `EventEnvelope`/`EventPayload`/`Severity`, ports (`ProcessRepository`, `EventRepository`, `TelemetrySource`), `correlate`, `ingest` |
| Testkit | `InMemoryRepository`, `MockTelemetrySource`, `ManualClock`, fixtures incl. the beacon + PID-reuse scenario, repository contract suites |
| Store | `SqliteStore` (WAL, `user_version` migrations, newer-schema rejection) passing the same contract suites |
| App | Tauri 2 + React 19 + TS shell; brand-kit theme tokens in `tokens.css`; brand logo and generated app icons; WCAG AA contrast test |
| Tooling | `scripts/dev/verify.ps1`, `check-file-size.ps1`, `run-etw-spike.ps1` (all Windows PowerShell 5.1 compatible) |
| Docs | PLAN, ARCHITECTURE, SCHEMA, THREAT-MODEL, README |
| Spike | `crates/syntrac-collector/src/bin/etw_spike`: Kernel-Process, Kernel-Network, DNS-Client counts, samples, own CPU % |

## Tests (written before the code they cover)

- `syntrac-core`: envelope serde round-trip for every payload; category/kind/pid mapping; severity ordering;
  process identity (same input same id, PID reuse gives a new id, JSON string form).
- `syntrac-testkit`: repository contract (process + event) against in-memory; pipeline scenarios (parent linking,
  beacon attribution, PID reuse, unknown PID stays unattributed, out-of-order delivery, idempotent re-drain).
- `syntrac-store`: same contracts against SQLite; PID reuse through SQLite; persistence across reopen; migration
  idempotence; newer-schema rejection.
- `app`: every text/surface colour pair meets WCAG AA (4.5:1); chart colours meet 3:1 against cards; severity tiers
  distinct from each other and from the surfaces; brand and chart colours cannot be mistaken for alert colours.

Final gate: `scripts/dev/verify.ps1` exit code 0 (see the last verify run at the end of the session: frontend
format/lint/typecheck/47 tests/build; Rust fmt, clippy `-D warnings`, build, 25 tests; file-size check).

Launch check: `pnpm tauri dev` was started with a WebView2 debug port and the rendered DOM was inspected: React
mounted, cream background, brand chip colours, logo loaded. This is not a visual review; the owner should look at the
window. The earlier concern about the strict CSP blocking the dev page did not occur.

## Decisions and findings

- **Theme findings (tests caught all of these):**
  1. My first severity palette failed tier distinctness (unusual vs suspicious RGB distance 28, required 60).
  2. My first fix put the brand accent (clay) about 21 away from the suspicious tier, so a clay button would read as
     an alert. Added brand/chart-vs-alert distance checks and moved the accent away from alert hues.
  3. Against the brand kit: Critical `#C96B5B` fails AA with text (3.8:1), so the confirmed fill is a darker
     derivative `#A3483A` and Critical stays as the indicator colour. Charcoal on Sage is 4.1:1, so the accent fill is
     a lightened sage. Clay is 2.4:1 as a chart line, so chart-3 is a darkened clay. Originals remain as `--brand-*`.
  Tiers also differ by label and shape (circle, triangle, octagon) so colour is never the only cue.
- **ferrisetw maintenance:** 1.2.0 was released 2024-06-27 (about 17 months ago), 176k recent downloads. Still used,
  but not actively released; kept behind `TelemetrySource` so raw `windows-rs` can replace it.
- **App icon:** regenerated from the brand SVG with `tauri icon` (SVG input works); source is
  `app/src-tauri/app-icon.svg`.
- **llama-server (Phase 4 question answered):** the owner's own local model server fronts llama-server with an
  OpenAI-compatible proxy on loopback, default model a 27-billion-parameter one. It was read, not run. Two
  findings recorded in PLAN and THREAT-MODEL: requesting another model name triggers a reload, and its
  `log_conversations = true` stores prompts in plaintext.
- **Toolchain:** `typescript-eslint` does not support TypeScript 7, so TypeScript is pinned to 5.9.3.
- **Tauri embeds `app/dist`** at compile time, so `verify.ps1` builds the frontend before the Rust steps.
- **MSVC check passed:** bundled SQLite (C) compiled on the first build.
- `ferrisetw` 1.2.0 (MIT/Apache-2.0) uses `windows` 0.57; the spike pins the same version to avoid a duplicate.

## File moves / deviations

- In-memory repository placed in `syntrac-testkit` rather than `syntrac-store` (avoids a dependency cycle with the
  shared contract suites).
- Deleted with the owner's explicit approval (2026-11-12): `app/src-tauri/icons/android/`, `app/src-tauri/icons/ios/`
  (generated by `tauri icon`, unneeded for a Windows-only tool) and the superseded placeholder
  `app/src-tauri/app-icon.png`. They are reproducible with `pnpm tauri icon src-tauri/app-icon.svg`.
- No other files were deleted, and no existing code was refactored.

## ETW spike

Round 1 was run by the owner on 2026-11-12 (28 cores, 30 s, unfiltered, normal use). Raw output:
`aiChangeLog/etw-spike-result-kw0.txt`. This also proves `run-etw-spike.ps1` (UAC relaunch + output capture) works.

| Provider | Result |
|---|---|
| Kernel-Process | 101,557 events (3,385/s), unfiltered. Event ids 21, 8 and 7 are about 85% of the volume. **No process start/stop event appeared in the sampled output** |
| Kernel-Network | 5,952 events (198/s). PID and `daddr` parse correctly; `dport` is in **network byte order** (157.240.241.63 showed 47873 = 0xBB01, i.e. 443 swapped) |
| DNS-Client | 29 events (1/s). Query name parsed on event 3006 (`wpad`); event 1001 has no `QueryName`. Header PID differed between events (3368 vs 3712), so which PID is the real caller is unconfirmed |
| Consumer-side CPU | 0.109 s over 30 s on 28 cores = 0.01%. Excludes emission cost in other processes and the kernel |

Answers to the Phase 0 questions:
1. **Kernel-Process `CreateTime` / `ParentProcessID` / `ImageName`: NOT answered.** The round-1 sampler kept only the
   first six events, all noise. Round 2 filters to process events and samples every event id.
2. **Kernel-Network: answered.** PID and remote address available; ports need `swap_bytes`. Mapping event ids to
   meanings (connect, send, receive, close, TCP vs UDP) is still to be done from the manifest in Phase 1.
3. **DNS-Client: partly answered.** Query names exist; PID attribution needs confirming.
4. **Overhead: promising but unproven.** Receiving about 3,400 events/s costs about 0.01% of the consumer's CPU, which
   suggests filtering for volume reasons is not urgent, but filtering is still advisable (the noise events carry no
   value for SYNTRAC).

**Round 2 (needed before Phase 1):** `powershell -ExecutionPolicy Bypass -File scripts/dev/run-etw-spike.ps1 -Seconds 30`
(default Kernel-Process keyword mask 0x10 = process start/stop; the mask comes from the provider manifest and is not
yet verified here). **During the run, open and close a few programs (Notepad, Calculator)** so process events occur.
Output goes to `aiChangeLog/etw-spike-result-kw10.txt`. If the Kernel-Process section shows 0 events, rerun with
`-ProcessKeywords 0` to get the unfiltered stream with per-event-id samples.

What to look for in round 2: event 1/2 samples showing `pid`, `parent_pid`, `create_time_raw` and `image`; whether
`create_time_raw` is a FILETIME (100 ns ticks since 1601) to convert; DNS samples across event ids.

Troubleshooting:
- **A provider reports 0 events:** suspect the keyword mask. Round 2 sets the Kernel-Process mask explicitly.
- **Trace start fails with "already exists"** (for example after a killed run): `logman stop SyntracSpike -ets`.
- **The CPU figure is consumer-side only**, so treat the 2% budget as not yet proven even though the number is low.

## Decisions carried into Phase 1

- `ferrisetw` works on this machine and parses fields, so it stays the first choice, behind `TelemetrySource`.
- Network ports must be byte-swapped. Add a test for that in the Phase 1 network source.
- Do not subscribe to unfiltered Kernel-Process in production.

## Next

Round 2 of the spike, then Phase 1 (Visibility).
