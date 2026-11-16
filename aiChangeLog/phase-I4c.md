# Phase I4c: install records (the Uninstall registry keys, schema v12)

Slice I4c of `docs/RUNAS-PLAN.md` (addendum approved 2026-11-15). Adds a third clue about where a program came from: the product an
installer REGISTERED for the folder the program lives in ("Registered by an installer as Acme from Acme Software, Inc., version
..."). A clue, never proof: anything that can write the registry can claim any name, so it never replaces the signature, never
changes a standing and never merges two products.

## What changed
- **Core (pure).** `install_records.rs`: `to_record` (an Uninstall entry to a record: text cleaned and capped; the install folder
  from `InstallLocation`, else worked out from `DisplayIcon` or the uninstall command and marked "derived"; folders that are too
  general are never used: a drive root, Program Files, ProgramData, Users, a profile, AppData, Temp, Downloads, anything under
  Windows, a bare share), `registered_for` (a program matches the record whose folder holds it: the most specific folder wins,
  equally specific ones are all returned, the same product listed in two registry views once, a product still installed before one
  that is gone, at most three), `sweep_installs`, the `InstallSource` and `InstallRepository` ports.
- **Stores.** Migration 12 (additive, no backup): `install_records` (one row per registry view and key; first and last seen, the
  other fields those of the newest sighting). When the keys were last read completely is the `launcher_sweeps` row of kind
  `install_records`. Not stored per program: one product has many executables, so a program is matched when shown. In-memory
  reference plus contract suite `install_contract.rs` run against both.
- **Windows adapter.** `install_registry.rs` (the machine's 64-bit and 32-bit Uninstall keys and each loaded user's) on the shared
  read-only `registry.rs`, which gained `dword` and now allocates its buffers once instead of about 96 KB per value.
- **Service.** The launch worker reads the Uninstall keys in the same sweep (`launch_enabled` and `launch_interval_minutes`); a
  failed read is reported in the sweep's failed list and the launch records are still kept.
- **App.** The detail view carries `registered` (name, publisher, version, install date, folder, whether the folder was only
  worked out, whether it is still installed) and the badge carries `registeredAs`. A product group with no product name and no
  signer takes the registered name before falling back to the folder name (order: product name, signer or publisher, registered
  name, folder). Frontend: "Registered by an installer as" in the identity panel, "No installer record names this folder" when
  empty, "(no longer installed)", "(folder worked out from the uninstall command, not stated)", and the clue wording.

## Tests (written first)
Core `install_records.rs` (13); testkit `install_contract` (both stores) and `install_sweep.rs` (4); store `migration_v12_tests.rs`
(2); collector `launch_sources.rs` (+2, against this machine's registry); service `launch_worker.rs` (+2); app `install_view.rs`
(5); frontend `ProgramsRegistered.test.tsx` (5).

## Rehearsal (copy of the live database, `examples/rehearse_launchers.rs`, copy at `<scratch folder>\installs-i4c`, to be deleted by the user)
743 Uninstall entries read in about 90 ms (a second read: 0 new): 697 have a usable name, 235 of those name an install folder (62 of
them worked out from the icon or the uninstall command rather than stated). Of the 116 programs that have a start entry, 36 sit in
a folder with a registered product. Many entries (installed by Windows Installer) name no folder at all, so most programs will
honestly say "No installer record names this folder". A registered folder can be broader than the product (a game store's
online-services host sits under the game launcher's folder and is shown as that launcher), which is why it is worded as a clue.

## Limits (as designed)
Only the registry's own words: a program with no installer record, an installer that writes no folder, and users who are not
signed in are not covered. The service must be reinstalled with this build (schema v12) and the app rebuilt together.
