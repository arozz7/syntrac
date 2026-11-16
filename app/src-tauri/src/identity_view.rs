//! What the app says about WHO a program is, built from the identity records the service keeps (`docs/IDENTITY-PLAN.md`).
//!
//! Everything here is evidence for the screen, never a verdict: a missing record is "pending" (not yet looked at), a record with
//! no facts is "unreadable" (we do not know), and only measured facts are reported as such. Same conventions as the other views:
//! camelCase on the wire, times in milliseconds since the Unix epoch.

use std::collections::HashMap;

use serde::Serialize;
use syntrac_core::rollup::executable_key;
use syntrac_core::{
    classify_location, install_root, ContextRepository, ExecutableContext, IdentityRecord, IdentityRepository,
    KnownIndex, KnownProduct, KnownProductRepository, LauncherRepository, LauncherSeen, RepoError,
};

use crate::history::ms;
use crate::known_view::{standing_view, StandingView};

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
    /// The kinds of account it has been SEEN running as, most recent first, each once (`system`, `local_service`,
    /// `network_service`, `service_account`, `user`, `other`). Empty when it has never been observed running (not "none").
    pub runs_as: Vec<String>,
    /// It has been seen hosting a Windows service.
    pub service: bool,
    /// Something that starts with Windows (an automatic service, a scheduled task, a Run key, a Startup folder entry) was found
    /// for it and is still there. `false` means none was found in the places read, never "does not autostart".
    pub starts_at_boot: bool,
}

/// One account a program has been seen running as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    /// As Windows names it (`NT AUTHORITY\SYSTEM`), when it could.
    pub name: Option<String>,
    pub kind: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
}

/// One way a program has been seen to get started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherView {
    /// `service_auto`, `service_other`, `task`, `run_key`, `run_once` or `startup_folder`.
    pub kind: String,
    /// Where it was found: the service name, the task path, the key and value name, the shortcut.
    pub location: String,
    /// The kinds of trigger of a task, the start type of a service.
    pub detail: String,
    pub arguments: String,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    /// It was still there when its kind was last read completely.
    pub current: bool,
}

/// One Windows service a program has been seen hosting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceView {
    pub name: String,
    pub display_name: Option<String>,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
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
    /// The accounts it has been seen running as, most recent first. Empty when it has never been observed running.
    pub accounts: Vec<AccountView>,
    /// The Windows services it has been seen hosting, most recent first.
    pub services: Vec<ServiceView>,
    /// How it has been seen to get started, most recent first. Empty when nothing was found (not "does not autostart").
    pub launchers: Vec<LauncherView>,
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
        runs_as: Vec::new(),
        service: false,
        starts_at_boot: false,
    }
}

/// The kinds of account in a context, most recent first, each once.
fn account_kinds(context: &ExecutableContext) -> Vec<String> {
    let mut kinds: Vec<String> = Vec::new();
    for account in &context.accounts {
        let kind = account.kind.as_str().to_owned();
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// The badge of a program with what has been seen of how it runs.
pub fn with_context(mut badge: IdentityBadgeView, context: &ExecutableContext) -> IdentityBadgeView {
    badge.runs_as = account_kinds(context);
    badge.service = !context.services.is_empty();
    badge
}

/// The accounts and services of a program for its identity panel.
pub fn context_views(context: &ExecutableContext) -> (Vec<AccountView>, Vec<ServiceView>) {
    let accounts = context
        .accounts
        .iter()
        .map(|account| AccountView {
            name: account.name.clone(),
            kind: account.kind.as_str().to_owned(),
            first_seen_ms: ms(account.first_seen),
            last_seen_ms: ms(account.last_seen),
        })
        .collect();
    let services = context
        .services
        .iter()
        .map(|service| ServiceView {
            name: service.name.clone(),
            display_name: service.display_name.clone(),
            first_seen_ms: ms(service.first_seen),
            last_seen_ms: ms(service.last_seen),
        })
        .collect();
    (accounts, services)
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
        accounts: Vec::new(),
        services: Vec::new(),
        launchers: Vec::new(),
    }
}

/// The badge flag and the panel list for a program's start entries.
pub fn with_launchers(mut view: IdentityDetailView, launchers: &[LauncherSeen]) -> IdentityDetailView {
    view.badge.starts_at_boot = starts_at_boot(launchers);
    view.launchers = launcher_views(launchers);
    view
}

/// Whether any entry that starts with Windows is still there.
fn starts_at_boot(launchers: &[LauncherSeen]) -> bool {
    launchers.iter().any(|l| l.current && l.kind.starts_with_windows())
}

fn launcher_views(launchers: &[LauncherSeen]) -> Vec<LauncherView> {
    launchers
        .iter()
        .map(|l| LauncherView {
            kind: l.kind.as_str().to_owned(),
            location: l.location.clone(),
            detail: l.detail.clone(),
            arguments: l.arguments.clone(),
            first_seen_ms: ms(l.first_seen),
            last_seen_ms: ms(l.last_seen),
            current: l.current,
        })
        .collect()
}

/// All the identity records, by executable key, how programs have been seen running, and the user's known products with an
/// index over them: read once per request, so a ranking of 200 rows costs three queries.
pub struct Identities {
    records: HashMap<String, IdentityRecord>,
    contexts: HashMap<String, ExecutableContext>,
    launchers: HashMap<String, Vec<LauncherSeen>>,
    products: Vec<KnownProduct>,
    index: KnownIndex,
}

impl Identities {
    pub fn load<R: IdentityRepository + ContextRepository + LauncherRepository + KnownProductRepository + ?Sized>(
        repo: &R,
    ) -> Result<Self, RepoError> {
        let records: HashMap<String, IdentityRecord> = repo
            .identity_entries()?
            .into_iter()
            .filter_map(|entry| entry.record.map(|record| (entry.exe.key, record)))
            .collect();
        let contexts: HashMap<String, ExecutableContext> =
            repo.all_contexts()?.into_iter().map(|(exe, context)| (exe.key, context)).collect();
        let launchers: HashMap<String, Vec<LauncherSeen>> =
            repo.all_launchers()?.into_iter().map(|(exe, list)| (exe.key, list)).collect();
        let products = repo.known_products()?;
        let index = KnownIndex::build(
            products.clone(),
            records.values().map(|record| (record.exe.path.as_str(), record.facts.as_ref())),
        );
        Ok(Self { records, contexts, launchers, products, index })
    }

    pub fn of(&self, path: &str) -> Option<&IdentityRecord> {
        executable_key(path).and_then(|key| self.records.get(&key))
    }

    /// How a program has been seen running (empty when it never was).
    pub fn context_of(&self, path: &str) -> ExecutableContext {
        executable_key(path).and_then(|key| self.contexts.get(&key)).cloned().unwrap_or_default()
    }

    /// How a program has been seen to get started (empty when nothing was found).
    pub fn launchers_of(&self, path: &str) -> Vec<LauncherSeen> {
        executable_key(path).and_then(|key| self.launchers.get(&key)).cloned().unwrap_or_default()
    }

    /// The badge of a program: its identity facts, how it has been seen running and whether it starts with Windows.
    pub fn badge(&self, path: &str) -> IdentityBadgeView {
        let mut badge = with_context(badge(path, self.of(path)), &self.context_of(path));
        badge.starts_at_boot = starts_at_boot(&self.launchers_of(path));
        badge
    }

    /// Every program that has an identity record.
    pub fn all(&self) -> Vec<&IdentityRecord> {
        self.records.values().collect()
    }

    pub fn products(&self) -> &[KnownProduct] {
        &self.products
    }

    /// Where a program stands against the known products (a program whose file has not been read is not judged).
    pub fn standing(&self, path: &str) -> StandingView {
        let facts = self.of(path).and_then(|record| record.facts.as_ref());
        standing_view(self.index.standing(path, facts), &self.products)
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
        runs_as: union_in_order(members.iter().flat_map(|m| m.runs_as.iter().cloned())),
        service: members.iter().any(|m| m.service),
        starts_at_boot: members.iter().any(|m| m.starts_at_boot),
    }
}

/// Each value once, in the order it first appears.
fn union_in_order(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for value in values {
        if !seen.contains(&value) {
            seen.push(value);
        }
    }
    seen
}

/// A detail view with the accounts and services a program has been seen with, and the matching badge facts.
pub fn with_accounts(mut view: IdentityDetailView, context: &ExecutableContext) -> IdentityDetailView {
    let (accounts, services) = context_views(context);
    view.badge = with_context(view.badge, context);
    view.accounts = accounts;
    view.services = services;
    view
}
