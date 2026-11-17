//! The "new network owner" rule (docs/ALERTS-PLAN.md, F4): a program that is NOT validly signed reaching a network owner it never had
//! before. A validly signed program (a browser, an updater) reaches new owners every day, so those are only counted in the digest.
//! The rule needs the downloaded owner data; without it nothing is raised and the screen says so. Pure: the detector at the bottom
//! only gathers the facts.

use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;

use crate::addr_class::classify_addr;
use crate::destination_trend::DestinationTrendRepository;
use crate::exe_identity::{FileFacts, IdentityEntry, IdentityRepository, SignatureStatus};
use crate::exe_location::classify_location;
use crate::findings::{Candidate, Tier};
use crate::findings_run::{DetectError, Detector};
use crate::new_things::concerns;
use crate::owner_dataset::Owner;
use crate::owner_refresh::OwnerRepository;
use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::time::Timestamp;

/// At most this many findings of this rule per run.
pub const MAX_NEW_OWNERS_PER_RUN: usize = 100;

/// An address a program reached for the first time after the switch-on, and who owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachedOwner {
    pub addr: IpAddr,
    pub owner: Owner,
    pub first_seen: Timestamp,
}

/// One program with new addresses.
#[derive(Debug, Clone)]
pub struct ProgramOwners {
    pub exe: Executable,
    pub facts: Option<FileFacts>,
    /// The program itself was first seen after the switch-on (its own finding covers it).
    pub is_new: bool,
    /// The AS numbers of the addresses it reached before the switch-on.
    pub earlier_owners: HashSet<u32>,
    pub reached: Vec<ReachedOwner>,
}

pub struct NewOwnerFacts<'a> {
    pub now: Timestamp,
    pub programs: &'a [ProgramOwners],
}

fn file_name(path: &str) -> String {
    path.trim().rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
}

pub fn new_owner_candidates(facts: &NewOwnerFacts<'_>) -> Vec<Candidate> {
    let mut found = Vec::new();
    for program in facts.programs {
        let valid = program.facts.as_ref().is_some_and(|f| f.signature.status == SignatureStatus::Valid);
        if valid || program.is_new || program.earlier_owners.is_empty() {
            continue;
        }
        let class = classify_location(&program.exe.path);
        let reasons = concerns(program.facts.as_ref(), class);
        let mut by_owner: BTreeMap<u32, Vec<&ReachedOwner>> = BTreeMap::new();
        for reached in &program.reached {
            if !program.earlier_owners.contains(&reached.owner.asn) {
                by_owner.entry(reached.owner.asn).or_default().push(reached);
            }
        }
        for (asn, reached) in by_owner {
            let owner = &reached[0].owner;
            let subject = format!("{}|as{asn}", program.exe.key);
            let name = file_name(&program.exe.path);
            let label = if owner.org.is_empty() { format!("AS{asn}") } else { format!("AS{asn} {}", owner.org) };
            let mut addresses: Vec<String> = reached.iter().map(|r| r.addr.to_string()).collect();
            addresses.sort();
            found.push(Candidate {
                kind: "new_owner".into(),
                subject: subject.clone(),
                subject_label: name.clone(),
                tier: if reasons.is_empty() { Tier::Unusual } else { Tier::Suspicious },
                title: format!("{name} reached a network it never reached before"),
                why: format!(
                    "This program is not validly signed and it has never reached {label} before{}",
                    if reasons.is_empty() { ".".to_owned() } else { format!(", and {}.", reasons.join(" and ")) }
                ),
                evidence: vec![
                    format!("Program: {}", program.exe.path),
                    format!("Reached: {}", addresses.join(", ")),
                    format!("Network owner: {label}, registered in {}", owner.country),
                    format!("Before this it had reached {} other networks", program.earlier_owners.len()),
                ],
                dedupe_key: format!("new_owner:{subject}"),
                level: None,
                at: facts.now,
            });
        }
    }
    found.sort_by_key(|candidate| std::cmp::Reverse(candidate.tier));
    found.truncate(MAX_NEW_OWNERS_PER_RUN);
    found
}

/// The rule as a detector: finds the programs with addresses first reached after the switch-on, looks every address up in the owner
/// data, and applies `new_owner_candidates`.
pub struct NewOwnerDetector<R> {
    repo: R,
}

impl<R> NewOwnerDetector<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

fn failed(what: &'static str) -> impl FnOnce(RepoError) -> DetectError {
    move |error| DetectError(format!("could not read {what}: {error}"))
}

impl<R> Detector for NewOwnerDetector<R>
where
    R: DestinationTrendRepository + OwnerRepository + IdentityRepository,
{
    fn name(&self) -> &'static str {
        "new network owners"
    }

    fn detect(&self, since: Timestamp, now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        // Without the owner data there is nothing to compare: the rule is off (the Findings screen says so).
        if self.repo.dataset_info().map_err(failed("the owner data"))?.is_none() {
            return Ok(Vec::new());
        }
        let pairs = self.repo.new_pairs(since).map_err(failed("the new addresses"))?;
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        let entries: Vec<IdentityEntry> = self.repo.identity_entries().map_err(failed("the programs"))?;
        let by_key: std::collections::HashMap<&str, &IdentityEntry> =
            entries.iter().map(|e| (e.exe.key.as_str(), e)).collect();

        let mut grouped: BTreeMap<String, (Executable, Vec<ReachedOwner>)> = BTreeMap::new();
        for pair in pairs {
            if !classify_addr(pair.addr).is_internet() {
                continue;
            }
            let Some(owner) = self.repo.lookup_owner(pair.addr).map_err(failed("the owner of an address"))? else {
                continue;
            };
            grouped
                .entry(pair.exe.key.clone())
                .or_insert_with(|| (pair.exe.clone(), Vec::new()))
                .1
                .push(ReachedOwner { addr: pair.addr, owner, first_seen: pair.first_seen });
        }

        let mut programs = Vec::new();
        for (key, (exe, reached)) in grouped {
            let entry = by_key.get(key.as_str()).copied();
            let is_new = entry.and_then(|e| e.seen).is_some_and(|seen| seen.first_seen >= since);
            let facts = entry.and_then(|e| e.record.as_ref()).and_then(|r| r.facts.clone());
            let new_addrs: HashSet<IpAddr> = reached.iter().map(|r| r.addr).collect();
            let mut earlier_owners: HashSet<u32> = HashSet::new();
            // Every address of the program before the switch-on; the new ones are in `reached` and are not "earlier".
            let epoch = Timestamp::from_micros(0);
            for row in
                self.repo.program_destinations(&exe.path, epoch, now).map_err(failed("the program's addresses"))?
            {
                if new_addrs.contains(&row.addr) || !classify_addr(row.addr).is_internet() {
                    continue;
                }
                if let Some(owner) = self.repo.lookup_owner(row.addr).map_err(failed("the owner of an address"))? {
                    earlier_owners.insert(owner.asn);
                }
            }
            programs.push(ProgramOwners { exe, facts, is_new, earlier_owners, reached });
        }
        Ok(new_owner_candidates(&NewOwnerFacts { now, programs: &programs }))
    }
}
