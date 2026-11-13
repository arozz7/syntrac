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

# Run the collector in an elevated window (UAC prompt); summary goes to aiChangeLog/console-run-*.txt
powershell -ExecutionPolicy Bypass -File scripts/dev/run-console.ps1 -Seconds 60
# Install as a Windows service and check it end to end as an ordinary user (one UAC prompt)
powershell -ExecutionPolicy Bypass -File scripts/deploy/smoke-test-service.ps1
# Ask a running collector (service, or console with --pipe syntrac-console) how it is doing
target/release/syntrac-service.exe --status
# Uninstall (keeps your data unless you pass -RemoveData)
powershell -ExecutionPolicy Bypass -File scripts/deploy/uninstall-service.ps1   # run elevated

# ...then review a console run's database (read-only):
python -I scripts/dev/inspect-run.py data/syntrac-<stamp>.db

# Collector without ETW (no elevation): startup snapshot, metrics, storage
target/debug/syntrac-service.exe --no-etw --seconds 20 --db data/try.db

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



