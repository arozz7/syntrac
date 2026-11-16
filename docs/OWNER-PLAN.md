# IP owner (T3c): implementation plan

Status: **APPROVED and BUILT 2026-11-15** (`aiChangeLog/phase-T3c.md` lists the deviations from this plan: no per-address owner
table, no size preview, overlapping rows skipped rather than rejecting the file, no "Top owners" view). The source and licence
were approved ("T3c approved") and the design was approved with the plan order. Nothing is downloaded until you press the button
described under "Opt-in".

## Goal

Next to each destination address, say **which network owns the range it sits in**, for example "AS13335 Cloudflare, Inc., registered
in US". A DNS name is only what a program asked for (T3a); the owner is a second, independent clue. Together they let you see at a
glance that a program talking to "unknown.example" is in fact talking to a big cloud network, or to a hosting provider you have never
heard of. Evidence, never a verdict, and worded that way on screen: "owner of the network range, not necessarily the service".

## The source, as the site states it (read first-hand 2026-11-15)

[IPtoASN](https://iptoasn.com) says on its own page: "Licensed under Public Domain (PDDL v1.0)" (link to opendatacommons.org). Free
downloads, "Updated hourly". The file we want is `https://iptoasn.com/data/ip2asn-combined.tsv.gz` (IPv4 and IPv6 in one file),
columns "range_start range_end AS_number country_code AS_description". The page states no rate limit for downloads, no upstream data
source, no accuracy statement and no header-row rule, so:
- the parser accepts a header row or none, comment lines, and counts (never trusts) malformed lines;
- we download rarely (monthly at most, and only when asked), which is well inside any reasonable use;
- the full text of the PDDL itself was not read here (the link is to another site); it is the standard public-domain dedication.
The country is the one **the network is registered in**, not where the machine is. It is labelled that way.

## Opt-in (nothing leaves the machine unless you choose)

Until now SYNTRAC has made no network request of any kind. This adds exactly one, and only on a button press:
- **Off by default.** The Network tab shows "Owner data: not downloaded" with a button **Download owner data**.
- The button opens a confirmation that says exactly what happens: "SYNTRAC will fetch one file from iptoasn.com (about N MB, public
  domain). It sends no address of yours; the lookups happen on this machine." The size N is read from the response headers and shown
  before the transfer is accepted.
- The service does the fetch (it owns the database), on a request over the existing pipe; the setting "owner data enabled" and the
  download date live in the database, not in a file you must edit.
- Fetch rules (in `docs/THREAT-MODEL.md`): one fixed HTTPS URL compiled in, redirects to another host refused, response size capped,
  decompression size capped (no zip bomb), file parsed and validated in memory before anything is replaced, the old data kept if
  anything fails, a stable log line either way. The HTTP call uses Windows' own WinHTTP (system certificate store and proxy
  settings, no new crate), behind a port with a fake for tests.
- **Refresh:** a button "Refresh" (and a gentle reminder when the data is older than 30 days). Never automatic.

## Reverse DNS: a separate, second step (not in this slice)

A reverse (PTR) lookup sends each address to your resolver and so leaks what you talk to. It stays **off, opt-in, and decided
separately after you have seen how useful the owner data is**. Not built by this plan.

## Design

**Data (schema: next free version, additive).**
- `asn_ranges (start BLOB, end BLOB, asn INTEGER, country TEXT, org TEXT, PRIMARY KEY (start)) WITHOUT ROWID`: 16-byte big-endian
  keys (an IPv4 range lives in the IPv4-mapped IPv6 space, the same canonical form the flows use). A lookup is one query: the range
  with the greatest `start` not above the address, then the check `address <= end`. Rows with AS 0 ("not routed") are kept as "no
  owner" so they are not looked up again.
- `owner_dataset (id = 1, source, url, downloaded_at, rows, bad_lines, sha256)`: one row; what is on screen as "downloaded
  <date> from <source>".
- `address_owners (addr PRIMARY KEY, asn, country, org, resolved_at)`: the owner **recorded when an address is first seen** (your
  earlier direction) and refreshed when the dataset is refreshed, so a destination keeps an answer after the range data changes.
  Permanent, like names; purge of detail never touches it.
- A refresh builds the new ranges in a staging table and swaps them in one transaction, so the Network tab never sees a half
  dataset.

**Core (pure).** `owner_dataset.rs`: `parse_ip2asn(lines)` (tolerant, counts bad lines, rejects overlapping ranges), `lookup_owner`
(boundaries, gaps, v4 mapped into v6), `OwnerRepository` (replace dataset, resolve missing addresses, owner of an address, owners of
many), `DatasetFetcher` port (real WinHTTP adapter, fake for tests), `refresh_owner_data(fetcher, repo, now)` with its limits.

**Collector/service.** WinHTTP adapter in `syntrac-collector` (`owner_fetch.rs`); a pipe request pair `owner_data_status` and
`refresh_owner_data` in `syntrac-ipc` and the service; after a successful refresh, the missing addresses are resolved in bounded
batches on the existing rollup thread (addresses first seen later are resolved as the rollup records them, if data exists).

**App.** `DestinationsView` rows and the destination detail get `owner` (ASN, organisation, registered country, or null), and a
filter "Owner" plus an optional **Top owners** view (bytes per owner, then the destinations inside it, then the programs: the
natural "who do I talk to" screen). The detail shows "Owner of this network range (dataset from <date>)". With no dataset, rows say
nothing about owners except a single quiet "Owner data: not downloaded" line.

## Tests first

- Core: parser (header or none, comments, IPv4 and IPv6, AS 0, a malformed line counted, overlapping ranges rejected, empty file,
  one huge line); lookup at the first and last address of a range, one outside, in a gap, IPv4-mapped IPv6; refresh keeps the old
  dataset when parse or fetch fails; size and decompression caps; no redirect to another host.
- Contract suite for `OwnerRepository` against the in-memory and SQLite stores; a query-plan test that a lookup is an index seek;
  migration test on a copy of the live schema; swap atomicity (a failure mid-refresh leaves the old data).
- Adapter test of the WinHTTP fetcher against a local loopback server in the test (no internet needed).
- App backend: no dataset, dataset, owner missing for some rows; frontend: the confirmation text, the button states, the owner
  line, Top owners, the honest-label wording.
- Rehearsal on a COPY of the live database: one real download (with your press of the button), parse time, rows, database growth,
  lookup time for every destination seen so far, and what the Network tab shows. Sizes and timings are measured then, not guessed
  now.

## Documentation and change log

`aiChangeLog/phase-T3c.md`, `docs/SCHEMA.md`, `docs/ARCHITECTURE.md`, `docs/THREAT-MODEL.md` (the first network use, with its
rules), `docs/NETWORK-PLAN.md` and `README.md` (what leaves the machine, and when). Ends with `scripts/dev/verify.ps1` green, the
rehearsal and a commit.

## Decisions I need from you

1. **Approve this design**, in particular: the service downloads on a button press (with the confirmation text above), the data
   lives in the main database (staging swap), and WinHTTP is used rather than a new crate.
2. **Order:** I suggest the Network screen fix first (`docs/NETWORK-PLAN.md`, slice T3d), then this slice, then I4b and I4c.
3. **Reverse DNS** stays out of this slice; say if you want it planned after you have seen owners.
