//! The "new thing" rules (docs/ALERTS-PLAN.md, F2): what appeared after detection was switched on. All pure: the facts come in, the
//! candidates go out; the detector at the bottom only loads the facts through read ports.
//!
//! Rules (each raises a `Candidate` with one plain sentence and the facts it used):
//! - `new_program`: a program first seen running after the switch-on, that no known product covers and that is not an UPDATE of a
//!   product already here (same maker and install folder, so a self-updating app's new version folder is not news).
//! - `new_start_entry`: a start entry (service, task, Run key, Startup file) first seen after the switch-on, for a program that
//!   was already here (a brand-new program's finding already says how it starts).
//! - `changed_file`: the same path now holds different content AND its signer changed, or it was validly signed and no longer is.
//!   New content from the same signer is an update and is not raised.
//! - `new_account`: a program that was already here now runs as an account it never ran as before.
//!
//! A tier is `unusual` unless the evidence matches a pattern worth a closer look (a file changed after signing, an unsigned program
//! in a folder any user can write, a look-alike of a known program, a changed signer): then it is `suspicious`. Never a verdict.

use std::collections::HashSet;

use crate::exe_context::{AccountKind, ContextRepository, ExecutableContext};
use crate::exe_identity::{FileFacts, IdentityEntry, IdentityRepository, SignatureStatus};
use crate::exe_location::{classify_location, product_key, LocationClass};
use crate::findings::{Candidate, Tier};
use crate::findings_run::{DetectError, Detector};
use crate::known::{KnownIndex, KnownProduct, KnownProductRepository, Standing};
use crate::launchers::{LauncherKind, LauncherRepository, LauncherSeen};
use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

/// At most this many findings of one rule per run; the most serious stay.
pub const MAX_NEW_PER_RULE: usize = 100;

/// What an identity looked like on one side of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeFacts {
    pub sha256: Option<String>,
    pub signer: Option<String>,
    pub status: Option<SignatureStatus>,
}

/// A program's file was found with different content than when it was last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityChange {
    pub exe: Executable,
    pub old: ChangeFacts,
    pub new: ChangeFacts,
    /// When the new content was read.
    pub at: Timestamp,
}

/// Where the changes of a program's file are kept (written by `save_identity` when the content differs). Permanent.
pub trait IdentityChangeRepository {
    /// The changes read at or after `since`, oldest first.
    fn identity_changes_since(&self, since: Timestamp) -> Result<Vec<IdentityChange>, RepoError>;
}

/// Everything the rules look at.
pub struct NewThingsFacts<'a> {
    /// When detection was first switched on: only what came after counts as new.
    pub since: Timestamp,
    pub now: Timestamp,
    pub known: &'a [KnownProduct],
    pub entries: &'a [IdentityEntry],
    pub launchers: &'a [(Executable, Vec<LauncherSeen>)],
    pub contexts: &'a [(Executable, ExecutableContext)],
    pub changes: &'a [IdentityChange],
}

fn file_name(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
}

fn writable(class: LocationClass) -> bool {
    matches!(
        class,
        LocationClass::UserProfile | LocationClass::Temporary | LocationClass::Network | LocationClass::ProgramData
    )
}

pub(crate) fn location_label(class: LocationClass) -> &'static str {
    match class {
        LocationClass::System => "the Windows folder",
        LocationClass::ProgramFiles => "Program Files",
        LocationClass::ProgramData => "ProgramData",
        LocationClass::UserProfile => "a user folder",
        LocationClass::Temporary => "a temporary folder",
        LocationClass::Network => "a network share",
        LocationClass::Other => "another folder",
        LocationClass::Unknown => "an unknown location",
    }
}

fn kind_label(kind: LauncherKind) -> &'static str {
    match kind {
        LauncherKind::ServiceAuto => "a service set to start automatically",
        LauncherKind::ServiceOther => "a service",
        LauncherKind::Task => "a scheduled task",
        LauncherKind::RunKey => "a Run key",
        LauncherKind::RunOnce => "a RunOnce key",
        LauncherKind::StartupFolder => "a Startup folder",
    }
}

pub(crate) fn signature_line(facts: Option<&FileFacts>) -> String {
    let Some(facts) = facts else { return "Signature: the file has not been read yet".to_owned() };
    let signer = facts.signature.signer.as_deref();
    match (facts.signature.status, signer) {
        (SignatureStatus::Valid, Some(signer)) => format!("Signature: signed by {signer} (trusted)"),
        (SignatureStatus::Valid, None) => "Signature: signed (trusted)".to_owned(),
        (SignatureStatus::Untrusted, Some(signer)) => format!("Signature: signed by {signer}, but not trusted"),
        (SignatureStatus::Untrusted, None) => "Signature: signed, but not trusted".to_owned(),
        (SignatureStatus::Altered, _) => "Signature: the file was changed after it was signed".to_owned(),
        (SignatureStatus::Unsigned, _) => "Signature: not signed".to_owned(),
    }
}

fn facts_of(entry: &IdentityEntry) -> Option<&FileFacts> {
    entry.record.as_ref().and_then(|record| record.facts.as_ref())
}

/// Why the evidence deserves a closer look, as phrases for the sentence; empty when it does not.
pub(crate) fn concerns(facts: Option<&FileFacts>, class: LocationClass) -> Vec<&'static str> {
    let Some(facts) = facts else { return Vec::new() };
    match facts.signature.status {
        SignatureStatus::Altered => vec!["its file was changed after it was signed"],
        SignatureStatus::Valid => Vec::new(),
        SignatureStatus::Untrusted if writable(class) => {
            vec!["its signature is not trusted", "it runs from a place any user can write"]
        }
        SignatureStatus::Unsigned if class == LocationClass::UserProfile => {
            vec!["it is not signed", "it runs from a user folder"]
        }
        SignatureStatus::Unsigned if writable(class) => {
            vec!["it is not signed", "it runs from a folder users can write"]
        }
        _ => Vec::new(),
    }
}

fn sentence(lead: &str, reasons: &[&str]) -> String {
    if reasons.is_empty() {
        format!("{lead}.")
    } else {
        format!("{lead}, and {}.", reasons.join(" and "))
    }
}

/// Keeps the `MAX_NEW_PER_RULE` most serious, in their order otherwise.
fn limited(mut found: Vec<Candidate>) -> Vec<Candidate> {
    found.sort_by_key(|candidate| std::cmp::Reverse(candidate.tier));
    found.truncate(MAX_NEW_PER_RULE);
    found
}

fn starts_with_windows_lines(exe: &Executable, launchers: &[(Executable, Vec<LauncherSeen>)]) -> Vec<String> {
    launchers
        .iter()
        .filter(|(other, _)| other.key == exe.key)
        .flat_map(|(_, entries)| entries.iter())
        .filter(|entry| entry.current && entry.kind.starts_with_windows())
        .map(|entry| format!("It starts with Windows via {} ({})", kind_label(entry.kind), entry.location))
        .collect()
}

// ---- new program

/// A program already here (seen before the switch-on) that is the same product by the same maker in the same folder.
fn is_update(new: &IdentityEntry, entries: &[IdentityEntry], since: Timestamp) -> bool {
    let Some(new_key) = product_key(&new.exe.path, facts_of(new)) else { return false };
    if new_key.publisher.is_none() {
        return false;
    }
    entries.iter().any(|old| {
        old.exe.key != new.exe.key
            && old.seen.is_some_and(|seen| seen.first_seen < since)
            && product_key(&old.exe.path, facts_of(old)).is_some_and(|key| key == new_key)
    })
}

fn new_programs(facts: &NewThingsFacts<'_>) -> Vec<Candidate> {
    let index =
        KnownIndex::build(facts.known.to_vec(), facts.entries.iter().map(|e| (e.exe.path.as_str(), facts_of(e))));
    let mut found = Vec::new();
    for entry in facts.entries {
        let Some(seen) = entry.seen else { continue };
        if seen.first_seen < facts.since {
            continue;
        }
        let file = facts_of(entry);
        let standing = index.standing(&entry.exe.path, file);
        if matches!(standing, Standing::Known { .. }) || is_update(entry, facts.entries, facts.since) {
            continue;
        }
        let class = classify_location(&entry.exe.path);
        let mut reasons = concerns(file, class);
        let mut evidence = vec![
            format!("Path: {}", entry.exe.path),
            format!("Location: {}", location_label(class)),
            signature_line(file),
        ];
        if let Some(file) = file {
            if let Some(product) = file.version.product.as_deref().or(file.version.company.as_deref()) {
                evidence.push(format!("Names itself: {product}"));
            }
            if file.downloaded() {
                evidence.push("Marked as downloaded from the Internet".to_owned());
            }
        }
        if let Standing::Lookalike { label, .. } = &standing {
            reasons.push("it looks like one of your known programs without being it");
            evidence.push(format!("Looks like your known program {label}"));
        }
        evidence.extend(starts_with_windows_lines(&entry.exe, facts.launchers));
        let tier = if reasons.is_empty() { Tier::Unusual } else { Tier::Suspicious };
        let name = file_name(&entry.exe.path);
        found.push(Candidate {
            kind: "new_program".into(),
            subject: entry.exe.key.clone(),
            subject_label: name.clone(),
            tier,
            title: format!("New program {name}"),
            why: sentence(
                "It was first seen running after detection was switched on and no known program covers it",
                &reasons,
            ),
            evidence,
            dedupe_key: format!("new_program:{}", entry.exe.key),
            level: None,
            at: facts.now,
        });
    }
    limited(found)
}

// ---- new start entry

fn new_start_entries(facts: &NewThingsFacts<'_>) -> Vec<Candidate> {
    let by_key: std::collections::HashMap<&str, &IdentityEntry> =
        facts.entries.iter().map(|e| (e.exe.key.as_str(), e)).collect();
    let mut found = Vec::new();
    for (exe, entries) in facts.launchers {
        let identity = by_key.get(exe.key.as_str()).copied();
        // A program that is itself new has its own finding, which already says how it starts.
        if identity.and_then(|e| e.seen).is_some_and(|seen| seen.first_seen >= facts.since) {
            continue;
        }
        let file = identity.and_then(facts_of);
        let class = classify_location(&exe.path);
        for entry in entries.iter().filter(|e| e.current && e.first_seen >= facts.since) {
            let reasons = concerns(file, class);
            let lead = if entry.kind.starts_with_windows() {
                format!(
                    "New entry: this program starts with Windows via {}, first seen after detection was switched on",
                    kind_label(entry.kind)
                )
            } else {
                format!(
                    "New entry: this program can be started by {}, first seen after detection was switched on",
                    kind_label(entry.kind)
                )
            };
            let mut evidence = vec![
                format!("Program: {}", exe.path),
                format!("Entry: {} ({})", kind_label(entry.kind), entry.location),
            ];
            if !entry.arguments.is_empty() {
                evidence.push(format!("Arguments: {}", entry.arguments));
            }
            evidence.push(format!("Location of the program: {}", location_label(class)));
            evidence.push(signature_line(file));
            let subject = format!("{}|{}|{}", exe.key, entry.kind.as_str(), entry.location.to_lowercase());
            let name = file_name(&exe.path);
            found.push(Candidate {
                kind: "new_start_entry".into(),
                subject: subject.clone(),
                subject_label: name.clone(),
                tier: if reasons.is_empty() { Tier::Unusual } else { Tier::Suspicious },
                title: format!("New start entry for {name}"),
                why: sentence(&lead, &reasons),
                evidence,
                dedupe_key: format!("new_start_entry:{subject}"),
                level: None,
                at: facts.now,
            });
        }
    }
    limited(found)
}

// ---- changed file

fn same_signer(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        (None, None) => true,
        _ => false,
    }
}

fn side_line(label: &str, side: &ChangeFacts) -> String {
    let signature = match (side.status, side.signer.as_deref()) {
        (Some(SignatureStatus::Valid), Some(signer)) => format!("signed by {signer} (trusted)"),
        (Some(SignatureStatus::Valid), None) => "signed (trusted)".to_owned(),
        (Some(SignatureStatus::Untrusted), Some(signer)) => format!("signed by {signer}, but not trusted"),
        (Some(SignatureStatus::Untrusted), None) => "signed, but not trusted".to_owned(),
        (Some(SignatureStatus::Altered), _) => "changed after it was signed".to_owned(),
        (Some(SignatureStatus::Unsigned), _) => "not signed".to_owned(),
        (None, _) => "signature not read".to_owned(),
    };
    let content = side
        .sha256
        .as_deref()
        .map_or("content not read".to_owned(), |sha| format!("content {}", sha.chars().take(12).collect::<String>()));
    format!("{label}: {signature}, {content}")
}

fn changed_files(facts: &NewThingsFacts<'_>) -> Vec<Candidate> {
    let mut found = Vec::new();
    for change in facts.changes.iter().filter(|c| c.at >= facts.since) {
        if change.old.sha256 == change.new.sha256 {
            continue;
        }
        let signer_changed = change.old.signer.is_some() && !same_signer(&change.old.signer, &change.new.signer);
        let was_valid = change.old.status == Some(SignatureStatus::Valid);
        let no_longer_valid = was_valid && change.new.status != Some(SignatureStatus::Valid);
        if !signer_changed && !no_longer_valid {
            continue; // a new version by the same maker: an update
        }
        let reason = if signer_changed { "its signer changed" } else { "it was validly signed and no longer is" };
        let name = file_name(&change.exe.path);
        let new_content =
            change.new.sha256.as_deref().map_or("unknown".to_owned(), |sha| sha.chars().take(12).collect());
        found.push(Candidate {
            kind: "changed_file".into(),
            subject: change.exe.key.clone(),
            subject_label: name.clone(),
            tier: Tier::Suspicious,
            title: format!("File changed: {name}"),
            why: format!("The file now has different content, and {reason}."),
            evidence: vec![
                format!("Path: {}", change.exe.path),
                side_line("Before", &change.old),
                side_line("Now", &change.new),
            ],
            dedupe_key: format!("changed_file:{}:{new_content}", change.exe.key),
            level: None,
            at: facts.now,
        });
    }
    limited(found)
}

// ---- new account

fn account_name(name: &Option<String>, sid: &str) -> String {
    name.clone().unwrap_or_else(|| sid.to_owned())
}

fn new_accounts(facts: &NewThingsFacts<'_>) -> Vec<Candidate> {
    let by_key: std::collections::HashMap<&str, &IdentityEntry> =
        facts.entries.iter().map(|e| (e.exe.key.as_str(), e)).collect();
    let mut found = Vec::new();
    let mut seen_subjects: HashSet<String> = HashSet::new();
    for (exe, context) in facts.contexts {
        let identity = by_key.get(exe.key.as_str()).copied();
        // Only a program that was already here: a new program's first account is not a change.
        if identity.and_then(|e| e.seen).is_none_or(|seen| seen.first_seen >= facts.since) {
            continue;
        }
        let before: Vec<_> = context.accounts.iter().filter(|a| a.first_seen < facts.since).collect();
        if before.is_empty() {
            continue;
        }
        let file = identity.and_then(facts_of);
        for account in context.accounts.iter().filter(|a| a.first_seen >= facts.since) {
            let subject = format!("{}|{}", exe.key, account.sid.to_lowercase());
            if !seen_subjects.insert(subject.clone()) {
                continue;
            }
            let privileged = account.kind == AccountKind::System;
            let unproven = file.is_some_and(|f| f.signature.status != SignatureStatus::Valid);
            let suspicious = privileged && unproven;
            let name = file_name(&exe.path);
            let now_as = account_name(&account.name, &account.sid);
            let previously: Vec<String> = before.iter().map(|a| account_name(&a.name, &a.sid)).collect();
            let reasons: &[&str] = if suspicious { &["it is not validly signed"] } else { &[] };
            found.push(Candidate {
                kind: "new_account".into(),
                subject: subject.clone(),
                subject_label: name.clone(),
                tier: if suspicious { Tier::Suspicious } else { Tier::Unusual },
                title: format!("{name} now runs as {now_as}"),
                why: sentence(
                    &format!("This program now runs as {now_as}, an account it had not run as before"),
                    reasons,
                ),
                evidence: vec![
                    format!("Program: {}", exe.path),
                    format!("Now also runs as: {now_as}"),
                    format!("Before it ran as: {}", previously.join(", ")),
                    signature_line(file),
                ],
                dedupe_key: format!("new_account:{subject}"),
                level: None,
                at: facts.now,
            });
        }
    }
    limited(found)
}

/// Every candidate of the four rules.
pub fn new_thing_candidates(facts: &NewThingsFacts<'_>) -> Vec<Candidate> {
    let mut all = new_programs(facts);
    all.extend(new_start_entries(facts));
    all.extend(changed_files(facts));
    all.extend(new_accounts(facts));
    all
}

fn failed(what: &'static str) -> impl FnOnce(RepoError) -> DetectError {
    move |error| DetectError(format!("could not read {what}: {error}"))
}

/// The rules as a detector: loads the facts through read ports and applies `new_thing_candidates`.
pub struct NewThingsDetector<R> {
    repo: R,
}

impl<R> NewThingsDetector<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

impl<R> Detector for NewThingsDetector<R>
where
    R: IdentityRepository + LauncherRepository + ContextRepository + KnownProductRepository + IdentityChangeRepository,
{
    fn name(&self) -> &'static str {
        "new things"
    }

    fn detect(&self, since: Timestamp, now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        let entries = self.repo.identity_entries().map_err(failed("the programs"))?;
        let known = self.repo.known_products().map_err(failed("the known programs"))?;
        let launchers = self.repo.all_launchers().map_err(failed("the start entries"))?;
        let contexts = self.repo.all_contexts().map_err(failed("the accounts"))?;
        let changes = self.repo.identity_changes_since(since).map_err(failed("the file changes"))?;
        Ok(new_thing_candidates(&NewThingsFacts {
            since,
            now,
            known: &known,
            entries: &entries,
            launchers: &launchers,
            contexts: &contexts,
            changes: &changes,
        }))
    }
}
