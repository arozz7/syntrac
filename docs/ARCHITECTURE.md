# SYNTRAC Architecture

Three loosely coupled layers, wired together by dependency injection. See [PLAN.md](PLAN.md) for decisions and phases.

## Layers

| Layer | Responsibility | Where |
|---|---|---|
| Reasoning (brain) | Pure, synchronous logic: correlate, detect, score. No I/O, no vendor imports. | `syntrac-core` |
| Memory (context) | State and storage behind repository traits. | traits in `syntrac-core`; `syntrac-store` (SQLite), `syntrac-testkit` (in-memory) |
| Tools (hands) | Side effects behind adapter traits: ETW, file hashing, signatures, LLM. | traits in `syntrac-core`; `syntrac-collector` |

## Process model

```
syntrac-service (elevated Windows service)   -- writes -->   SQLite (WAL)
        ^                                                        |
        | named pipe (commands, live push; ACL: current user)    | read-only
        |                                                        v
syntrac-app (Tauri 2, unelevated)  <--------------------------- reads
```

The service is the only writer. The UI can close without stopping collection.

## Interfaces (defined in `syntrac-core::ports`)

| Interface | Kind | Status |
|---|---|---|
| `ProcessRepository` | Memory | Phase 0, implemented (SQLite, in-memory) |
| `EventRepository` | Memory | Phase 0, implemented (SQLite, in-memory) |
| `TelemetrySource` | Tool | Phase 0 trait + mock; ETW implementation in Phase 1 |
| `Clock` | Tool | Phase 0 trait + `ManualClock` |
| `EntityRepository` / `ProfileRepository` / `BaselineRepository` / `AlertRepository` | Memory | Planned (Phases 2 and 3) |
| `FileEnricher`, `IntelProvider` | Tool | Planned (Phases 2 and 3) |
| `LLMService` | Tool | Planned (Phase 4): adapter over the local llama-server's OpenAI-compatible API |

## Reasoning loop (Phase 0 slice)

`ingest(source, repo)`: drain the source, sort by timestamp, `correlate` each event (resolve PID to a process
instance, maintain lifetimes), persist. Proven offline by `syntrac-testkit/tests/pipeline.rs` using
`MockTelemetrySource` + `InMemoryRepository`, and again against SQLite in `syntrac-store/tests`.

Later phases add `Detector` and `RiskScorer` after the correlator, each pure and injected.

## Key invariants

1. **Process identity is `(pid, creation time)`**, never the PID alone (`ProcessInstanceId`, UUIDv5). `resolve(pid, at)`
   returns the instance that owned the PID at that moment, so a reused PID never inherits an old process's activity.
2. **Unknown stays unknown.** An event whose PID cannot be resolved is stored unattributed rather than guessed.
3. **One event envelope** (`EventEnvelope`) for all sources. Sysmon or other sources later map into the same shape.
4. **Severity tiers are distinct**: `Unusual` (baseline deviation) < `Suspicious` (rule match) < `Confirmed` (intel).
5. **Config-driven**: rules, thresholds, prompts and the llama-server endpoint live in `config/`, not in code.

## Deviations from the plan

- The in-memory repository lives in `syntrac-testkit` (not `syntrac-store`). It is test infrastructure, and placing it
  there avoids a dependency cycle between the store and the shared contract suites.
- The correlator uses the start event's timestamp as the process creation time. Phase 1 will use the real `CreateTime`
  field from the ETW Kernel-Process event once the spike confirms it is available.

## Logging

`tracing` with structured fields and stable, low-cardinality messages, e.g. `info!(id = %id, "Process started")`.
Wired up when the service is built in Phase 1.
