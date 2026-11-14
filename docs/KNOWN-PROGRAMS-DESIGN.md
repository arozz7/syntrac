# I3 design addendum: known programs and the pipe's first write commands

Status: written 2026-11-13 under the approved `docs/IDENTITY-PLAN.md` (I3, decision 2: stored in the database, written through the
service pipe). The user said "I3 next" after being told this note would be written first.

## What the user can do

Open a program (its drill-down) and choose "Mark as known", with a label (prefilled from the product name) and an optional note. The
program's PRODUCT (signer or company, plus install folder, exactly `product_key`) is then known: every program of that product is
shown as "Known: Acme Backup". A program that claims to be a known product but is not (it has the product name or a member's
file name, yet a different signer or folder) is shown as "Looks like Acme Backup, but ...". A "Known programs" tab lists the
products and lets the user remove one.

## Decisions made in this design (for the user to overrule)

1. **The request carries the PATH, not the key.** `known_product_add { path, label, note }`. The service derives the product key from
   the identity it measured itself (the stored record), so a client can never invent "signed by Acme". Refused: a program
   whose file has not been read yet, one whose file could not be read (nothing is known), and one whose signature is **altered**
   (changed since signed). A program that is untrusted or unsigned may be marked; the user is the judge, and the screen says what
   "known" means for it (for an unsigned product it covers the unsigned files of that folder).
2. **One row per product**, unique on (publisher, install root). Adding the same product again updates its label and note.
3. **Lookalike rules** (pure, tested), for a program that is NOT of a known product:
   - **same product name**: its version information names the known product's product name, unless it is validly signed by the SAME
     MAKER (the first significant word of the signer: `Microsoft Windows`, `Microsoft Windows Publisher` and `Microsoft Corporation`
     are all `microsoft`) or validly signed inside the product's own folder. (Found by rehearsal on real data, see
     `aiChangeLog/phase-I3.md`: the first version flagged 31 genuine Windows files.)
   - **same file name**: it has the file name of a program of the known product (`acmeserv.exe`) and its signature is NOT valid
     (unsigned, untrusted or altered). A valid signature by someone else on a generic name (`setup.exe`) is not flagged: it would
     fire all day. A file that has not been read yet is not flagged either.
4. **Only the two write commands** go through the pipe: `known_product_add` and `known_product_remove { id }`. Listing is a plain
   read of the table by the app (read-only, like every other view). The pipe's protections stay as they are (access list, 4 KiB
   cap, per-connection deadline, first-instance flag, local clients only); the new commands are validated (label 1 to 80
   characters, note up to 500, no control characters, path up to 1,024 characters) and every change is logged with its time.
5. **Schema v7**, additive: `known_products(id, label, publisher, root, product_name, note, created_at, updated_at)`, unique on
   (publisher, root); publisher is '' for a product with no publisher. Retention and the purge never touch it.
6. **Residual risk, accepted for a personal tool (as already recorded in the threat model):** a program running as the installing
   user can send these commands and mark itself known. It cannot forge a signer, and an altered file is refused.

## Tests first

- Core (pure): validation of label and note; building a known product from an identity record (refusals above); `standing`: known,
  lookalike by product name, lookalike by file name, the two exceptions, unlisted, not-yet-read.
- Contract for `KnownProductRepository` on both stores (add, update on repeat, list, remove, survives purge) and migration v7.
- Protocol: both commands encode and decode; unknown fields ignored; oversize refused.
- Service handler (fakes): every refusal above, a ping still answered, removal of an unknown id is an error not a panic.
- App: rows and detail carry the standing; marking through a real pipe against a fake service; frontend states and the known tab.

## Not in I3

Pre-seeding Acme (the user marks it from the screen), baselines and alerts (Phase 3), hash-pinning of unsigned products.
