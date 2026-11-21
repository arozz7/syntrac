# Plan: documentation, screenshots and getting ready to make the repository public

Status: **approved 2026-11-21** (decisions below). Nothing here changes how the product behaves.

## Why
The user is considering making the repository public. A visitor needs to see what SYNTRAC is (README with screenshots), how to use it (a user guide), and under which licence. Nothing on the screenshots, in the files, or in the history may reveal anything personal.

## Decisions (2026-11-21)
1. **Screenshots come from synthetic data**: a generator creates a demo database from scratch (fictional programs, makers, folders, addresses and names; never a copy of anyone's data) and the REAL app is run on it.
2. **Licence: MIT.**
3. **Git identity: rewrite the history so every commit uses the GitHub no-reply address** (done last; all commit ids change; a backup bundle first; the exact address and the force-push are confirmed with the user before anything is pushed).
4. **Setup details are replaced by generic examples** in the tracked files, `aiChangeLog` included, and the same replacements are applied to the history when it is rewritten: this machine's drive letters and folders, and the names of software installed on it that appear as examples or test data. The pattern pack's list of well-known remote-access, tunnelling and copy tools stays: it is a public detection list, not a statement about this machine.
5. After this, the remaining backlog is worked (Phase 4 first: the app launches the local model server; whether the owner's existing local model server can be reused is looked at when that is planned).

## Slices (each ends with `scripts/dev/verify.ps1` green and a commit; the history rewrite is last)
- **P1 Demo data.** `crates/syntrac-store/examples/make_demo_db.rs` writes a deterministic demo database through the real repositories and the real rollup code: about three weeks of hourly history for fictional programs (a browser, an editor, a sync tool, a backup tool, a game, a local AI tool that uses the GPU, one unsigned miner-like program in a user folder), flows to documentation-range addresses (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24) with fictional names and owners, signatures and makers, launch entries, installs, GPU use, findings of every tier and a few expectations. A tiny stand-in for the collector's pipe (`demo_collector` example) answers the status request, so the app shows "running" instead of "offline". `scripts/dev/run-demo.ps1` generates the database, starts the stand-in and the app against them, with its own per-user folder so nothing real is read or written. Tests: the generator is deterministic, contains no path or name outside the fictional set, and every screen's data loads from it.
- **P2 Screenshots.** `scripts/dev/capture-screenshots.ps1` drives the demo app through each screen (a fixed window size) and captures ONLY the app's window into `docs/images/`. Every image is looked at before it is committed.
- **P3 Documents.** `LICENSE` (MIT), `docs/USER-GUIDE.md`, a rewritten `README.md` (what and why, screenshot gallery, quick start, privacy and safety statements, status, documents index), `THIRD-PARTY.md` (MITRE labels, iptoasn data, dependency licences).
- **P4 Scrub the tip.** Replace the setup details of decision 4 with fictional ones, with the tests still green (a reviewed mapping, applied file by file; no blind search and replace on code).
- **P5 History.** Back the repository up to a bundle outside it; rewrite the history (author and committer to the no-reply address; the same text replacements as P4 in every commit); check that nothing personal is left in any commit; check the tip is identical to the scrubbed tip; ask for the final yes; force-push `main`; then the repository can be made public.

## Out of scope
A website, an installer, code signing, translations.
