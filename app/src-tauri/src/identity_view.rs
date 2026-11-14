//! What the app says about WHO a program is, built from the identity records the service keeps (`docs/IDENTITY-PLAN.md`).
//!
//! Everything here is evidence for the screen, never a verdict: a missing record is "pending" (not yet looked at), a record with
//! no facts is "unreadable" (we do not know), and only measured facts are reported as such. Same conventions as the other views:
//! camelCase on the wire, times in milliseconds since the Unix epoch.

use std::collections::HashMap;

use serde::Serialize;
use syntrac_core::rollup::executable_key;
use syntrac_core::{classify_location, install_root, IdentityRecord, IdentityRepository, RepoError};

use crate::history::ms;

/// A short summary for a row of a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBadgeView {
    /// `pending` (never looked at), `known` (the file was read), or `unreadable` (it could not be read, nothing is known).
    pub state: String,
    /// `valid`, `untrusted`, `altered` or `unsigned`; only when the file was read.
    pub signature: Option<String>,
    /// Who made it: the signer, else the company in the version information.
    pub publisher: Option<String>,
    /// The product name in the version information.
    pub product: Option<String>,
    /// Where it runs from (`system`, `program_files`, `program_data`, `user_profile`, `temporary`, `network`, `other`, `unknown`).
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityFailureView {
    pub kind: String,
    pub message: String,
    pub attempts: u32,
}

/// Everything for the panel of one program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityDetailView {
    #[serde(flatten)]
    pub badge: IdentityBadgeView,
    /// Who signed the file (the certificate's name), when it is signed.
    pub signer: Option<String>,
    pub signature_kind: Option<String>,
    pub thumbprint: Option<String>,
    pub company: Option<String>,
    pub description: Option<String>,
    pub original_filename: Option<String>,
    pub file_version: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
    /// When the facts were measured.
    pub inspected_at_ms: Option<i64>,
    pub last_attempt_ms: Option<i64>,
    /// The file carries the "downloaded from the Internet" mark.
    pub downloaded: bool,
    pub install_root: Option<String>,
    /// The last look failed (the facts above, if any, are from an earlier look).
    pub failure: Option<IdentityFailureView>,
}

pub fn badge(path: &str, record: Option<&IdentityRecord>) -> IdentityBadgeView {
    let location = classify_location(path).as_str().to_owned();
    let facts = record.and_then(|r| r.facts.as_ref());
    let state = match (record, facts) {
        (None, _) => "pending",
        (Some(_), Some(_)) => "known",
        (Some(_), None) => "unreadable",
    };
    IdentityBadgeView {
        state: state.to_owned(),
        signature: facts.map(|f| f.signature.status.as_str().to_owned()),
        publisher: facts.and_then(|f| f.signature.signer.clone().or_else(|| f.version.company.clone())),
        product: facts.and_then(|f| f.version.product.clone()),
        location,
    }
}

pub fn detail(path: &str, record: Option<&IdentityRecord>) -> IdentityDetailView {
    let facts = record.and_then(|r| r.facts.as_ref());
    IdentityDetailView {
        badge: badge(path, record),
        signer: facts.and_then(|f| f.signature.signer.clone()),
        signature_kind: facts.and_then(|f| f.signature.kind).map(|k| k.as_str().to_owned()),
        thumbprint: facts.and_then(|f| f.signature.thumbprint.clone()),
        company: facts.and_then(|f| f.version.company.clone()),
        description: facts.and_then(|f| f.version.description.clone()),
        original_filename: facts.and_then(|f| f.version.original_filename.clone()),
        file_version: facts.and_then(|f| f.version.file_version.clone()),
        sha256: facts.and_then(|f| f.sha256.clone()),
        size_bytes: facts.map(|f| f.stamp.size),
        modified_ms: facts.and_then(|f| f.stamp.modified).map(ms),
        inspected_at_ms: record.and_then(|r| r.inspected_at).map(ms),
        last_attempt_ms: record.map(|r| ms(r.last_attempt)),
        downloaded: facts.is_some_and(|f| f.downloaded()),
        install_root: install_root(path),
        failure: record.and_then(|r| r.failure.as_ref()).map(|f| IdentityFailureView {
            kind: f.kind.as_str().to_owned(),
            message: f.message.clone(),
            attempts: f.attempts,
        }),
    }
}

/// All the identity records, by executable key: read once per request, so a ranking of 200 rows costs one query.
pub struct Identities(HashMap<String, IdentityRecord>);

impl Identities {
    pub fn load<R: IdentityRepository + ?Sized>(repo: &R) -> Result<Self, RepoError> {
        let map = repo
            .identity_entries()?
            .into_iter()
            .filter_map(|entry| entry.record.map(|record| (entry.exe.key, record)))
            .collect();
        Ok(Self(map))
    }

    pub fn of(&self, path: &str) -> Option<&IdentityRecord> {
        executable_key(path).and_then(|key| self.0.get(&key))
    }
}

fn severity(signature: &str) -> u8 {
    match signature {
        "altered" => 3,
        "untrusted" => 2,
        "unsigned" => 1,
        _ => 0,
    }
}

/// The most common value (ties: the smallest), so a product's label does not depend on the order its members arrive in.
fn most_common(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).map(|(value, _)| value)
}

/// The folder that names the product, spelled as it is on disk (the install root itself is lower case).
fn folder_name(path: &str) -> Option<String> {
    let root = install_root(path)?;
    let depth = root.trim_end_matches('\\').split('\\').count();
    let normalised = path.trim().replace('/', "\\");
    let shown = normalised.trim_start_matches(r"\\?\");
    shown.split('\\').filter(|part| !part.is_empty()).nth(depth.saturating_sub(1)).map(str::to_owned)
}

/// The name of a product row: the product name members agree on, else their publisher, else the folder they live in.
pub fn product_name(paths: &[&str], badges: &[IdentityBadgeView]) -> String {
    most_common(badges.iter().filter_map(|b| b.product.clone()))
        .or_else(|| most_common(badges.iter().filter_map(|b| b.publisher.clone())))
        .or_else(|| paths.first().and_then(|path| folder_name(path)))
        .unwrap_or_else(|| "Program group".to_owned())
}

/// One badge for a whole product: the WORST signature among the members that were read (an altered file must not hide behind
/// valid siblings), the label they agree on, and the location they share.
pub fn merged_badge(members: &[IdentityBadgeView]) -> IdentityBadgeView {
    let known = members.iter().filter(|m| m.state == "known");
    let signature = known
        .clone()
        .filter_map(|m| m.signature.clone())
        .max_by(|a, b| severity(a).cmp(&severity(b)).then_with(|| b.cmp(a)));
    let state = if known.clone().next().is_some() {
        "known"
    } else if members.iter().any(|m| m.state == "unreadable") {
        "unreadable"
    } else {
        "pending"
    };
    IdentityBadgeView {
        state: state.to_owned(),
        signature,
        publisher: most_common(members.iter().filter_map(|m| m.publisher.clone())),
        product: most_common(members.iter().filter_map(|m| m.product.clone())),
        location: members.first().map_or_else(|| "unknown".to_owned(), |m| m.location.clone()),
    }
}
