//! How programs stand against the user's list of known programs (`docs/KNOWN-PROGRAMS-DESIGN.md`), and the Known programs tab.
//! Same conventions as the other views: camelCase on the wire, times in milliseconds since the Unix epoch.

use serde::Serialize;
use syntrac_core::{
    product_key, IdentityRepository, KnownProduct, KnownProductRepository, LookalikeReason, RepoError, Standing,
};

use crate::history::ms;
use crate::identity_view::Identities;
use crate::names::display_name;

/// Where a program stands against the list. Evidence for the screen, not a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingView {
    /// `unlisted`, `known` or `lookalike`.
    pub state: String,
    pub id: Option<i64>,
    /// The known product's label (for `known` and `lookalike`).
    pub label: Option<String>,
    /// What the known product is signed by (lower case), when it has a publisher. Only for `lookalike`.
    pub expected_publisher: Option<String>,
    /// The folder the known product lives in (lower case). Only for `lookalike`.
    pub expected_folder: Option<String>,
    /// Why a lookalike looks like the known product, in plain sentences.
    pub reasons: Vec<String>,
}

impl StandingView {
    pub fn unlisted() -> Self {
        Self {
            state: "unlisted".into(),
            id: None,
            label: None,
            expected_publisher: None,
            expected_folder: None,
            reasons: Vec::new(),
        }
    }
}

fn reason_text(reason: &LookalikeReason, label: &str) -> String {
    match reason {
        LookalikeReason::SameProductName(name) => format!("its version information names it \"{name}\""),
        LookalikeReason::SameFileName(file) => {
            format!("it has the file name {file}, like a program of {label}, but is not validly signed")
        }
    }
}

pub fn standing_view(standing: Standing, products: &[KnownProduct]) -> StandingView {
    match standing {
        Standing::Unlisted => StandingView::unlisted(),
        Standing::Known { id, label } => {
            StandingView { state: "known".into(), id: Some(id), label: Some(label), ..StandingView::unlisted() }
        }
        Standing::Lookalike { id, label, reasons } => {
            let product = products.iter().find(|product| product.id == id);
            StandingView {
                state: "lookalike".into(),
                id: Some(id),
                reasons: reasons.iter().map(|reason| reason_text(reason, &label)).collect(),
                label: Some(label),
                expected_publisher: product.and_then(|p| p.publisher.clone()),
                expected_folder: product.map(|p| p.root.clone()),
            }
        }
    }
}

/// One standing for a product row: a lookalike among its programs shows (it must not hide behind known siblings); otherwise
/// it is known when every program is, else unlisted.
pub fn merged_standing(members: &[StandingView]) -> StandingView {
    if let Some(lookalike) = members.iter().find(|m| m.state == "lookalike") {
        return lookalike.clone();
    }
    match members.first() {
        Some(first) if members.iter().all(|m| m.state == "known" && m.id == first.id) => first.clone(),
        _ => StandingView::unlisted(),
    }
}

/// A known product, with the programs on this machine that belong to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownProductView {
    pub id: i64,
    pub label: String,
    pub publisher: Option<String>,
    pub folder: String,
    pub product_name: Option<String>,
    pub note: Option<String>,
    pub created_ms: i64,
    /// File names of its programs seen on this machine, in order.
    pub programs: Vec<String>,
}

/// A program that looks like a known one without being it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookalikeView {
    pub name: String,
    pub path: String,
    pub standing: StandingView,
    pub signature: Option<String>,
    pub publisher: Option<String>,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownProgramsView {
    pub products: Vec<KnownProductView>,
    pub lookalikes: Vec<LookalikeView>,
}

/// The most lookalikes listed: a screen of them is already a finding, and the list is read on every refresh.
const MAX_LOOKALIKES: usize = 200;

pub fn known_programs<R: KnownProductRepository + IdentityRepository + syntrac_core::ContextRepository>(
    repo: &R,
) -> Result<KnownProgramsView, RepoError> {
    let identities = Identities::load(repo)?;
    let mut programs = identities.all();
    programs.sort_by(|a, b| a.exe.key.cmp(&b.exe.key));

    let products = identities
        .products()
        .iter()
        .map(|product| KnownProductView {
            id: product.id,
            label: product.label.clone(),
            publisher: product.publisher.clone(),
            folder: product.root.clone(),
            product_name: product.product_name.clone(),
            note: product.note.clone(),
            created_ms: ms(product.created_at),
            programs: programs
                .iter()
                .filter(|record| {
                    let facts = record.facts.as_ref();
                    facts.is_some_and(|facts| product_key(&record.exe.path, Some(facts)) == Some(product.key()))
                })
                .map(|record| display_name(&record.exe.path, 0))
                .collect(),
        })
        .collect();

    let lookalikes = programs
        .iter()
        .filter_map(|record| {
            let standing = identities.standing(&record.exe.path);
            (standing.state == "lookalike").then(|| {
                let badge = crate::identity_view::badge(&record.exe.path, Some(record));
                LookalikeView {
                    name: display_name(&record.exe.path, 0),
                    path: record.exe.path.clone(),
                    standing,
                    signature: badge.signature,
                    publisher: badge.publisher,
                    location: badge.location,
                }
            })
        })
        .take(MAX_LOOKALIKES)
        .collect();
    Ok(KnownProgramsView { products, lookalikes })
}
