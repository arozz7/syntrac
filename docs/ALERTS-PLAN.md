# Plan: sorting, the remaining network views, and findings (alerts that learn)

Status: APPROVED 2026-11-16 (S1, N1, N2, F1 to F4 as written; toast delivery is enough; D1 as written). N3 (reverse DNS) is DOCUMENTED ONLY, not to be implemented. F5 (known patterns) was APPROVED as written on 2026-11-17: **F5a and F5b are built** (`aiChangeLog/phase-F5a.md`, `phase-F5b.md`, `docs/PATTERNS.md`); F5c (hash-list import, beaconing) was SKIPPED by the user on 2026-11-17: no hash source to import from; it can be reopened later.

Progress: **S1 built** (`aiChangeLog/phase-S1.md`), **N1 and N2 built** (`aiChangeLog/phase-N1-N2.md`). N2 needed no index (measured: 5 ms
without one on the real data, an index made no difference), so the schema numbers below shift down by one: **F1 is v13, F2's
`identity_history` is v14**. N1 opens an owner in place instead of in the pop-up. **F1 built** (`aiChangeLog/phase-F1.md`, schema v13): findings, expectations and detection state, the pipe commands, the detect worker (off by default, no detectors yet) and the Findings tab. **F2 built** (`aiChangeLog/phase-F2.md`, schema v14 as `identity_changes`): the new program, new start entry, changed file and new account rules, the backtest example, and a folder scope for "always expected" (added after the backtest showed that a developer machine creates new unsigned programs in new folders all the time). The fifth F2 rule, "new network owner for a program", moves to F4 with the digest (it needs the per-program address list and the owner data, and belongs with the new-destination digest). **F3 built** (`aiChangeLog/phase-F3.md`, schema v15 as `findings.level`): the usage rules, "raise the limit to this level", and the usage replay in the backtest. Differences from the plan: the CPU floor is half of one core of the machine (the backtest showed a flat 25% of the whole machine could never fire: the busiest program's busiest hour was 4.4%); the machine-wide disk finding is not built (per-program disk was never possible; machine-wide disk stays a later idea). **F4 built** (`aiChangeLog/phase-F4.md`): the review screen, the digest, the new-network-owner rule, the open-findings badge, "new since you last looked", and toasts (the user approved the official Tauri notification plugin on 2026-11-17). F5 is next, after the user has reviewed its pattern list.

## Goals
1. Let the user sort the tables (frontend only).
2. Build the network ideas left over from T3: "Top owners", "where does this program connect". Reverse DNS as its own opt-in slice (N3), with a recommendation against it that the user decides.
3. Tell the user when something NEW appears or something uses MORE than expected, and let the user teach SYNTRAC what is expected, without hidden learning and without ever calling an anomaly "malicious" (docs/PLAN.md: unusual, suspicious, confirmed).

## Decisions I recommend (the user can change any of them)
| # | Decision | Recommendation |
|---|---|---|
| D1 | Learning | No silent waiting period for usage: rollups are kept for ever, so a 14-day baseline exists the day detection is switched on. The "new" rules only look at things first seen AFTER the switch-on moment; everything older is "already here" and goes to a "Review what is here today" screen to confirm in batches. The backtest (below) decides whether any silent wait is still needed |
| D2 | What "expected" covers | Always scoped, never global. The click offers the narrowest sensible scope first (this program group: same signer or product, same folder), then wider ones. An expectation for one program never covers a different program that merely looks like it |
| D3 | How the baseline adapts | It does NOT adapt silently. A baseline is built from the program's own past hours, leaving out hours that raised a finding, and a limit moves only when the user says so ("raise the limit to what I just saw"). This blunts a program that grows slowly to hide |
| D4 | Delivery | A "Findings" tab with a count badge, plus a Windows toast while the app is running (the SYSTEM service cannot show toasts on your desktop). No always-on tray helper for now; when the app opens it says "N new since you last looked" |
| D5 | New destinations | A digest, not alerts (there are about 2,100 destinations and new ones appear daily). A new destination becomes a finding only when it adds a NEW OWNER for a program that had none, or is first talked to by a program that is itself new or unsigned |
| D6 | Reverse DNS | Plan it (N3), off by default, but I recommend declining it: measured on the live database, 2,017 of 2,318 destinations (87%) already have a name from the machine's own DNS answers, 59 of the 301 without one are private or multicast addresses, so reverse DNS could only add names for about 240 public addresses (10%), many of which would be generic provider names, and it sends each address to a DNS server |

## Slice S1: sorting (frontend only, no schema, no service change)
- A pure `sortRows(rows, column, direction)` (stable; empty values last in both directions; numbers numerically, text case-insensitively, dates by time) and a `SortHeader` (a real button inside the `th`, `aria-sort`, a visible arrow, keyboard works). A `useSort` hook keeps the choice for the session.
- Tables: Processes, Connections, Overview, Network history, Programs history, New destinations, Known programs, destination detail.
- Honest limit: where the server returns a "top N" list, sorting reorders only the rows shown, and the table says so ("sorted within the 25 shown"). A later option is to ask the server for a different top-N by column; not in S1.
- Tests first: `sortRows` (numbers, text, empty values, stable ties, direction toggle), `SortHeader` (click, Enter, aria-sort), one table test for each kind of column.

## Slice N1: Top owners
Read-only. Sums `rollup_traffic_hour` per destination over the chosen range, looks each address up in the owner data at display time (as the Network tab already does), and groups by owner: bytes, flows, destinations, programs. A row opens the same pop-up (the destinations of that owner). No schema change. Tests first: grouping and ranking (pure), store query plan, the "no owner data yet" state.

## Slice N2: where does this program connect
Read-only. In a program's history dialog: its destinations with owner and DNS names, first and last seen, and which of them are new. "Which destinations and which are new" comes from `exe_destinations (exe_id, addr)`, which already exists; BYTES per destination need `rollup_traffic_hour`, whose key starts with `hour_start` and whose only other index is `(addr, hour_start)`. So the bytes need a new index on `(exe_id, hour_start)` (additive, schema v13). The database is about 1.1 GB: the first step is to measure, on the copy, the index build time and the size growth, and to show both before the index is accepted; if the cost is too high, the dialog shows destinations without bytes. Tests first: contract on both stores, ranking, plan test.

## Slice N3: reverse DNS (DOCUMENTED ONLY: the user decided 2026-11-16 not to implement it)
Looks up the name of an address that has none, by asking the system resolver (a PTR query). It is the only feature that would send the user's destination addresses to another party (the DNS server in use), so: a separate setting `reverse_dns_allowed = false`, a confirmation dialog that says exactly that, its own row in `docs/THREAT-MODEL.md`, names cleaned by the shared text cleaner and cached in `address_names` with a source column (`seen_in_dns` versus `reverse_lookup`), shown labelled as "reverse lookup (not seen in your DNS traffic)". Limits: at most N lookups per minute, public addresses only, never private or multicast. Tests first: the rate limiter and the public-only filter (pure), a fake resolver. Built only if the user approves it after reading the numbers in D6.

## Slice F1: findings foundation (schema v14; v13 is the N2 index; if N2 needs no index, F1 is v13 and F2 is v14)
Ports and adapters, so the whole loop runs in memory without Windows.
- **Memory (repositories):** `FindingRepository` (`record`, `open`, `by_id`, `update_status`, `counts`), `ExpectationRepository` (`add`, `remove`, `list`, `matching(kind, subject)`), `DetectionState` (learning start, per-detector watermark).
- **Tools:** the service's detect worker thread (every few minutes, like the launch worker); a `Notifier` port (the app implements it as a toast; a fake for tests).
- **Reasoning loop (pure core):** `detect(now, facts, expectations, state) -> Vec<Candidate>` where facts come from read ports over what already exists (executables, rollups, launch records, identity). Then `reconcile(candidates, open findings)` dedupes by `dedupe_key` (kind + subject + window), applies cooldowns and a daily cap (overflow goes to the digest), and returns the writes.
- **Tables:** `findings` (id, kind, subject_key, severity tier, status, first_at, last_at, hits, evidence as capped text lines, `why` sentence, dedupe_key); `expectations` (id, kind, scope_key, params, note, created_at, source finding); `detect_state` (one row: learning start, per-kind watermarks). Additive, permanent (the purge of detail never touches them). Written only by the service.
- **Pipe requests (the app is read-only on the database):** `finding_list`, `finding_set_status` (open, expected, not expected, snoozed until), `expectation_add`, `expectation_remove`, `detection_status`. Same style as `owner_data_*`.
- **Statuses:** open -> expected (this time) | always expected (creates an expectation) | not expected (stays visible, marked as a concern) | snoozed | resolved (the condition ended). Every status change is stored with its time, so what the user told SYNTRAC is always visible and reversible.
- **UI:** a Findings tab: list sorted by newest, filter by status, each row with the plain "why", the evidence, and the buttons above; a "What you told SYNTRAC is expected" list with Remove; the learning-period banner.
- Tests first: reconcile (dedupe, cooldown, cap, reopen after resolve), expectation matching (scope never wider than chosen; a look-alike in another folder does not match), state transitions, learning period (no findings, baseline recorded), the contract suite on both stores, pipe round trips.

## Slice F2: things that are NEW (rules, no statistics)
Each rule is a pure function with a one-sentence "why" and the evidence it used.
| Rule | Fires when | Tier |
|---|---|---|
| New program | an executable first seen after the switch-on moment, no known product covers it, and it is NOT an update: a new path with the same signer in the same program group or known-product folder (self-updating apps put each version in a new folder) is recorded as an update, not raised | unusual; suspicious when unsigned, or in a user-writable folder (Temp, Downloads, AppData) |
| New start entry | a launch record (service, task, Run key, Startup file) first seen after the learning period | unusual; suspicious when its target is unsigned or user-writable |
| Changed file | the same path now has a different SHA-256 AND (signer changed, or signed became unsigned) | suspicious (same signer, new version: not a finding; recorded as an update) |
| New account | a program runs as an account it never used (for example SYSTEM) | unusual |
| New owner for a program | a program talks to a network owner it never did (needs owner data; otherwise the rule is off and says so) | unusual |
`Changed file` needs the identity facts of the previous inspection: a small `identity_history` table (path id, sha256, signer, status, seen_at; additive, schema v15) because `executable_identity` keeps only the latest row.
Tests first: one table-driven test per rule (fires, does not fire, covered by an expectation, before the switch-on moment, an update path of the same signer and group).

## Slice F3: USAGE above what is expected
Explainable statistics only (docs/PLAN.md decision 7), no machine learning.
- Per program and per hour, from `rollup_process_hour` and `rollup_traffic_hour`: CPU average, memory peak, bytes sent, bytes received. The system as a whole from `rollup_system_hour`.
- Baseline per program and metric: the 95th percentile and the maximum of its own hours in the last 14 days, leaving out hours that raised a finding; it needs at least 48 active hours before it is trusted ("not enough history" otherwise, shown, never guessed).
- A finding needs ALL of: above max(3 x the 95th percentile, 1.5 x the prior maximum), above an absolute floor (for example 100 MB sent in an hour, 25% CPU, 500 MB more memory), and sustained for 2 consecutive hours (a single hour only for bytes sent above 10 x the prior maximum, the exfiltration shape).
- The sentence says it in numbers: "Sent 2.1 GB in the hour from 14:00. In the 14 days before, its busiest hour was 310 MB and 95% of hours were under 120 MB."
- Buttons: Expected this time; Raise the limit to this level (stored as an expectation with the new ceiling); Not expected.
- All numbers are in `config/service.toml` (floors, multipliers, window, history needed).
- Covers per program: CPU, memory, bytes sent and bytes received. NOT disk per program: the stored per-program hourly rollup has no I/O columns (and "disk" here would only mean file I/O, not the physical disk anyway); disk exists only for the machine as a whole, so a machine-wide disk finding is the only disk alert possible, and per-program disk would need a new collection and rollup first.
- Known limit: no time-of-day or day-of-week pattern in v1 (a backup that runs every night is learned as "its busiest hour is high"); a seasonal baseline is a later slice if the backtest shows it is needed.

## Slice F4: review and delivery
- "Review what is here today": programs and start entries present at switch-on, grouped by product or signer, one click confirms a group as expected (creates expectations, and offers to add the product to Known programs, which is already the user's list of expected programs and suppresses the "new program" rule).
- Digest: new destinations and new owners of the last day (D5), shown on the Findings tab, not toasted.
- Toast: the app, while it runs, polls `detection_status` and raises one Windows toast per new suspicious finding (and one summary for unusual ones). Tauri notification plugin (new dependency: approval needed, which I will ask for in this slice). Never a toast for the digest.

## Slice F5: known patterns of rogue software (approved 2026-11-17; F5a and F5b BUILT, see docs/PATTERNS.md; F5c skipped by the user)
The user asked whether SYNTRAC can use known patterns of shady or rogue programs to identify them. Yes, as a fixed, reviewable set of rules that SYNTRAC ships with (no download at run time, nothing leaves the machine), each tied to a public technique name, and each limited to what SYNTRAC already records. They raise findings through the F1 machinery (same buttons, same expectations, same "why"), at most at the tier "suspicious": a pattern match is a reason to look, never a verdict.

**Where the patterns come from (checked 2026-11-16):**
- The technique catalogue of MITRE ATT&CK: free to use for research, development and commercial purposes if MITRE's copyright notice and licence are reproduced; no use of the name that implies endorsement. Each SYNTRAC rule cites its technique id (for example T1036.005, "masquerading: match legitimate name or location") as a label only, with the notice in `docs/`.
- SigmaHQ rules (licence DRL 1.1: use allowed with credit to the rule author wherever matches are shown) and the LOLBAS list (GPL-3.0, which would bind the whole project if bundled): their FILES are NOT copied. They are read as references for what is worth looking for, and each SYNTRAC rule is written from scratch against SYNTRAC's own data. Where a rule is directly inspired by one, the rule text names it.
- The rules cannot be a downloaded feed: the "no network except the one owner-data download" rule stays.

**Closed vocabulary, no scripting:** a rule is a small data entry (`config/patterns/*.toml`, embedded in the build) with an id, title, technique id, tier, a plain "why" and "benign reasons" text, and conditions chosen from a fixed list evaluated by pure code: name equals or within one edit of a list, folder under or not under a list of folders, signature state, signer in or not in a list, double extension, parent name, start-entry kind, "first network use", "first time as this account". No regular expressions over user data, no code in rules, so a rule cannot be made slow or hostile. A rule pack has a version, shown on the Findings tab.

**Starter patterns (each only if SYNTRAC records what it needs):**
| Pattern | Data used | Notes |
|---|---|---|
| A system program name running from the wrong folder (svchost, lsass, csrss, services, winlogon, explorer, taskhostw ... outside System32 or the Windows folder), or a name one letter away from one (svch0st, scvhost) | executables path, identity | T1036.005, T1036.004. Strong and quiet |
| An unsigned program (or one with no company) running from Temp, Downloads, a user's AppData, Public or the root of ProgramData | path, identity | T1204, T1036. Quiet once known products exist |
| A start entry (service, task, Run key, Startup file) whose target is unsigned, or lives in a user-writable folder | launch records, identity | T1547.001, T1053.005, T1543.003 |
| A double extension or a document name ending in an executable one (invoice.pdf.exe), or a name with direction-override characters | path | T1036.007, T1036.002 |
| Script and download helpers (powershell, wscript, cscript, mshta, regsvr32, rundll32, certutil, bitsadmin, msiexec) using the network for the FIRST time, or from a new parent | rollups; processes (parent, 30 days raw) | T1218, T1105, T1059. Only "first time", because each also has normal uses |
| An office or browser program starting a command shell or a script host | processes (parent, command line, 30 days raw) | T1566, T1059. Needs the raw rows, so it can only look back 30 days |
| Regular, clock-like connections from one program to one destination (a beacon): at least 10 connections whose gaps vary by under 10% | raw flows (30 days) | T1071. Gated: the program is new, unsigned or unknown, and the destination is new; the backtest decides if it is usable at all |
| Dual-use tools that appear: remote access (AnyDesk, TeamViewer, RustDesk, ScreenConnect ...), tunnels (ngrok, cloudflared, frp, chisel), bulk copy to the cloud (rclone) | name, signer, company | T1219, T1572, T1567. "Commonly used by attackers and by administrators"; unsigned or in a user-writable folder is higher. Named so the user can confirm in one click, like Acme |
| A hash from a list the USER imports (F5c) | SHA-256 in identity | The only route to the tier "confirmed". A file the user downloads and imports by hand (for example a public hash list), never fetched by SYNTRAC; its source and licence are the user's choice, shown with each match |

**Honest limits:** names, folders and signers are weak evidence on their own (anyone can name a file anything, or sign with a stolen certificate), and a patient program that avoids every pattern will not match. The value is catching the careless majority and giving the user a short, explained list. Every pattern states its likely harmless explanation. Patterns that need raw processes or flows see only the last 30 days.

**Slices:** F5a the pattern engine and the path/name/signature/start-entry patterns (everything from tables SYNTRAC already keeps); F5b parent-child and first-network-use patterns; F5c the hash-list import and the beacon pattern (the beacon only after the backtest). Tests first: each pattern in a table-driven test (matches; harmless look-alike does not; covered by an expectation; Known product does not match); the vocabulary parser (unknown condition is refused at build time); the name-distance function; the backtest prints how many of today's programs each pattern matches, and the list is read by the user before anything is enabled (`patterns_enabled = false` until then).

## Backtest before anything is switched on (applies to F2, F3, F4 and F5)
`crates/syntrac-store/examples/rehearse_detect.rs`: on a COPY of the real database, replays the last 30 days hour by hour through the same pure rules and prints how many findings each rule would have raised, the noisiest programs, and a sample of the sentences. Thresholds are tuned from this before the detect worker is enabled by default (`detect_enabled = false` until the user has read the backtest).

## Out of scope here
Threat-intel lists, YAML rule files, alert rules over command lines or parents (Phase 3 proper), the LLM "Investigate" button (Phase 4), a tray helper, mail or phone delivery, and any use of the network.

## Honest limits
- A rogue program that behaves normally for a week becomes part of the baseline: that is why the review screen and the "new start entry" and "changed file" rules matter more than usage statistics.
- A finding is evidence, never a verdict. "Expected" means "the user said so", and the screen shows who said it and when.
- If the service is stopped, nothing is detected; the existing "collector offline" banner stays the signal for that.

## Order and size
S1 (small) -> N1, N2 (small each) -> F1 (largest: schema, ports, pipe, tab) -> F2 -> backtest -> F3 -> F4 -> F5 (after its own approval). Each slice ends with `verify.ps1` green, a rehearsal on a copy of the live data, a change log in `aiChangeLog/`, doc updates, and a commit. Push in batches.

## Decisions taken (2026-11-16)
1. S1, N1, N2 and F1 to F4 approved as written.
2. Reverse DNS (N3): documented, not implemented.
3. D4: a toast while the app runs is enough; no tray helper.
4. D1: approved as written (no silent wait; new things count after switch-on; review screen; the backtest decides).
5. F5 (known patterns): approved as written 2026-11-17. F5a built (the pattern engine and the name, path, signature and start-entry patterns; off until `patterns_enabled = true`). The user named no tools they use, so nothing is pre-expected. F5b built 2026-11-17 (started-by and first-network-use patterns; no command lines are stored and the process rows last 14 days, so those plan lines were narrowed). F5c (hash list, beaconing) was skipped by the user on 2026-11-17 because they have no hash source; F5 is complete as built.
