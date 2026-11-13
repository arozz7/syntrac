# SYNTRAC

**System intelligence and activity tracking** for Windows: a personal workstation intelligence platform that answers

1. **What is happening?** Processes, network connections, configuration changes.
2. **What is normal?** Learned baselines and per-application profiles.
3. **What is unusual?** Explainable detections. An anomaly is not a threat.

Local-first. Nothing leaves your machine. AI features (Phase 4) use your own local llama-server.

## Status

Phase 0 (Foundations) in progress. See [docs/PLAN.md](docs/PLAN.md) for the full plan and
[aiChangeLog/](aiChangeLog/) for per-phase change logs.

## Architecture in one paragraph

An elevated Windows **service** collects telemetry (ETW) and writes it to SQLite. An unelevated **Tauri 2 + React**
app reads the data and shows it. Pure logic lives in `syntrac-core` behind interfaces, so the whole pipeline runs
offline against mocks. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Requirements

- Windows 11 x64
- Rust (stable, MSVC toolchain) and the Visual Studio C++ Build Tools
- Node.js 22+ and pnpm
- WebView2 runtime (ships with Windows 11)

## Everyday commands (PowerShell)

```powershell
# Full local gate: format, lint (warnings are errors), build, tests, file-size check
powershell -ExecutionPolicy Bypass -File scripts/dev/verify.ps1

# Run the desktop app in dev mode
cd app; pnpm tauri dev

# ETW feasibility spike (opens an elevated window; UAC prompt). Open/close a few programs while it runs.
powershell -ExecutionPolicy Bypass -File scripts/dev/run-etw-spike.ps1 -Seconds 30 -ProcessKeywords 10
```

There is deliberately **no hosted CI**. `verify.ps1` is the gate; run it before every push.

## Repository layout

| Path | Purpose |
|---|---|
| `crates/syntrac-core` | Domain types, interfaces, pure logic (correlator, ingest) |
| `crates/syntrac-store` | SQLite repositories and migrations |
| `crates/syntrac-collector` | Telemetry sources (ETW) and enrichers; contains the Phase 0 spike |
| `crates/syntrac-service` | Windows service host (placeholder until Phase 1) |
| `crates/syntrac-testkit` | Mocks, in-memory repository, fixtures, repository contract suites |
| `app/` | Tauri 2 shell and React/TypeScript UI |
| `config/` | Rules, thresholds, prompts, llama-server endpoint (later phases) |
| `docs/` | Plan, architecture, schema, threat model |
| `aiChangeLog/` | One file per phase |
| `scripts/` | `dev/` verification, `release/` and `deploy/` (later) |

## Theme

Light canvas (cream and sand) built on the SYNTRAC Brand Kit in `docs/branding/SYNTRAC_Brand_Kit/` (forest green,
sage, clay). Colours live only in `app/src/theme/tokens.css` and are verified against WCAG AA by an automated test.
