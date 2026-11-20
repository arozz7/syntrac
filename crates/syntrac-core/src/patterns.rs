//! The pattern pack (docs/PATTERNS.md, docs/ALERTS-PLAN.md F5): the known patterns of rogue software SYNTRAC ships with, as data.
//!
//! A pattern is a small entry with an id, a title, a technique id (a label only), a tier, a plain "why", the likely harmless
//! explanation, and conditions chosen from a CLOSED list. There are no regular expressions over user data and no code in a pack, so
//! a pattern cannot be made slow or hostile; a word outside the list stops the pack loading (the shipped pack is parsed by a test,
//! so such a mistake fails the build). The matching lives in `pattern_run`.

use serde::Deserialize;

use crate::exe_identity::SignatureStatus;
use crate::exe_location::LocationClass;
use crate::findings::Tier;
use crate::launchers::LauncherKind;

/// The pack shipped inside the program: nothing is fetched at run time.
pub const BUILT_IN_PACK: &str = include_str!("../../../config/patterns/rogue-software.toml");

const MAX_LIST: usize = 100;
const MAX_WORD: usize = 80;

/// What a pattern is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// A program that has been seen running.
    Program,
    /// An entry that starts a program (a service, a task, a Run key, a Startup file).
    StartEntry,
    /// A program started by another program (the parent is named by `parent_is`, the started program by the other conditions).
    ProcessStart,
}

/// How much GPU, for how many hours in a row: a share (1 to 100) of the program's busiest GPU engine, not of the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SustainedGpu {
    pub percent: u32,
    pub hours: u32,
}

/// The most hours a `sustained_gpu` condition may ask for (the detector reads this many plus a few).
pub const MAX_SUSTAINED_HOURS: u32 = 24;

/// The conditions of a pattern; every one that is present must hold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct When {
    /// The file name equals one of these (lower case).
    pub name_is: Vec<String>,
    /// The file name, without its extension, is exactly one edit away from one of these and equal to none of them.
    pub name_near: Vec<String>,
    pub location_is: Vec<LocationClass>,
    pub location_not: Vec<LocationClass>,
    pub signature_is: Vec<SignatureStatus>,
    /// The signer text contains one of these words (lower case).
    pub signer_has: Vec<String>,
    /// The signer text contains none of these words (lower case); needs the file to have been read.
    pub signer_lacks: Vec<String>,
    pub downloaded: bool,
    /// First seen before detection was switched on (what came after is the business of the new-thing rules).
    pub already_here: bool,
    pub double_extension: bool,
    pub direction_override: bool,
    pub entry_kinds: Vec<LauncherKind>,
    /// (started programs only) the file name of the PARENT equals one of these (lower case).
    pub parent_is: Vec<String>,
    /// The program used the network for the first time after detection was switched on (it had no earlier sighting at all).
    pub first_network_use: bool,
    /// (programs only) the program's hourly GPU average was at least this in EACH of the last completed hours (consecutive; a missing hour counts as zero).
    pub sustained_gpu: Option<SustainedGpu>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub id: String,
    pub title: String,
    /// A public technique id (MITRE ATT&CK), shown as a label.
    pub technique: String,
    /// `Unusual` or `Suspicious`: a pattern is never a verdict.
    pub tier: Tier,
    pub subject: Subject,
    pub why: String,
    /// The likely harmless explanation, shown with every match.
    pub benign: String,
    /// Lift the tier to suspicious when the program is unsigned or untrusted in a place users can write.
    pub raise_if_concerning: bool,
    pub when: When,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternPack {
    pub version: String,
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PackError(pub String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPack {
    version: String,
    pattern: Vec<RawPattern>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPattern {
    id: String,
    title: String,
    technique: String,
    tier: String,
    applies_to: String,
    why: String,
    benign: String,
    #[serde(default)]
    raise_if_concerning: bool,
    when: RawWhen,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawWhen {
    name_is: Option<Vec<String>>,
    name_near: Option<Vec<String>>,
    location_is: Option<Vec<String>>,
    location_not: Option<Vec<String>>,
    signature_is: Option<Vec<String>>,
    signer_has: Option<Vec<String>>,
    signer_lacks: Option<Vec<String>>,
    downloaded: Option<bool>,
    already_here: Option<bool>,
    double_extension: Option<bool>,
    direction_override: Option<bool>,
    entry_kind_is: Option<Vec<String>>,
    parent_is: Option<Vec<String>>,
    first_network_use: Option<bool>,
    sustained_gpu: Option<RawSustainedGpu>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSustainedGpu {
    percent: u32,
    hours: u32,
}

fn sustained_gpu(id: &str, raw: Option<RawSustainedGpu>) -> Result<Option<SustainedGpu>, PackError> {
    let Some(raw) = raw else { return Ok(None) };
    if !(1..=100).contains(&raw.percent) || !(1..=MAX_SUSTAINED_HOURS).contains(&raw.hours) {
        return Err(PackError(format!(
            "{id}: sustained_gpu needs a percent from 1 to 100 and hours from 1 to {MAX_SUSTAINED_HOURS}"
        )));
    }
    Ok(Some(SustainedGpu { percent: raw.percent, hours: raw.hours }))
}

fn words(id: &str, field: &str, list: Option<Vec<String>>) -> Result<Vec<String>, PackError> {
    let Some(list) = list else { return Ok(Vec::new()) };
    if list.is_empty() || list.len() > MAX_LIST {
        return Err(PackError(format!("{id}: {field} must have between 1 and {MAX_LIST} entries")));
    }
    list.iter()
        .map(|word| {
            let word = word.trim().to_lowercase();
            if word.is_empty() || word.chars().count() > MAX_WORD || word.chars().any(char::is_control) {
                Err(PackError(format!("{id}: {field} has an entry that is empty, too long or has control characters")))
            } else {
                Ok(word)
            }
        })
        .collect()
}

fn named<T>(
    id: &str,
    field: &str,
    list: Option<Vec<String>>,
    parse: fn(&str) -> Option<T>,
) -> Result<Vec<T>, PackError> {
    words(id, field, list)?
        .iter()
        .map(|word| parse(word).ok_or_else(|| PackError(format!("{id}: {field} does not know \"{word}\""))))
        .collect()
}

fn only_true(id: &str, field: &str, flag: Option<bool>) -> Result<bool, PackError> {
    match flag {
        None => Ok(false),
        Some(true) => Ok(true),
        Some(false) => Err(PackError(format!("{id}: {field} can only be true (leave it out otherwise)"))),
    }
}

fn is_technique(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('T') else { return false };
    let (main, sub) = rest.split_once('.').map_or((rest, None), |(main, sub)| (main, Some(sub)));
    let digits = |part: &str, n: usize| part.len() == n && part.chars().all(|c| c.is_ascii_digit());
    digits(main, 4) && sub.is_none_or(|sub| digits(sub, 3))
}

fn text(id: &str, field: &str, value: &str, max: usize) -> Result<String, PackError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(PackError(format!("{id}: {field} must be 1 to {max} characters of plain text")));
    }
    Ok(value.to_owned())
}

fn when_of(id: &str, raw: RawWhen) -> Result<When, PackError> {
    Ok(When {
        name_is: words(id, "name_is", raw.name_is)?,
        name_near: words(id, "name_near", raw.name_near)?,
        location_is: named(id, "location_is", raw.location_is, LocationClass::parse)?,
        location_not: named(id, "location_not", raw.location_not, LocationClass::parse)?,
        signature_is: named(id, "signature_is", raw.signature_is, SignatureStatus::parse)?,
        signer_has: words(id, "signer_has", raw.signer_has)?,
        signer_lacks: words(id, "signer_lacks", raw.signer_lacks)?,
        downloaded: only_true(id, "downloaded", raw.downloaded)?,
        already_here: only_true(id, "already_here", raw.already_here)?,
        double_extension: only_true(id, "double_extension", raw.double_extension)?,
        direction_override: only_true(id, "direction_override", raw.direction_override)?,
        entry_kinds: named(id, "entry_kind_is", raw.entry_kind_is, LauncherKind::parse)?,
        parent_is: words(id, "parent_is", raw.parent_is)?,
        first_network_use: only_true(id, "first_network_use", raw.first_network_use)?,
        sustained_gpu: sustained_gpu(id, raw.sustained_gpu)?,
    })
}

fn is_empty(when: &When) -> bool {
    *when == When::default()
}

fn pattern_of(raw: RawPattern) -> Result<Pattern, PackError> {
    let id = raw.id.trim().to_owned();
    if id.is_empty() || id.len() > 40 || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(PackError(format!(
            "\"{id}\" is not a valid pattern id (lower-case letters, digits and _, at most 40)"
        )));
    }
    let tier = match Tier::parse(raw.tier.trim()) {
        Some(Tier::Confirmed) => {
            return Err(PackError(format!(
                "{id}: a pattern can be unusual or suspicious; only an imported list confirms"
            )));
        }
        Some(tier) => tier,
        None => return Err(PackError(format!("{id}: unknown tier \"{}\"", raw.tier))),
    };
    let subject = match raw.applies_to.trim() {
        "program" => Subject::Program,
        "start_entry" => Subject::StartEntry,
        "process_start" => Subject::ProcessStart,
        other => {
            return Err(PackError(format!(
                "{id}: applies_to must be program, start_entry or process_start, not \"{other}\""
            )));
        }
    };
    if !is_technique(raw.technique.trim()) {
        return Err(PackError(format!("{id}: technique must look like T1036 or T1036.005")));
    }
    let when = when_of(&id, raw.when)?;
    if is_empty(&when) {
        return Err(PackError(format!("{id}: a pattern needs at least one condition")));
    }
    if !when.entry_kinds.is_empty() && subject != Subject::StartEntry {
        return Err(PackError(format!("{id}: entry_kind_is only makes sense for start entries")));
    }
    if !when.parent_is.is_empty() && subject != Subject::ProcessStart {
        return Err(PackError(format!("{id}: parent_is only makes sense for patterns about a started program")));
    }
    if subject == Subject::ProcessStart && (when.parent_is.is_empty() || when.name_is.is_empty()) {
        return Err(PackError(format!(
            "{id}: a started-by pattern names its parents (parent_is) and the programs it cares about (name_is)"
        )));
    }
    if subject == Subject::ProcessStart && when.already_here {
        return Err(PackError(format!("{id}: already_here makes no sense for a started program")));
    }
    if when.first_network_use && subject != Subject::Program {
        return Err(PackError(format!("{id}: first_network_use only makes sense for a program")));
    }
    if when.sustained_gpu.is_some() && subject != Subject::Program {
        return Err(PackError(format!("{id}: sustained_gpu only makes sense for a program")));
    }
    Ok(Pattern {
        title: text(&id, "title", &raw.title, 80)?,
        technique: raw.technique.trim().to_owned(),
        why: text(&id, "why", &raw.why, 250)?,
        benign: text(&id, "benign", &raw.benign, 200)?,
        raise_if_concerning: raw.raise_if_concerning,
        tier,
        subject,
        when,
        id,
    })
}

/// Reads a pack. Anything outside the closed vocabulary, a duplicate id, a tier of "confirmed", an empty condition list: refused.
pub fn parse_pack(source: &str) -> Result<PatternPack, PackError> {
    let raw: RawPack =
        toml::from_str(source).map_err(|error| PackError(format!("the pattern pack is not valid: {error}")))?;
    let version = text("version", "version", &raw.version, 40)?;
    let mut patterns: Vec<Pattern> = Vec::new();
    for item in raw.pattern {
        let pattern = pattern_of(item)?;
        if patterns.iter().any(|other| other.id == pattern.id) {
            return Err(PackError(format!("the pattern id {} is used twice", pattern.id)));
        }
        patterns.push(pattern);
    }
    Ok(PatternPack { version, patterns })
}

impl PatternPack {
    /// The file names of the programs the started-by patterns care about (lower case, once each): what the process read asks for.
    pub fn started_program_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .patterns
            .iter()
            .filter(|pattern| pattern.subject == Subject::ProcessStart)
            .flat_map(|pattern| pattern.when.name_is.iter().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Whether any pattern needs the hourly GPU rows (they are only read when one does).
    pub fn uses_sustained_gpu(&self) -> bool {
        self.patterns.iter().any(|pattern| pattern.when.sustained_gpu.is_some())
    }

    pub fn uses_first_network_use(&self) -> bool {
        self.patterns.iter().any(|pattern| pattern.when.first_network_use)
    }
}

/// The shipped pack. A mistake in it fails the test that parses it, so reaching a user with a broken pack is not possible.
pub fn built_in_pack() -> PatternPack {
    parse_pack(BUILT_IN_PACK).expect("the shipped pattern pack is valid (a test parses it)")
}
