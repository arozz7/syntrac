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
| Local user abuses the elevated service via the pipe | Explicit access list (SYSTEM, Administrators, the installing user only); first-instance flag so the name cannot be squatted; remote clients rejected; 4 KiB request cap, per-connection deadline and connection cap; read-only commands (ping, status) plus, since I3, two WRITE commands that only edit the user's known-programs list (`known_product_add`, `known_product_remove`): the request names a program by path and the service builds the product from the identity IT measured, so a client cannot claim a signer; labels, notes and paths are length-limited and refuse control characters; a program that is unread, unreadable or altered-since-signed is refused; every change is logged. A program running as the installing user can still mark itself known (accepted, see the database row below); it cannot forge a signer; the user SID in the config is validated before it reaches the access list. All tested |
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
| The SYSTEM service reads files at paths an ordinary user can influence (identity worker, `docs/IDENTITY-PLAN.md`) | Only absolute local-drive paths are opened (bare names, device and network paths are refused before the disk is touched); regular files only, a link or other reparse point is refused, never followed; opened read-only with full sharing so a running program or its updater is never blocked; hash and signature from the same handle; a hash size cap and a stop flag checked between 1 MB chunks; runs on its own thread so it cannot delay collection. Tested on real files. Residual: (1) the version resource is parsed by the operating system's loader inside the service (a malformed file could exploit that loader); hardening path is a low-privilege helper process; (2) the signature checks (WinVerifyTrust, the catalog hash) read the whole file and can be neither interrupted nor limited by the size cap, which only bounds the SHA-256 loop; the per-file time limit in the plan is deferred (a stop request starts no new file, but one already in a signature check finishes). If a very large executable ever matters, the fix is to skip the signature check above the cap and record it as not checked |
| Signer, company and description text come from the file and can be crafted (control characters, direction overrides, very long text) | One sanitiser for every string read from a file: control and direction-override characters removed, trimmed, capped at 256 characters; stored and shown as text only; given to the LLM (Phase 4) as quoted data only |
| Signature checks reach out to certificate authorities (revocation lists, intermediate certificates), leaking which programs run | Revocation checking is off and URL retrieval is cache-only, so nothing leaves the machine (owner decision 3). Cost: a certificate revoked since signing is not noticed, and a chain that needs a missing intermediate is reported as untrusted rather than fetched |
| The SYSTEM service opens other processes to read their account (context worker, `docs/RUNAS-PLAN.md`) | Query only: each process is opened with `PROCESS_QUERY_LIMITED_INFORMATION` and its token with `TOKEN_QUERY`; nothing is written to or injected into a process; a process that cannot be opened is counted and skipped; account and service names go through the same sanitiser as strings from files (control and direction characters removed, length capped); at most 2,000 rows are written per pass; the context worker can be switched off with `context_enabled = false` |
| The SYSTEM service downloads the owner dataset (the first and only network use, `docs/OWNER-PLAN.md`) | Only on a request the user confirms in the app; never by itself and never on a schedule. The address is compiled in (`https://iptoasn.com/data/ip2asn-combined.tsv.gz`): a pipe request carries no address, so a client cannot redirect it. WinHTTP with the system certificate store; redirects are refused, not followed; the download is capped at 64 MB and the unpacked text at 256 MB (a zip bomb is stopped); every wait has a timeout. What is downloaded is only read by a pure text parser: ranges that overlap, run backwards or cross address families are dropped and counted, text goes through the sanitiser (control and direction characters removed, 120 characters), and a file with too few ranges or too many unreadable lines is rejected and the old dataset kept. The replacement is atomic (staging table, then one swap). No address of the user's is ever sent: lookups are local. `owner_data_allowed = false` makes the service refuse even a confirmed request |
| The SYSTEM service reads the Task Scheduler folder, the Run keys (also under other users' loaded hives in `HKEY_USERS`), the Startup folders and every service's configuration (launch worker, `docs/RUNAS-PLAN.md`, I4b) | Read only: nothing is created, written, started or stopped; no task is run. A task file larger than 1 MB, deeper nesting than 64 elements or more than 20,000 elements is refused (`mini_xml.rs`); at most 5,000 entries per sweep; all text goes through the sanitiser; a shortcut is only resolved by the shell's own reader and never opened or run. A folder or key that cannot be read is a failed source, never "empty", so nothing is wrongly shown as removed. `launch_enabled = false` switches the worker off |
| The service reads the Uninstall registry keys of the machine and of other users' loaded hives (install records, I4c) | Read only, as for the Run keys. Names, publishers, versions and folders are text anyone who can write the registry can set, so they are shown only as a clue ("Registered by an installer as"), never as a verdict, never change a program's standing and never merge two products; the text goes through the sanitiser and is capped; a folder too general to mean anything (a drive root, Program Files, a profile, Temp, anything under Windows) is never used to match a program; the uninstall command is text and is never run. A key that cannot be read is a failed read, so nothing is marked as removed |
| A client of the pipe silences a rule for a program it picks, or floods the findings (findings commands, F1) | A decision names only a finding's id: the service loads the finding it stored and builds any expectation from that finding's own kind and subject, so a client cannot name a subject. The decision name, the snooze length (1 to 365 days) and the note (cleaned, 200 characters) are checked; a refused decision changes nothing; every change is logged. The pipe is already limited to the current user. New findings per run are capped (`detect_max_new_per_run`), the overflow is counted, and a finding found again is a hit, not a new row |
| A program grows slowly so that its usage becomes the baseline (F3) | The baseline is built from the program's own past hours leaving out hours that raised a finding, and a limit moves only when the user says so. A rogue program that behaves normally for a week still becomes part of the baseline: that is why the new-entry and changed-file rules matter more |

## Out of scope (for now)

Defending against an attacker with kernel-level control or SYSTEM privileges on the host, remote/multi-machine
deployment, and tamper resistance against a determined local administrator.

## Privacy principles

Local storage by default, explicit collection controls, no hidden monitoring, no data transmission.
