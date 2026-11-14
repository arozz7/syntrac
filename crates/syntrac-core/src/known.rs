//! Known programs: products the user has confirmed (Acme, their backup software), and how any other program stands
//! against that list. PURE: no storage, no files. Design and decisions: `docs/KNOWN-PROGRAMS-DESIGN.md`.
//!
//! A KNOWN product is identified exactly as `product_key` identifies a product: its publisher (the signer, else the company in
//! the version information) and its install folder. The service builds one from what IT measured about an executable, never from
//! values a client sends, so nobody can ask for "signed by Acme" without the file actually being so.

use std::collections::{HashMap, HashSet};

use crate::exe_identity::{FileFacts, IdentityRecord, SignatureStatus};
use crate::exe_location::{product_key, ProductKey};
use crate::ports::RepoError;
use crate::time::Timestamp;

pub const MAX_LABEL_CHARS: usize = 80;
pub const MAX_NOTE_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnownError {
    #[error("{0}")]
    Invalid(String),
    #[error(
        "This program's file has not been checked yet, so there is nothing to confirm. Try again once it has been."
    )]
    NotInspected,
    #[error("This program's file could not be read, so nothing is known about it.")]
    Unreadable,
    #[error("This file has been changed since it was signed, so it cannot be marked as known.")]
    Altered,
}

/// A product the user has confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownProduct {
    pub id: i64,
    pub label: String,
    /// Lower case, as in `ProductKey`; `None` for a product with no signer and no company.
    pub publisher: Option<String>,
    /// The install folder, lower case.
    pub root: String,
    /// The product name in the version information when it was confirmed.
    pub product_name: Option<String>,
    pub note: Option<String>,
    pub created_at: Timestamp,
}

impl KnownProduct {
    pub fn key(&self) -> ProductKey {
        ProductKey { publisher: self.publisher.clone(), root: self.root.clone() }
    }
}

/// A product to be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKnownProduct {
    pub label: String,
    pub publisher: Option<String>,
    pub root: String,
    pub product_name: Option<String>,
    pub note: Option<String>,
}

/// Where known products are kept.
pub trait KnownProductRepository {
    fn known_products(&self) -> Result<Vec<KnownProduct>, RepoError>;

    /// Adds the product, or (when its publisher and folder are already known) updates its label and note. Returns its id.
    fn add_known_product(&mut self, product: &NewKnownProduct, at: Timestamp) -> Result<i64, RepoError>;

    /// `false` when there was no such product.
    fn remove_known_product(&mut self, id: i64) -> Result<bool, RepoError>;
}

fn printable(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

pub fn clean_label(label: &str) -> Result<String, KnownError> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(KnownError::Invalid(format!("The label must be 1 to {MAX_LABEL_CHARS} characters.")));
    }
    if !printable(label) {
        return Err(KnownError::Invalid("The label may not contain control characters or line breaks.".into()));
    }
    Ok(label.to_owned())
}

pub fn clean_note(note: Option<&str>) -> Result<Option<String>, KnownError> {
    let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) else { return Ok(None) };
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(KnownError::Invalid(format!("The note may be at most {MAX_NOTE_CHARS} characters.")));
    }
    if !printable(note) {
        return Err(KnownError::Invalid("The note may not contain control characters or line breaks.".into()));
    }
    Ok(Some(note.to_owned()))
}

/// Builds a known product from the identity the service measured for `path`. The label and note are cleaned first.
pub fn new_known_product(
    path: &str,
    record: Option<&IdentityRecord>,
    label: &str,
    note: Option<&str>,
) -> Result<NewKnownProduct, KnownError> {
    let label = clean_label(label)?;
    let note = clean_note(note)?;
    let record = record.ok_or(KnownError::NotInspected)?;
    let facts = record.facts.as_ref().ok_or(if record.failure.is_some() {
        KnownError::Unreadable
    } else {
        KnownError::NotInspected
    })?;
    if facts.signature.status == SignatureStatus::Altered {
        return Err(KnownError::Altered);
    }
    let key = product_key(path, Some(facts)).ok_or_else(|| {
        KnownError::Invalid("That is not a program with a full path, so it cannot be marked as known.".into())
    })?;
    Ok(NewKnownProduct {
        label,
        publisher: key.publisher,
        root: key.root,
        product_name: facts.version.product.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned),
        note,
    })
}

/// Why a program that is not known looks like a known one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookalikeReason {
    /// Its version information names the known product (the name given is the known product's).
    SameProductName(String),
    /// It has the file name of a program of the known product, and no valid signature to say otherwise (the file name, lower case).
    SameFileName(String),
}

/// How a program stands against the list of known products.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Not on the list, and not claiming to be anything on it. Also what a file that has not been read yet gets.
    Unlisted,
    Known {
        id: i64,
        label: String,
    },
    Lookalike {
        id: i64,
        label: String,
        reasons: Vec<LookalikeReason>,
    },
}

/// The known products, and the file names of the programs each one has on this machine.
pub struct KnownIndex {
    products: Vec<KnownProduct>,
    members: HashMap<i64, HashSet<String>>,
}

fn file_name(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase()
}

/// The maker's name without the legal form and the spelling of one particular signer string: its first significant word, lower
/// case (Microsoft Windows, Microsoft Windows Publisher and MICROSOFT CORPORATION are all microsoft; The darktable team is
/// darktable). Used to tell the same maker's other components from someone else's.
fn maker(publisher: &str) -> Option<String> {
    publisher
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .find(|word| word.len() >= 2 && *word != "the")
        .map(str::to_owned)
}

fn lower(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_lowercase)
}

impl KnownIndex {
    /// `programs` are all the programs seen on the machine, with their facts when they have been read.
    pub fn build<'a>(
        mut products: Vec<KnownProduct>,
        programs: impl IntoIterator<Item = (&'a str, Option<&'a FileFacts>)>,
    ) -> Self {
        products.sort_by_key(|product| product.id);
        let mut members: HashMap<i64, HashSet<String>> = HashMap::new();
        for (path, facts) in programs {
            let Some(facts) = facts else { continue };
            let Some(key) = product_key(path, Some(facts)) else { continue };
            for product in products.iter().filter(|product| product.key() == key) {
                members.entry(product.id).or_default().insert(file_name(path));
            }
        }
        Self { products, members }
    }

    pub fn is_empty(&self) -> bool {
        self.products.is_empty()
    }

    pub fn standing(&self, path: &str, facts: Option<&FileFacts>) -> Standing {
        let Some(facts) = facts else { return Standing::Unlisted };
        let Some(key) = product_key(path, Some(facts)) else { return Standing::Unlisted };
        let mut first_lookalike: Option<Standing> = None;
        for product in &self.products {
            if product.key() == key {
                return Standing::Known { id: product.id, label: product.label.clone() };
            }
            let reasons = self.reasons(product, path, facts, &key);
            if !reasons.is_empty() && first_lookalike.is_none() {
                first_lookalike = Some(Standing::Lookalike { id: product.id, label: product.label.clone(), reasons });
            }
        }
        first_lookalike.unwrap_or(Standing::Unlisted)
    }

    fn reasons(&self, product: &KnownProduct, path: &str, facts: &FileFacts, key: &ProductKey) -> Vec<LookalikeReason> {
        let valid = facts.signature.status == SignatureStatus::Valid;
        let mut reasons = Vec::new();
        let claimed = lower(&facts.version.product);
        if let (Some(claimed), Some(known)) = (claimed, product.product_name.as_ref()) {
            // Not suspicious: a valid signature by the same maker (a product's name is on all of its files, wherever they are
            // installed, and makers sign under several names), or a valid signature inside the product's own folder.
            let same_maker = matches!(
                (key.publisher.as_deref().and_then(maker), product.publisher.as_deref().and_then(maker)),
                (Some(theirs), Some(ours)) if theirs == ours
            );
            let benign = valid && (same_maker || key.root == product.root);
            if claimed == known.to_lowercase() && !benign {
                reasons.push(LookalikeReason::SameProductName(known.clone()));
            }
        }
        let name = file_name(path);
        let shares_name = self.members.get(&product.id).is_some_and(|names| names.contains(&name));
        if shares_name && !valid {
            reasons.push(LookalikeReason::SameFileName(name));
        }
        reasons
    }
}
