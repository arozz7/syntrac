# Third-party material

SYNTRAC is released under the [MIT licence](LICENSE). It builds on the work below. Nothing here is bundled in this repository except
the short public labels and ids named in the first two rows; the libraries are fetched by `cargo` and `pnpm` from their own registries
under their own licences.

| What | Where it appears | Licence or terms |
|---|---|---|
| **MITRE ATT&CK** technique names and ids (for example `T1496.001`, "Compute Hijacking") | `config/patterns/rogue-software.toml` and the findings, as plain labels on each known pattern | ATT&CK is a product of The MITRE Corporation. Only the public names and ids are used, as labels: no rule text, data file or description of MITRE's is copied. The terms of use are on <https://attack.mitre.org/resources/legal-and-branding/terms-of-use/>. SYNTRAC is not endorsed by or affiliated with MITRE |
| **IPtoASN** data (which network owns an address) | Downloaded only when you press "Download owner data" in the app, from <https://iptoasn.com>; stored in your own database | Published by the site as public-domain (PDDL) data. You can switch the download off for good with `owner_data_allowed = false` |
| **Rust crates** (Tauri, rusqlite and SQLite, serde, windows, tracing, and others) | Compiled into the programs | Each crate's own licence (mostly MIT or Apache-2.0); see `Cargo.lock` for the exact list and `cargo tree` for how they are used |
| **JavaScript packages** (React, Apache ECharts, the Tauri API, and others) | The app's screens | Each package's own licence; see `app/pnpm-lock.yaml` |
| **WebView2** | The app's window on Windows | A Microsoft component that ships with Windows 11 |

The demo data in `crates/syntrac-demo` is invented. Its companies (Contoso, Fabrikam, Northwind, Acme, Tailspin) are the sample companies
used in documentation, and its addresses are the ranges reserved for documentation (RFC 5737). They do not describe any real machine.
