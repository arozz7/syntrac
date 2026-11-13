# SYNTRAC: Master Implementation Plan

Status: **Approved** (v1.1, 2026-11-12). Phase 0 built; awaiting ETW spike measurement (user-run, elevated).

SYNTRAC is a personal workstation intelligence platform for Windows. It answers three questions:

1. **What is happening?** Complete visibility into processes, network and configuration.
2. **What is normal?** Learned baselines and per-application profiles ("Application DNA").
3. **What is unusual?** Explainable detections that distinguish *unusual*, *suspicious* and *confirmed*.

Principles: local-first, no hidden collection, no data leaves the desktop, explainable detections, and an anomaly is not a threat.

## Decisions log

| Decision | Choice |
|---|---|
| Audience | Personal tool first. Product concerns (signing, installer, licensing) deferred. |
| Collector | Elevated Windows service plus an unelevated Tauri UI. |
| Telemetry (Phase 1) | Native ETW in Rust, with polling for snapshots. Sysmon is an optional later adapter. |
| Storage | SQLite (WAL) only. DuckDB deferred until queries demand it. |
| AI | Local only, via the user's **llama-server** (llama.cpp, OpenAI-compatible HTTP API). No cloud provider. |
| UI theme | **Light canvas** (cream/sand) using the SYNTRAC Brand Kit palette (forest, sage, clay). The kit's charcoal app canvas is deliberately not used: the owner asked for a non-dark interface. |
| Platform | Windows 11 x64. |
| Verification | Local `scripts/dev/verify.ps1`. No hosted CI unless explicitly requested. |

## 1. Tech stack

| Layer | Choice | Notes |
|---|---|---|
| Collector | Rust, Windows service (`windows-service`) | Elevated, runs independently of the UI |
| Telemetry | ETW via `ferrisetw` (fallback `windows-rs`) | Kernel-Process, Kernel-Network, DNS-Client. Polling for startup snapshot, metrics, IP Helper reconciliation |
| Config monitors | `windows-rs`, WMI | Services, scheduled tasks, Run keys, drivers, firewall |
| Storage | SQLite WAL via `rusqlite` | Service is the only writer, UI reads |
| UI shell | Tauri 2, unelevated | Reads DB read-only; named pipe to the service for commands and live push |
| Frontend | React + TypeScript + Vite, pnpm | TanStack Query, Zustand, Apache ECharts |
| Logging | `tracing`, structured, stable message strings | `info!(id = %id, "Process started")` style |
| Config | TOML/YAML in `config/` | Rules, thresholds, LLM prompts, llama-server endpoint and model |
| AI (Phase 4) | `LLMService` adapter over llama-server's OpenAI-compatible API | Base URL and model in `config/`. Loopback only |

## 2. Architecture (modular)

```
+------------- syntrac-service (elevated, Windows service) --------------+
| Tools/Hands: EtwProcessSource, EtwNetworkSource, EtwDnsSource,          |
|              MetricsSampler, ConfigWatcher, FileEnricher, IntelProvider |
| Brain:       Correlator -> Detector (rules, baselines) -> RiskScorer    |
| Memory:      EventRepository, EntityRepository, ProfileRepository,      |
|              BaselineRepository, AlertRepository                        |
+-------------+-----------------------------------+-----------------------+
              | SQLite (WAL, service writes)      | named pipe (ACL: current user)
+-------------v-----------------------------------v-----------------------+
| syntrac-app (Tauri 2, unelevated): Rust IPC layer + React/TS UI         |
|   Phase 4: LLMService -> llama-server on loopback (read-only query tools)|
+--------------------------------------------------------------------------+
```

Interfaces are defined before implementations:

- **MemoryInterface:** the repository traits above.
- **ToolInterfaces:** `TelemetrySource`, `FileEnricher`, `IntelProvider`, `LLMService`, `Clock`.
- **ReasoningLoop:** `Correlator` -> `Detector` -> `RiskScorer`. Synchronous, pure and stateless; async and channels live only at the edges. All dependencies are injected.
- **Proof of modularity:** the full pipeline runs against `MockTelemetrySource` and `InMemoryRepository` with replayable fixtures, offline.

## 3. Core design decisions

1. **Process identity.** Every process is keyed by `process_instance_id`, derived from `(pid, creation_time)`. Never the PID alone (PID reuse).
2. **Process-to-connection correlation.** ETW network events carry the PID and resolve against the live process table at event time. Pre-existing connections are reconciled from IP Helper snapshots.
3. **Normalized event envelope.** `{id, ts, category, kind, process_instance_id?, severity?, payload}`. All sources map into it, so Sysmon can be added later without touching detectors.
4. **Retention.** Raw events 30 days by default (configurable). Hourly and daily rollups kept indefinitely; profiles and baselines are built from rollups.
5. **Privacy.** Command-line redaction patterns, per-collector switches, no network egress, DB under `%ProgramData%\Syntrac` with restricted ACLs.
6. **Detection tiers.** `unusual` (baseline deviation), `suspicious` (rule match), `confirmed` (intel match). The UI never calls a mere anomaly "malicious".
7. **Baselines.** Simple, explainable statistics first (first/last seen, frequency, percentile bands, co-occurrence). No ML until the data justifies it.

## 4. Theme (light, earthy)

Colors are defined once as design tokens (`app/src/theme/tokens.css`) and never hard-coded in components.

- Source of truth for identity: `docs/branding/SYNTRAC_Brand_Kit/` (guidelines, logos, icons, dark dashboard concept). App assets are copies under `app/src/assets/brand/`; the kit itself is untouched.
- Surfaces: cream and sand. Text: charcoal, muted text slate. Primary: forest green. Secondary: sage. Clay is a restrained highlight. Font: Inter with Segoe UI fallback; tabular numerals for telemetry.
- Severity tiers map to the kit's Info (unusual), Warning (suspicious) and Critical (confirmed). Each also has a label and a shape, never colour alone. The confirmed fill is a darker derivative of Critical because the kit's `#C96B5B` fails AA with text.
- Derived values (lightened sage for accent fills, darkened clay for chart series) exist only where the kit's originals fail contrast; the originals stay as `--brand-*` tokens.
- An automated test (`tokens.test.ts`) checks WCAG AA text contrast, 3:1 graphics contrast for chart colours, tier distinctness, and that brand/chart colours cannot be mistaken for alert colours. Earthy palettes blur warnings together, which is why this is tested rather than eyeballed.
- The dashboard concept in the kit is dark. It is a layout reference for Phase 1, to be adapted to the light tokens, not copied.

## 5. Repository layout

```
syntrac/
  crates/  syntrac-core/      domain types, interfaces, brain (pure)
           syntrac-store/     SQLite + in-memory repositories
           syntrac-collector/ ETW sources and enrichers
           syntrac-service/   Windows service host
           syntrac-testkit/   mocks, fixtures, contract tests
  app/     Tauri 2 + React/TS
  config/  rules, thresholds, prompts, llm endpoint
  docs/    PLAN, ARCHITECTURE, SCHEMA, THREAT-MODEL
  aiChangeLog/   scripts/dev/verify.ps1   scripts/release/   scripts/deploy/
```

File size limits: 600 lines soft, 900 hard, enforced by the verify script.

## 6. Phases

Each phase ends with an `aiChangeLog/phase-XX.md` entry, updated docs and README, and a green `verify.ps1`. Tests are written before code where applicable.

### Phase 0: Foundations (no features)
- [x] Git, folder convention, README, change log
- [x] Cargo workspace with five crates (dependency versions from crates.io via `cargo add`; licences checked. `ferrisetw` 1.2.0 was last released 2024-06-27, so it is kept behind the `TelemetrySource` trait with raw `windows-rs` as the fallback)
- [x] First `cargo build` as the MSVC toolchain check
- [x] Tests first: envelope serde round-trip; `process_instance_id` including PID reuse; repository contract suite against in-memory and SQLite
- [x] Core types and traits that make those tests pass
- [x] Tauri 2 + React/TS shell with theme tokens and an automated contrast test (shell only, no dashboard features)
- [x] `scripts/dev/verify.ps1` and file-size checker (Windows PowerShell 5.1 compatible)
- [x] `ARCHITECTURE.md`, `SCHEMA.md`, `THREAT-MODEL.md`
- [ ] ETW spike: per-provider event counts and CPU overhead. **Requires an elevated terminal; the user runs it.**

### Phase 1: Visibility
Service install and run, ETW process and network sources, metrics sampler, startup process snapshot, parent/child tracking, SQLite schema and migrations, retention job, named-pipe protocol. UI: dashboard, process list, connections list.
Done when the UI can close and reopen with no data loss, and a connection links to the correct process even after PID reuse.

### Phase 2: Intelligence
SHA-256 (cached by path, size, mtime), Authenticode/publisher via `WinVerifyTrust`, DNS-to-IP linking, first/last seen, Process Tree Explorer, Application DNA, connection map, day timeline with before/after context, global search, config change monitors (services, scheduled tasks, startup, Run keys).

### Phase 3: Detection
YAML rules engine with starter rules (exe in temp path, unsigned new exe, unusual parent launching PowerShell, new scheduled task, periodic beacon), baseline learning, anomaly and risk scoring, behavioral score, alert lifecycle with toast notifications, offline threat-intel list import.

### Phase 4: AI investigation (local only)
`LLMService` adapter for llama-server, prompts in config, natural-language questions answered through a fixed set of **read-only query tools** (not raw SQL), event explanations from correlated context. The adapter refuses non-loopback endpoints by default.

### Later
Sysmon adapter, DuckDB analytics, installer and signing, file/registry ETW.

## 7. Risks

| Risk | Mitigation |
|---|---|
| ETW volume and CPU overhead | Provider filtering, batched writes, measured budget (target under 2% CPU idle), checked in the Phase 0 spike |
| Network ETW gaps (no DNS names, pre-existing connections) | DNS-Client events plus IP Helper reconciliation |
| Service crash or data loss | Batched transactional writes, WAL, watchdog recovery |
| Sensitive command lines in storage | Redaction rules, retention limits, restricted ACLs |
| False positives eroding trust | Tiered severities, explanation on every alert, allowlist |
| Small local models give weak answers | Constrain to fixed query tools and structured context; keep deterministic detection independent of the LLM |

## 8. Open questions

- **Phase 4:** llama-server host, port and model. Not found on PATH and not listening during the Phase 0 probe, so these must come from the user before Phase 4.
- Retention defaults (30 days raw) to be revisited after measuring real event volume in the Phase 0/1 spike.
