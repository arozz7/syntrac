# Phase I1b: executable identity, Windows adapter and service worker

Implements I1b of `docs/IDENTITY-PLAN.md` (approved 2026-11-13). The service now reads the files of the programs it has seen.

## What changed

**Collector crate (Windows adapter)**
- `file_parse.rs` (pure): HRESULT to trust outcome (`Valid`, `NoSignature`, `Altered`, `Untrusted`, `Failed`), one sanitiser for
  every string read from a file (control and direction-override characters removed, trimmed, capped at 256 characters),
  thumbprint hex, `Zone.Identifier` parsing, the path pre-check (only an absolute local-drive path is opened), version
  resource sub-block paths.
- `file_hash.rs`: streaming SHA-256 (`sha2` 0.11, approved), 1 MB chunks, a stop check before each chunk.
- `file_trust.rs`: `WinVerifyTrust` with revocation checking off and cache-only URL retrieval (nothing leaves the machine), the
  embedded signature first, then the Windows catalog found by hash (SHA-256, then SHA-1). Signer name and SHA-1 thumbprint come
  from the verification state, which is always closed (RAII guards release the catalog contexts).
- `file_version.rs`: the neutral version resource through the OS loader; every failure gives an empty result.
- `file_inspector.rs`: `WindowsFileInspector` (`stamp`, `inspect`). The file is opened once, read-only, with read, write and
  delete sharing, without following reparse points; a link, folder or other non-regular file is refused. The cheap `stamp` and the
  full read use the same function to build the stamp, so an unchanged file never looks changed. A file over the cap
  (`identity_max_hash_mb`, 512) is not hashed. A stop request ends a hash in progress.

**Service**: `identity_worker.rs` (`IdentityWorker`, generic over repository, inspector and clock: `tick` runs a pass when due and
schedules the next, a short pause while a backlog remains, the long interval otherwise, a storage error is reported and does not
end the worker; `run` loops until stopped). `host.rs` starts it on its OWN thread with its OWN database connection, so reading
files can never delay draining ETW, and stops it with the service. New config keys (validated, non-zero):
`identity_interval_minutes` 5, `identity_files_per_pass` 5, `identity_backlog_pause_secs` 30, `identity_max_hash_mb` 512
(documented in `config/service.toml`). An installed config without them gets these defaults.

## Tests (written first)
- `file_parse.rs` (11) and `file_hash.rs` (4): known answers for SHA-256, the HRESULT mapping, sanitising, zone parsing, the path
  pre-check, version paths.
- `file_inspector.rs` (14, on real files): a Windows file is valid and Microsoft-signed through its catalog; the cheap stamp equals
  the full read's stamp; an unsigned text file and the test program itself are unsigned with the right hash and size; a downloaded
  file carries its zone; a file over the cap is not hashed; a stop request ends the read; missing, folder, bare name and network
  path give errors; an exclusively locked file is access denied; a file open for writing elsewhere can still be read; reading does not
  stop a delete; an app-execution alias (a reparse point) is refused; a copy of a real embedded-signed file with one byte changed is
  reported as altered. (The symbolic-link test skips on an account without the privilege to create one.)
- `identity_worker.rs` (4): first pass at once then the interval, the backlog pause, a storage error does not end the worker, the
  loop stops promptly. Config tests (defaults, tuning, zero rejected).

## Rehearsal (a copy of the live database, unelevated, release build; the copy was migrated v5 to v6 on open)
- 725 executables: 681 read in full, 44 failed (43 `NotFound`: programs since deleted, mostly build output; 1 `NotAFile`: the bare
  name `vmmemWSL`). No access-denied results.
- Time: 32.6 s wall, 6.3 s CPU for the whole backlog (45 ms per file); slowest pass of 25 files 5.1 s. The database grew 180 KB.
  At the default pace (5 files per 30 s) the backlog takes about an hour in the background.
- Signatures: 223 valid (embedded), 80 valid (catalog), 377 unsigned, 1 untrusted. Top signers: Microsoft Windows 119, Microsoft
  Corporation 65, then hardware and software vendors with 7 to 27 files each. Nothing altered.
- Spot checks: `cmd.exe` valid, catalog, "Microsoft Windows"; Acme (`acmeserv.exe`, `acmetransmit64.exe`) valid, embedded,
  signer "ACME SOFTWARE, INC.", company "Acme", product "Acme Backup"; a web browser valid; Defender `MsMpEng.exe`
  valid; SYNTRAC's own `syntrac-service.exe` unsigned.
- Findings for the user: the one untrusted file is a text editor's executable under a second drive's `Program Files`
  (embedded signature); the offline settings (no fetching of missing intermediate certificates) may be the cause, so this is reported,
  not worked around (decision 3 was the user's). Most unsigned files are build output under a projects folder, Git's `usr\bin`
  tools, cargo tools, and Microsoft Store apps under `WindowsApps` (those are signed as a package, not per file). No file
  carried a downloaded-from-the-internet mark.
- For I2: "original file name differs from the name on disk" is NOT a safe warning by itself. It is common and harmless for .NET
  and Intel launchers (`IntelGraphicsSoftware.exe` whose original name is `IntelGraphicsSoftware.dll`), PowerToys, Git's `bash.exe`,
  and even Windows' own `audiodg.exe` (original name `audioadg.exe`). It will be shown as information, never as an alert.

## Deployment (important)
The new service migrates the database to schema v6 the first time it starts. The app opens the database read-only and refuses any
schema that is not exactly the one it was built for, so the installed app (built for v5) will show a schema error from that moment.
Reinstall the service AND rebuild the app (`cd app; pnpm tauri dev`) together. Going back to the old service later is not possible
without a backup, because it refuses a newer schema (the v6 migration itself is additive and needs no backup).

## Not in I1b
The UI (I2), known programs and the pipe's write commands (I3), run-as account (I4). The one-off backfill is not a separate step: the
worker's normal backlog pacing does it after the service starts.
