//! Matching the pattern pack against what SYNTRAC knows (docs/ALERTS-PLAN.md, F5a). Pure: facts in, candidates out; the detector at
//! the bottom only loads the facts through read ports.
//!
//! Which programs are judged: only programs seen running in the last `RECENT_DAYS` (a program that ran once last year and was
//! uninstalled is not a finding today), never a program a known product covers, and never one whose file has not been read when the
//! pattern needs the file. Each pattern is one kind of finding (`pattern_<id>`), the subject is the program (or, for a start entry,
//! the program and the entry), and the pack version is NOT part of the key, so updating the pack does not raise everything again.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::destination_trend::DestinationTrendRepository;
use crate::exe_identity::{FileFacts, IdentityEntry, IdentityRepository};
use crate::exe_location::{classify_location, LocationClass};
use crate::findings::{Candidate, Tier};
use crate::findings_run::{DetectError, Detector};
use crate::known::{KnownIndex, KnownProduct, KnownProductRepository, Standing};
use crate::launchers::{LauncherRepository, LauncherSeen};
use crate::new_things::{concerns, location_label, signature_line};
use crate::patterns::{Pattern, PatternPack, Subject, SustainedGpu, When, MAX_SUSTAINED_HOURS};
use crate::ports::RepoError;
use crate::rollup::Executable;
use crate::spawns::{SpawnPair, SpawnRepository};
use crate::time::Timestamp;
use crate::usage::{ProcessHourRow, UsageRepository, STALE_AFTER};

/// Only programs seen running within this many days are judged.
pub const RECENT_DAYS: i64 = 30;

/// At most this many findings of one pattern per run; the most serious stay.
pub const MAX_PER_PATTERN: usize = 50;

const DAY_MICROS: i64 = 86_400 * 1_000_000;
const HOUR_MICROS: i64 = 3_600 * 1_000_000;
const MIN_NEAR_LEN: usize = 5;
const PROGRAM_EXTENSIONS: [&str; 10] = ["exe", "scr", "com", "bat", "cmd", "pif", "js", "vbs", "msi", "jar"];
const DOCUMENT_EXTENSIONS: [&str; 20] = [
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "jpg", "jpeg", "png", "gif", "bmp", "mp3",
    "mp4", "avi", "zip", "rar",
];
/// Text-direction controls: U+202A to U+202E and U+2066 to U+2069.
const DIRECTION_CONTROLS: [char; 9] =
    ['\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'];

/// Everything the patterns look at.
pub struct PatternFacts<'a> {
    /// When detection was first switched on.
    pub since: Timestamp,
    pub now: Timestamp,
    pub recent_days: i64,
    pub known: &'a [KnownProduct],
    pub entries: &'a [IdentityEntry],
    pub launchers: &'a [(Executable, Vec<LauncherSeen>)],
    /// Programs that used the network for the first time after the switch-on.
    pub first_online: &'a [Executable],
    /// Which program started which, from the raw process rows (the detail retention).
    pub spawns: &'a [SpawnPair],
    /// The programs' hourly GPU rows of the last completed hours (read only when a pattern asks for them).
    pub gpu_hours: &'a [ProcessHourRow],
}

/// The number of single-character edits (change, add, remove, or swap two neighbours) between two names: optimal string
/// alignment. A swap counts as ONE edit, so `scvhost` is one edit from `svchost`.
pub fn osa_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    rows[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1).min(rows[i][j - 1] + 1).min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

/// What a pattern is judged on: a program, and for a start entry the entry, and for a started program the start.
struct Target<'a> {
    exe: &'a Executable,
    /// The file name as written.
    name: &'a str,
    facts: Option<&'a FileFacts>,
    class: LocationClass,
    already_here: bool,
    entry: Option<&'a LauncherSeen>,
    spawn: Option<&'a SpawnPair>,
    first_online: bool,
    /// The program's hourly GPU averages (hour start in microseconds), and the newest completed hour of all the data when it is
    /// fresh enough to mean "the last hours" (`None` when the rollups have stopped).
    gpu: Option<&'a BTreeMap<i64, f64>>,
    gpu_latest: Option<i64>,
}

fn facts_of(entry: &IdentityEntry) -> Option<&FileFacts> {
    entry.record.as_ref().and_then(|record| record.facts.as_ref())
}

fn file_name(path: &str) -> &str {
    path.trim().rsplit(['\\', '/']).next().unwrap_or_default()
}

fn stem_of(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, "exe" | "scr" | "com")) => stem,
        _ => name,
    }
}

/// One edit away from a listed program name, and equal to none of them. Names shorter than five letters are not judged (on either
/// side): too many ordinary words are one edit from each other.
fn near_a_name(name: &str, listed: &[String]) -> bool {
    let stem = stem_of(name);
    let stems: Vec<&str> = listed.iter().map(|listed| stem_of(listed)).collect();
    if stems.contains(&stem) {
        return false;
    }
    stem.chars().count() >= MIN_NEAR_LEN
        && stems.iter().any(|other| other.chars().count() >= MIN_NEAR_LEN && osa_distance(stem, other) == 1)
}

fn has_double_extension(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    let [.., inner, outer] = parts.as_slice() else { return false };
    parts.len() >= 3 && PROGRAM_EXTENSIONS.contains(outer) && DOCUMENT_EXTENSIONS.contains(inner)
}

fn direction_control(name: &str) -> Option<char> {
    name.chars().find(|c| DIRECTION_CONTROLS.contains(c))
}

/// The program's hourly GPU averages in each of the last `wanted.hours` completed hours (newest first), when EVERY one of them is at
/// least `wanted.percent`. A missing hour counts as zero, so the hours must be consecutive; rollups that have stopped match nothing.
fn sustained_gpu_hours(target: &Target<'_>, wanted: &SustainedGpu) -> Option<Vec<f64>> {
    let (series, latest) = (target.gpu?, target.gpu_latest?);
    (0..i64::from(wanted.hours))
        .map(|back| series.get(&(latest - back * HOUR_MICROS)).copied().filter(|gpu| *gpu >= f64::from(wanted.percent)))
        .collect()
}

fn holds(when: &When, target: &Target<'_>) -> bool {
    let lower = target.name.to_lowercase();
    let located = target.class != LocationClass::Unknown;
    let signer = target.facts.and_then(|f| f.signature.signer.as_deref()).map(str::to_lowercase);
    (when.name_is.is_empty() || when.name_is.contains(&lower))
        && (when.name_near.is_empty() || near_a_name(&lower, &when.name_near))
        && (when.location_is.is_empty() || (located && when.location_is.contains(&target.class)))
        && (when.location_not.is_empty() || (located && !when.location_not.contains(&target.class)))
        && (when.signature_is.is_empty()
            || target.facts.is_some_and(|f| when.signature_is.contains(&f.signature.status)))
        && (when.signer_has.is_empty()
            || signer.as_deref().is_some_and(|s| when.signer_has.iter().any(|word| s.contains(word.as_str()))))
        && (when.signer_lacks.is_empty()
            || (target.facts.is_some()
                && !signer.as_deref().is_some_and(|s| when.signer_lacks.iter().any(|word| s.contains(word.as_str())))))
        && (!when.downloaded || target.facts.is_some_and(FileFacts::downloaded))
        && (!when.already_here || target.already_here)
        && (!when.double_extension || has_double_extension(&lower))
        && (!when.direction_override || direction_control(target.name).is_some())
        && (when.entry_kinds.is_empty() || target.entry.is_some_and(|entry| when.entry_kinds.contains(&entry.kind)))
        && (when.parent_is.is_empty()
            || target.spawn.is_some_and(|spawn| when.parent_is.contains(&file_name(&spawn.parent.path).to_lowercase())))
        && (!when.first_network_use || target.first_online)
        && when.sustained_gpu.is_none_or(|wanted| sustained_gpu_hours(target, &wanted).is_some())
}

fn candidate(pattern: &Pattern, pack: &PatternPack, target: &Target<'_>, now: Timestamp) -> Candidate {
    let name = target.name.to_owned();
    let (subject, mut evidence) = match (target.entry, target.spawn) {
        (_, Some(spawn)) => (
            format!("{}|{}", spawn.parent.key, spawn.child.key),
            vec![
                format!("Parent: {}", spawn.parent.path),
                format!("Program: {}", spawn.child.path),
                started_line(spawn, now),
            ],
        ),
        (None, None) => (target.exe.key.clone(), vec![format!("Path: {}", target.exe.path)]),
        (Some(entry), None) => (
            format!("{}|{}|{}", target.exe.key, entry.kind.as_str(), entry.location.to_lowercase()),
            vec![
                format!("Program: {}", target.exe.path),
                format!("Entry: {} ({})", entry.kind.as_str().replace('_', " "), entry.location),
            ],
        ),
    };
    evidence.push(format!("Location: {}", location_label(target.class)));
    evidence.push(signature_line(target.facts));
    if let Some(wanted) = pattern.when.sustained_gpu {
        if let Some(hours) = sustained_gpu_hours(target, &wanted) {
            let list: Vec<String> = hours.iter().map(|gpu| format!("{gpu:.1}%")).collect();
            evidence.push(format!(
                "GPU use in each of the last {} hours (newest first): {}; the limit is {}% of one GPU engine (a share of its busiest engine, not of the machine)",
                wanted.hours,
                list.join(", "),
                wanted.percent
            ));
        }
    }
    if let Some(control) = direction_control(target.name) {
        evidence.push(format!("Its name contains the text-direction control character U+{:04X}", u32::from(control)));
    }
    evidence
        .push(format!("Pattern: {} ({}), a public technique name used as a label", pattern.title, pattern.technique));
    evidence.push(format!("Often harmless: {}", pattern.benign));
    evidence.push(format!("Pattern pack {}", pack.version));
    let raised = pattern.raise_if_concerning && !concerns(target.facts, target.class).is_empty();
    Candidate {
        kind: format!("pattern_{}", pattern.id),
        dedupe_key: format!("pattern_{}:{subject}", pattern.id),
        subject,
        subject_label: name.clone(),
        tier: if raised { Tier::Suspicious } else { pattern.tier },
        title: match target.spawn {
            Some(spawn) => format!("{}: {} -> {name}", pattern.title, file_name(&spawn.parent.path)),
            None => format!("{}: {name}", pattern.title),
        },
        why: pattern.why.clone(),
        evidence,
        level: None,
        at: now,
    }
}

/// How often and how recently the parent started the program, in words (command lines are not stored, so none is claimed).
fn started_line(spawn: &SpawnPair, now: Timestamp) -> String {
    let days = ((now.as_micros() - spawn.last.as_micros()) / DAY_MICROS).max(0);
    let when = match days {
        0 => "today".to_owned(),
        1 => "1 day ago".to_owned(),
        n => format!("{n} days ago"),
    };
    format!("Started {} time(s) in the days SYNTRAC keeps process records, the last {when}", spawn.count)
}

/// Every candidate of every pattern in the pack.
pub fn pattern_candidates(pack: &PatternPack, facts: &PatternFacts<'_>) -> Vec<Candidate> {
    let recent_since = Timestamp::from_micros(facts.now.as_micros() - facts.recent_days * DAY_MICROS);
    let index =
        KnownIndex::build(facts.known.to_vec(), facts.entries.iter().map(|e| (e.exe.path.as_str(), facts_of(e))));
    let is_known = |path: &str, file: Option<&FileFacts>| matches!(index.standing(path, file), Standing::Known { .. });
    let by_key: HashMap<&str, &IdentityEntry> = facts.entries.iter().map(|e| (e.exe.key.as_str(), e)).collect();

    let online: HashSet<&str> = facts.first_online.iter().map(|exe| exe.key.as_str()).collect();

    // "The last hours" only means something while the rollups keep up: older than the usage rule's limit, nothing matches.
    let gpu_latest = facts
        .gpu_hours
        .iter()
        .map(|row| row.hour.as_micros())
        .max()
        .filter(|latest| facts.now.as_micros() - latest <= STALE_AFTER);
    let mut gpu_series: HashMap<&str, BTreeMap<i64, f64>> = HashMap::new();
    for row in facts.gpu_hours {
        gpu_series.entry(row.exe.key.as_str()).or_default().insert(row.hour.as_micros(), row.gpu_avg);
    }

    let mut targets: Vec<(Subject, Target<'_>)> = Vec::new();
    for entry in facts.entries {
        let Some(seen) = entry.seen.filter(|seen| seen.last_seen >= recent_since) else { continue };
        let file = facts_of(entry);
        if is_known(&entry.exe.path, file) {
            continue;
        }
        targets.push((
            Subject::Program,
            Target {
                exe: &entry.exe,
                name: file_name(&entry.exe.path),
                facts: file,
                class: classify_location(&entry.exe.path),
                already_here: seen.first_seen < facts.since,
                entry: None,
                spawn: None,
                first_online: online.contains(entry.exe.key.as_str()),
                gpu: gpu_series.get(entry.exe.key.as_str()),
                gpu_latest,
            },
        ));
    }
    for (exe, entries) in facts.launchers {
        let file = by_key.get(exe.key.as_str()).copied().and_then(facts_of);
        if is_known(&exe.path, file) {
            continue;
        }
        for launcher in entries.iter().filter(|e| e.current && e.last_seen >= recent_since) {
            targets.push((
                Subject::StartEntry,
                Target {
                    exe,
                    name: file_name(&exe.path),
                    facts: file,
                    class: classify_location(&exe.path),
                    already_here: launcher.first_seen < facts.since,
                    entry: Some(launcher),
                    spawn: None,
                    first_online: false,
                    gpu: None,
                    gpu_latest: None,
                },
            ));
        }
    }
    for spawn in facts.spawns.iter().filter(|spawn| spawn.last >= recent_since) {
        targets.push((
            Subject::ProcessStart,
            Target {
                exe: &spawn.child,
                name: file_name(&spawn.child.path),
                facts: by_key.get(spawn.child.key.as_str()).copied().and_then(facts_of),
                class: classify_location(&spawn.child.path),
                already_here: false,
                entry: None,
                spawn: Some(spawn),
                first_online: false,
                gpu: None,
                gpu_latest: None,
            },
        ));
    }

    let mut all = Vec::new();
    for pattern in &pack.patterns {
        let mut found: Vec<Candidate> = targets
            .iter()
            .filter(|(subject, target)| *subject == pattern.subject && holds(&pattern.when, target))
            .map(|(_, target)| candidate(pattern, pack, target, facts.now))
            .collect();
        found.sort_by_key(|c| std::cmp::Reverse(c.tier));
        found.truncate(MAX_PER_PATTERN);
        all.extend(found);
    }
    all
}

fn failed(what: &'static str) -> impl FnOnce(RepoError) -> DetectError {
    move |error| DetectError(format!("could not read {what}: {error}"))
}

/// The patterns as a detector: loads the facts through read ports and applies `pattern_candidates`.
pub struct PatternDetector<R> {
    repo: R,
    pack: PatternPack,
}

impl<R> PatternDetector<R> {
    pub fn new(repo: R, pack: PatternPack) -> Self {
        Self { repo, pack }
    }
}

impl<R> Detector for PatternDetector<R>
where
    R: IdentityRepository
        + LauncherRepository
        + KnownProductRepository
        + DestinationTrendRepository
        + SpawnRepository
        + UsageRepository,
{
    fn name(&self) -> &'static str {
        "known patterns"
    }

    fn detect(&self, since: Timestamp, now: Timestamp) -> Result<Vec<Candidate>, DetectError> {
        let entries = self.repo.identity_entries().map_err(failed("the programs"))?;
        let known = self.repo.known_products().map_err(failed("the known programs"))?;
        let launchers = self.repo.all_launchers().map_err(failed("the start entries"))?;
        // Only what the pack asks about is read: the process rows are the big table.
        let first_online = if self.pack.uses_first_network_use() {
            self.repo.first_online_since(since).map_err(failed("the first network use"))?
        } else {
            Vec::new()
        };
        // The hourly rows are read only when a pattern asks for the GPU: the last hours any pattern may ask about, plus a few.
        let gpu_hours = if self.pack.uses_sustained_gpu() {
            let from = Timestamp::from_micros(now.as_micros() - (i64::from(MAX_SUSTAINED_HOURS) + 4) * HOUR_MICROS);
            self.repo.process_hour_rows(from, now).map_err(failed("the hourly GPU use"))?
        } else {
            Vec::new()
        };
        let child_names = self.pack.started_program_names();
        let recent_since = Timestamp::from_micros(now.as_micros() - RECENT_DAYS * DAY_MICROS);
        let spawns =
            self.repo.spawn_pairs(recent_since, &child_names).map_err(failed("which programs started which"))?;
        Ok(pattern_candidates(
            &self.pack,
            &PatternFacts {
                since,
                now,
                recent_days: RECENT_DAYS,
                known: &known,
                entries: &entries,
                launchers: &launchers,
                first_online: &first_online,
                spawns: &spawns,
                gpu_hours: &gpu_hours,
            },
        ))
    }
}
