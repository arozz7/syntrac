# SYNTRAC Threat Model (initial)

SYNTRAC is a defensive, local observability tool for one workstation. It will hold sensitive data (process command
lines, network destinations, activity history) and runs a privileged component, so its own attack surface matters.

## Assets

- The telemetry database (activity history, command lines, destinations).
- The elevated collector service (a privilege boundary).
- Detection rules and allowlists (tampering could blind the tool).

## Trust boundaries

1. **Service (elevated) vs. UI (unelevated).** The service never trusts the UI: all pipe commands are validated,
   read-only by default, and the pipe ACL admits only the current user.
2. **Local machine vs. network.** No telemetry leaves the machine. No cloud AI. The LLM adapter refuses non-loopback
   endpoints by default.
3. **Collected data vs. the LLM.** Telemetry fields (command lines, hostnames) are attacker-influenced text. The LLM
   receives them as quoted data only and can act only through a fixed set of read-only query tools.

## Threats and mitigations

| Threat | Mitigation |
|---|---|
| Malware reads the database for activity history | DB under `%ProgramData%\Syntrac` with restricted ACLs; retention limits |
| Secrets appear in command lines | Redaction patterns applied before storage; per-collector switches |
| Local user abuses the elevated service via the pipe | Explicit access list (SYSTEM, Administrators, the installing user only); first-instance flag so the name cannot be squatted; remote clients rejected; 4 KiB request cap, per-connection deadline and connection cap; only read-only commands (ping, status); the user SID in the config is validated before it reaches the access list. All tested |
| Malware tampers with the DB or kills the service to hide | Service recovery policy; tamper-evidence and "collector offline" alerts (Phase 3) |
| LLM backend persists prompts: the local model server's `log_conversations = true` setting writes every prompt and response to `logs/conversations.<date>.jsonl` in plaintext (7-day retention) | Owner decision: keep it a setting and test both ways. SYNTRAC's `llm.context_detail` defaults to `summary` (redacted aggregates); `full` is a deliberate opt-in. Treat that log directory as sensitive as the SYNTRAC database |
| Prompt injection via a crafted process name or domain | Telemetry passed as data; LLM limited to read-only tools; no tool can modify config or the DB |
| Blind spots: processes the collector cannot open (protected or other-user) get no identity, and CPU used by interrupts/drivers belongs to no process | Skipped processes are counted and logged; the CPU breakdown reports "unaccounted" explicitly; the service runs elevated to shrink the gap |
| Restricted database ACL breaks read-only readers (SQLite must create the WAL `-shm` file) | Phase 1b installer sets the ACL deliberately and tests the unelevated UI reading it; until then the folder inherits normal ProgramData permissions |
| The SYSTEM service runs a user-writable binary or reads a user-writable config (privilege escalation) | Binary and config live under Program Files (administrators only); the data folder is the only place the user can write. The smoke test proves the user CANNOT write to either folder |
| Any program running as the installing user can read or modify the database (it has Modify on `data\`, which SQLite needs for the WAL `-shm` file) | Accepted for a personal tool (owner decision: the UI reads the database directly). Hardening path: route all reads through the service over the named pipe |
| A stalled service stop looks like a crash and triggers an immediate restart loop | The stop watchdog reports the service Stopped (timeout code) before ending the process; failure actions restart after 5 s then 30 s only |
| Event flood exhausts disk or CPU | Provider filtering, batching, retention, measured CPU budget (ETW spike) |
| Supply-chain compromise of dependencies | Lockfiles committed (`Cargo.lock`, `pnpm-lock.yaml`), `--frozen-lockfile`, minimal dependency set |
| False positives erode trust | Tiered severities, explanation per alert, allowlists |
| The SYSTEM service reads files at paths an ordinary user can influence (identity worker, `docs/IDENTITY-PLAN.md`) | Only absolute local-drive paths are opened (bare names, device and network paths are refused before the disk is touched); regular files only, a link or other reparse point is refused, never followed; opened read-only with full sharing so a running program or its updater is never blocked; hash and signature from the same handle; a hash size cap and a stop flag checked between 1 MB chunks; runs on its own thread so it cannot delay collection. Tested on real files. Residual: the version resource is parsed by the operating system's loader inside the service (a malformed file could exploit that loader); hardening path is a low-privilege helper process |
| Signer, company and description text come from the file and can be crafted (control characters, direction overrides, very long text) | One sanitiser for every string read from a file: control and direction-override characters removed, trimmed, capped at 256 characters; stored and shown as text only; given to the LLM (Phase 4) as quoted data only |
| Signature checks reach out to certificate authorities (revocation lists, intermediate certificates), leaking which programs run | Revocation checking is off and URL retrieval is cache-only, so nothing leaves the machine (owner decision 3). Cost: a certificate revoked since signing is not noticed, and a chain that needs a missing intermediate is reported as untrusted rather than fetched |

## Out of scope (for now)

Defending against an attacker with kernel-level control or SYSTEM privileges on the host, remote/multi-machine
deployment, and tamper resistance against a determined local administrator.

## Privacy principles

Local storage by default, explicit collection controls, no hidden monitoring, no data transmission.
