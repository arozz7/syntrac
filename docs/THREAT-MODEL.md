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
| Local user abuses the elevated service via the pipe | Pipe ACL; minimal command set; input validation; no arbitrary file or shell operations |
| Malware tampers with the DB or kills the service to hide | Service recovery policy; tamper-evidence and "collector offline" alerts (Phase 3) |
| Prompt injection via a crafted process name or domain | Telemetry passed as data; LLM limited to read-only tools; no tool can modify config or the DB |
| Event flood exhausts disk or CPU | Provider filtering, batching, retention, measured CPU budget (ETW spike) |
| Supply-chain compromise of dependencies | Lockfiles committed (`Cargo.lock`, `pnpm-lock.yaml`), `--frozen-lockfile`, minimal dependency set |
| False positives erode trust | Tiered severities, explanation per alert, allowlists |

## Out of scope (for now)

Defending against an attacker with kernel-level control or SYSTEM privileges on the host, remote/multi-machine
deployment, and tamper resistance against a determined local administrator.

## Privacy principles

Local storage by default, explicit collection controls, no hidden monitoring, no data transmission.
