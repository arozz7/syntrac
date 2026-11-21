//! Phase 4, slice L4b (docs/PHASE4-PLAN.md): the facts for the AI help, read through the same read-only views the screens use. This only
//! gathers; what may be told at each detail level is decided by `syntrac_core::investigate`.

use std::net::IpAddr;

use syntrac_core::investigate::{AccountFact, DestinationFact, Evidence, FindingFacts, HistoryFacts, ProgramFacts};
use syntrac_core::{
    classify_addr, ContextRepository, DestinationTrendRepository, Finding, IdentityRepository, InstallRepository,
    KnownProductRepository, LauncherRepository, OwnerRepository, RepoError, RollupRepository, Timestamp,
};

use crate::history_programs::{program_history, ProgramHistoryView};

/// The range every investigation looks at.
const RANGE_SECS: u64 = 7 * 24 * 3600;
/// How many destinations are read (the prompt itself shows fewer).
const MAX_DESTINATIONS: usize = 200;
/// Owners and names are looked up for this many of the busiest destinations only.
const NAMED_DESTINATIONS: usize = 10;
const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;

/// The reads the evidence needs: everything `program_history` reads, plus where the program connected and who owns those addresses.
pub trait EvidenceSource:
    RollupRepository
    + IdentityRepository
    + ContextRepository
    + LauncherRepository
    + InstallRepository
    + KnownProductRepository
    + DestinationTrendRepository
    + OwnerRepository
{
}

impl<T> EvidenceSource for T where
    T: RollupRepository
        + IdentityRepository
        + ContextRepository
        + LauncherRepository
        + InstallRepository
        + KnownProductRepository
        + DestinationTrendRepository
        + OwnerRepository
{
}

/// The program a finding is about, when its subject starts with a Windows file path (`C:\...\x.exe` or `C:\...\x.exe|cpu`). Subjects of
/// other shapes (an address, a folder) name no program.
pub fn program_path_of_subject(subject: &str) -> Option<&str> {
    let path = subject.split('|').next().unwrap_or(subject).trim();
    let bytes = path.as_bytes();
    let drive_path =
        bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
    drive_path.then_some(path)
}

/// Everything known about one program.
pub fn program_evidence<R: EvidenceSource>(
    repo: &R,
    now: Timestamp,
    path: &str,
    user_name: Option<String>,
) -> Result<Evidence, RepoError> {
    Ok(Evidence { finding: None, program: Some(program_facts(repo, now, path)?), user_name })
}

/// One finding and, when its subject names a program, that program too.
pub fn finding_evidence<R: EvidenceSource>(
    repo: &R,
    now: Timestamp,
    finding: &Finding,
    user_name: Option<String>,
) -> Result<Evidence, RepoError> {
    let program = match program_path_of_subject(&finding.subject) {
        Some(path) => Some(program_facts(repo, now, path)?),
        None => None,
    };
    Ok(Evidence { finding: Some(finding_facts(finding)), program, user_name })
}

pub fn finding_facts(finding: &Finding) -> FindingFacts {
    FindingFacts {
        kind: finding.kind.clone(),
        tier: finding.tier.as_str().to_owned(),
        title: finding.title.clone(),
        why: finding.why.clone(),
        evidence: finding.evidence.clone(),
        hits: finding.hits,
        level: finding.level,
    }
}

fn program_facts<R: EvidenceSource>(repo: &R, now: Timestamp, path: &str) -> Result<ProgramFacts, RepoError> {
    let view = program_history(repo, now, RANGE_SECS, path)?;
    let from = Timestamp::from_micros(now.as_micros() - i64::try_from(RANGE_SECS).unwrap_or(i64::MAX) * 1_000_000);
    let destinations = destination_facts(repo, &view.path, from, now)?;
    Ok(facts_from_view(&view, destinations.0, destinations.1))
}

fn facts_from_view(
    view: &ProgramHistoryView,
    destinations: Vec<DestinationFact>,
    destinations_total: usize,
) -> ProgramFacts {
    let identity = &view.identity;
    let badge = &identity.badge;
    let folder = view.path.rsplit_once(['\\', '/']).map_or("", |(folder, _)| folder).to_owned();
    ProgramFacts {
        file_name: view.name.clone(),
        folder,
        location: badge.location.clone(),
        signature: badge.signature.clone(),
        signer: identity.signer.clone(),
        publisher: badge.publisher.clone(),
        product: badge.product.clone(),
        description: identity.description.clone(),
        file_version: identity.file_version.clone(),
        sha256: identity.sha256.clone(),
        downloaded: identity.downloaded,
        runs_as: identity
            .accounts
            .iter()
            .map(|account| AccountFact { kind: account.kind.clone(), name: account.name.clone() })
            .collect(),
        service: badge.service,
        starts_at_boot: badge.starts_at_boot,
        launchers: identity.launchers.iter().filter(|launcher| launcher.current).map(|l| l.kind.clone()).collect(),
        registered_as: badge.registered_as.clone(),
        standing: Some(standing_words(&view.standing.state, view.standing.label.as_deref())),
        first_seen_days_ago: view
            .first_seen_ms
            .map(|first| (view.to_ms - first).max(0) / DAY_MS)
            .and_then(|d| u64::try_from(d).ok()),
        last_seen_hours_ago: view
            .last_seen_ms
            .map(|last| (view.to_ms - last).max(0) / HOUR_MS)
            .and_then(|h| u64::try_from(h).ok()),
        history: history_facts(view),
        destinations,
        destinations_total,
    }
}

fn standing_words(state: &str, label: Option<&str>) -> String {
    match (state, label) {
        ("known", Some(label)) => format!("on your known list as {label}"),
        ("lookalike", Some(label)) => format!("looks like your known program {label} but is not it"),
        _ => "not on your known list".to_owned(),
    }
}

/// Hourly numbers over the range. The average is weighted by the samples behind each hour, as the rankings are.
fn history_facts(view: &ProgramHistoryView) -> Option<HistoryFacts> {
    if view.hours.is_empty() {
        return None;
    }
    let samples: f64 = view.hours.iter().map(|hour| f64::from(hour.samples)).sum();
    let weighted: f64 = view.hours.iter().map(|hour| hour.cpu_avg_percent * f64::from(hour.samples)).sum();
    Some(HistoryFacts {
        hours_covered: u32::try_from(view.hours.len()).unwrap_or(u32::MAX),
        hours_present: u32::try_from(view.hours.iter().filter(|hour| hour.present).count()).unwrap_or(u32::MAX),
        cpu_avg_percent: if samples > 0.0 { weighted / samples } else { 0.0 },
        cpu_peak_percent: view.hours.iter().map(|hour| hour.cpu_peak_percent).fold(0.0, f64::max),
        gpu_peak_percent: view.hours.iter().map(|hour| hour.gpu_peak_percent).fold(0.0, f64::max),
        memory_peak_mb: view.hours.iter().map(|hour| hour.memory_peak_bytes).max().unwrap_or(0) / (1024 * 1024),
        starts: view.hours.iter().map(|hour| hour.starts).sum(),
    })
}

/// The busiest destinations first (up to `MAX_DESTINATIONS`), with the owner and name for the busiest few, and how many there were in all.
fn destination_facts<R: EvidenceSource>(
    repo: &R,
    path: &str,
    from: Timestamp,
    to: Timestamp,
) -> Result<(Vec<DestinationFact>, usize), RepoError> {
    let mut rows = repo.program_destinations(path, from, to)?;
    let total = rows.len();
    rows.sort_by(|a, b| b.bytes().cmp(&a.bytes()).then(a.addr.cmp(&b.addr)));
    rows.truncate(MAX_DESTINATIONS);
    let mut facts = Vec::with_capacity(rows.len());
    for (index, row) in rows.into_iter().enumerate() {
        let internet = classify_addr(row.addr).is_internet();
        let (owner, name) =
            if index < NAMED_DESTINATIONS { owner_and_name(repo, row.addr, internet)? } else { (None, None) };
        facts.push(DestinationFact {
            owner,
            name,
            addr: row.addr.to_string(),
            internet,
            bytes_sent: row.bytes_sent,
            bytes_received: row.bytes_received,
        });
    }
    Ok((facts, total))
}

fn owner_and_name<R: EvidenceSource>(
    repo: &R,
    addr: IpAddr,
    internet: bool,
) -> Result<(Option<String>, Option<String>), RepoError> {
    // Only addresses on the Internet have a registered owner; the name is what some program asked for when it reached the address.
    let owner = if internet { repo.lookup_owner(addr)?.map(|owner| owner.org) } else { None };
    let name = repo.address_names(addr)?.into_iter().next().map(|asked| asked.name);
    Ok((owner, name))
}
