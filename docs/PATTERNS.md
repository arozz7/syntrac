# Known patterns of rogue software (F5)

SYNTRAC ships with a small, fixed set of rules ("the pattern pack", `config/patterns/rogue-software.toml`) that recognise behaviour often seen in rogue or
unwanted software. They are matched against what SYNTRAC already records. Nothing is downloaded, nothing leaves the machine, and the pack
is compiled into the program.

A match is a reason to look, never a verdict. The tier is `unusual` or `suspicious` (never `confirmed`: a pattern cannot be that sure), the
finding says in plain words why it was raised and what the likely harmless explanation is, and the usual buttons apply: expected this time,
always expected, not expected, snooze.

## What a pattern is

A pattern is data: an id, a title, a technique id, a tier, a "why", a "benign" text and conditions from a CLOSED list. The list is
(all conditions of a pattern must hold):

| Condition | Meaning |
|---|---|
| `name_is` | the file name equals one of these |
| `name_near` | the file name (without `.exe`) is exactly ONE edit from one of these and equal to none of them. A swap of two letters is one edit (`scvhost`). Names under five letters are not judged |
| `location_is`, `location_not` | where the program lives: system, program_files, program_data, user_profile, temporary, network, other. A path that says nothing about location (a bare name) never matches either |
| `signature_is` | valid, untrusted, altered, unsigned. Needs the file to have been read |
| `signer_has`, `signer_lacks` | the signer text contains / contains none of these words. Needs the file to have been read |
| `downloaded` | the file carries the Windows "downloaded from the Internet" mark |
| `already_here` | first seen BEFORE detection was switched on. What came after belongs to the rules for new things (F2), so nothing is raised twice |
| `double_extension` | a document or picture name that really ends in a program extension (`invoice.pdf.exe`). `python3.11.exe` and `setup.x64.exe` do not match |
| `direction_override` | the name contains a text-direction control character (U+202A to U+202E, U+2066 to U+2069) that can disguise an extension. The finding names the character; it never repeats the disguised name |
| `entry_kind_is` | (start entries only) service_auto, task, run_key, run_once, startup_folder |

`raise_if_concerning = true` lifts the tier to `suspicious` when the program is unsigned or untrusted in a place users can write.

There are no regular expressions over user data and no code in a pack, so a pattern cannot be made slow or hostile. A word outside the list
stops the pack loading; a test parses the shipped pack, so such a mistake fails the build and never reaches a user.

## How matching works

- Only programs seen running in the **last 30 days** are judged (`RECENT_DAYS`). A program that ran once last year and was uninstalled is not a finding today.
- A program a **known product** covers is never matched.
- A program whose file has **not been read yet** is not judged by a pattern that needs the file; the next run does.
- One kind of finding per pattern (`pattern_<id>`), so "always expected" silences exactly one pattern for exactly one program.
- The subject is the program (for a start entry: the program and the entry). The **pack version is not part of the key**: updating the pack does not raise everything again. The version is shown in each finding's evidence.
- At most **50** findings of one pattern per run (the most serious stay); the global cap of the detect worker still applies.
- "Always expected" offers this item only (no "this folder" for patterns in F5a).

## The shipped patterns (pack 2026.11.1)

| Id | Technique | Tier | What it says |
|---|---|---|---|
| `system_name_outside_windows` | T1036.005 | suspicious | A core Windows program name running from outside the Windows folder, not signed by Microsoft |
| `system_name_lookalike` | T1036.005 | suspicious | A name one edit from a core Windows program (svch0st, scvhost), not signed by Microsoft |
| `unsigned_download_in_user_folder` | T1204.002 | suspicious | Unsigned, marked as downloaded, runs from a user-writable folder, and was here before the switch-on |
| `start_entry_unsigned_in_writable_folder` | T1547.001 | suspicious | A start entry whose program is unsigned/untrusted/altered and lives in a user-writable folder; the entry was here before the switch-on |
| `double_extension` | T1036.007 | suspicious | `invoice.pdf.exe` |
| `direction_override_name` | T1036.002 | suspicious | A direction-control character in the name |
| `remote_access_tool` | T1219 | unusual (suspicious if unsigned/untrusted in a writable place) | AnyDesk, TeamViewer, RustDesk, ScreenConnect, MeshAgent, VNC, Radmin, Supremo, Atera |
| `tunnel_tool` | T1572 | unusual (same lift) | ngrok, cloudflared, frp, chisel, plink |
| `bulk_copy_tool` | T1567.002 | unusual (same lift) | rclone |

Readings of the plan that were narrowed on purpose (the backtest on this machine's copy is in `aiChangeLog/phase-F5a.md`):
- "A start entry whose target is unsigned **or** user-writable" is built as unsigned **and** in a user-writable folder. The "or" would match every validly signed app that starts from AppData (OneDrive, Teams, Discord): the backtest found 9 of those against 3 unsigned targets anywhere.
- "An unsigned program in a user folder" requires the download mark. Without it, build outputs and developer tools match (31 unsigned programs in user folders on this machine in 30 days). A plain unsigned program in a user folder is already handled by the new-program rule (for new programs) and by the review screen (for what was here at the switch-on).
- Technique T1036.004 (task or service masquerading) was dropped: the lookalike pattern is T1036.005.

Not in F5a (planned): processes starting from a new parent, script helpers using the network for the first time (F5b, needs raw processes, 30 days); a user-imported hash list, the only route to `confirmed` (F5c); clock-like beaconing (F5c, only after a backtest shows it is usable); the pack version on the Findings tab header (it is in each finding's evidence).

## Switching it on

`patterns_enabled = false` in the service configuration until the backtest has been read:

```
cargo run --release -p syntrac-store --example rehearse_detect -- <database> <work folder> 1
```

The last section of its output lists, for a COPY of your database, how many programs each pattern matches today (seen in 30 days, and all
time for comparison) with the first matches by path. It names programs on your machine: keep it local. `detect_enabled = true` is needed too.

## Where the patterns come from

Each rule is written from scratch against SYNTRAC's own data. Technique ids are public names from the MITRE ATT&CK knowledge base, used as labels
only. SigmaHQ rules (Detection Rule License 1.1) and the LOLBAS list (GPL-3.0) were read as references for what is worth looking for; none of
their files, rule text or lists is copied.

MITRE ATT&CK is used under MITRE's terms of use (https://attack.mitre.org/resources/legal-and-branding/terms-of-use/), which grant a
non-exclusive, royalty-free licence to use ATT&CK for research, development and commercial purposes, on condition that MITRE's copyright
designation and the licence are reproduced:

> © 2026 The MITRE Corporation. This work is reproduced and distributed with the permission of The MITRE Corporation.

The full licence text is at the address above and applies to every use of the technique ids here. SYNTRAC is not affiliated with, sponsored by,
or endorsed by MITRE, and the inclusion of an ATT&CK technique id does not imply endorsement or support from MITRE. ATT&CK is provided "as
is" and does not enumerate every possibility, so absence of a match proves nothing.

## Honest limits

Names, folders and signers are weak evidence on their own: anyone can name a file anything, or sign with a stolen certificate, and a patient
program that avoids every pattern will not match. The value is catching the careless majority and giving a short, explained list.
Remote-access, tunnel and bulk-copy tools are used by administrators as much as by attackers: expect each tool you really use to raise once,
and mark it expected.
