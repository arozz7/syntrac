//! "Review what is here today" (docs/ALERTS-PLAN.md, F4). The programs that were already here when detection was switched on are never
//! raised as "new", so nothing ever asks about them. This groups them by product (the maker and the install folder) so the user can look
//! at them once, see which look worth a closer look, and say what is expected. Pure: the entries go in, the groups come out.

use std::collections::BTreeMap;

use crate::exe_identity::{FileFacts, IdentityEntry};
use crate::exe_location::{classify_location, product_key};
use crate::findings::{expectation_covers, Expectation};
use crate::known::{KnownIndex, KnownProduct, Standing};
use crate::new_things::{concerns, location_label};
use crate::time::Timestamp;

/// How many programs of a group are listed by name (the count says the rest).
pub const PROGRAMS_LISTED: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewProgram {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGroup {
    /// The install folder that names the product, lower case. What "mark as expected" covers.
    pub root: String,
    /// The signer (or company) all its programs share; none for unsigned programs.
    pub publisher: Option<String>,
    /// The product name the files give themselves, else the signer, else the folder's name.
    pub label: String,
    /// How many programs are in the group.
    pub count: usize,
    /// The first `PROGRAMS_LISTED` of them, by name.
    pub programs: Vec<ReviewProgram>,
    /// Why the group deserves a closer look, as phrases ("it is not signed", "it runs from a user folder"); empty when it does not.
    pub concerns: Vec<String>,
    pub location: &'static str,
}

fn file_name(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
}

fn facts_of(entry: &IdentityEntry) -> Option<&FileFacts> {
    entry.record.as_ref().and_then(|record| record.facts.as_ref())
}

fn most_common(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    // The highest count; a tie goes to the name that sorts first, so the label does not change between runs.
    counts.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).map(|(value, _)| value)
}

fn trimmed(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned)
}

/// The groups of programs seen running before `switched_on_at` (every program seen running when detection has not been switched on),
/// that no known product and no expectation of the user covers. Groups with something to look at come first, then the bigger groups.
pub fn review_groups(
    entries: &[IdentityEntry],
    known: &[KnownProduct],
    expectations: &[Expectation],
    switched_on_at: Option<Timestamp>,
) -> Vec<ReviewGroup> {
    let index = KnownIndex::build(known.to_vec(), entries.iter().map(|e| (e.exe.path.as_str(), facts_of(e))));
    let mut by_key: BTreeMap<(Option<String>, String), Vec<&IdentityEntry>> = BTreeMap::new();
    for entry in entries {
        let Some(seen) = entry.seen else { continue };
        if switched_on_at.is_some_and(|on| seen.first_seen >= on) {
            continue;
        }
        let file = facts_of(entry);
        if matches!(index.standing(&entry.exe.path, file), Standing::Known { .. })
            || expectation_covers(expectations, "new_program", &entry.exe.key)
        {
            continue;
        }
        let Some(key) = product_key(&entry.exe.path, file) else { continue };
        by_key.entry((key.publisher, key.root)).or_default().push(entry);
    }

    let mut groups: Vec<ReviewGroup> = by_key
        .into_iter()
        .map(|((publisher, root), members)| {
            let facts: Vec<Option<&FileFacts>> = members.iter().map(|m| facts_of(m)).collect();
            let product = most_common(facts.iter().flatten().filter_map(|f| trimmed(&f.version.product)));
            let signer = most_common(
                facts
                    .iter()
                    .flatten()
                    .filter_map(|f| trimmed(&f.signature.signer).or_else(|| trimmed(&f.version.company))),
            );
            let folder = root.rsplit('\\').next().filter(|name| !name.is_empty()).unwrap_or(&root).to_owned();
            let mut concern_list: Vec<String> = Vec::new();
            for member in &members {
                let class = classify_location(&member.exe.path);
                for phrase in concerns(facts_of(member), class) {
                    if !concern_list.iter().any(|existing| existing == phrase) {
                        concern_list.push(phrase.to_owned());
                    }
                }
            }
            let mut sorted: Vec<&&IdentityEntry> = members.iter().collect();
            sorted.sort_by(|a, b| a.exe.key.cmp(&b.exe.key));
            ReviewGroup {
                label: product.or(signer).unwrap_or(folder),
                count: members.len(),
                programs: sorted
                    .iter()
                    .take(PROGRAMS_LISTED)
                    .map(|m| ReviewProgram { path: m.exe.path.clone(), name: file_name(&m.exe.path) })
                    .collect(),
                concerns: concern_list,
                location: location_label(classify_location(&members[0].exe.path)),
                publisher,
                root,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        a.concerns
            .is_empty()
            .cmp(&b.concerns.is_empty())
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.root.cmp(&b.root))
    });
    groups
}
