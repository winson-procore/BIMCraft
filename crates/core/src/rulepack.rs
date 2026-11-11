//! The frontier-model rule pack seed library. Detail: `docs/areas/rule-packs.md`.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::Session;
use crate::generate::fnv1a64;

pub const ARCHETYPES: [&str; 5] = ["office", "clinic", "hospital", "datacenter", "airport"];

/// The generator's checks a soft rule may name: each is run for every generated plan.
pub const KNOWN_CHECKERS: [&str; 1] = ["GR-002"];

const SCHEMA_VERSION: &str = "rule-pack/1";
const LIBRARY_ID: &str = "frontier-model-seed-library";
const CREATED_BY: &str =
    "bimcraft rule-pack seed: distilled by an AI agent from stored Anthropic model proposals, not reviewed by a person";
const MANIFEST: &str = "rule-pack-manifest.json";
const REPORT: &str = "validation-report.json";
const PACK_FILES: [&str; 5] =
    ["packs/office.json", "packs/clinic.json", "packs/hospital.json", "packs/datacenter.json", "packs/airport.json"];
const MIN_RULES: [usize; 5] = [8, 8, 10, 10, 10];
const TOPICS: [[&str; 5]; 5] = [
    ["reception", "conference", "private", "restroom", "support"],
    ["waiting", "exam", "nurse", "clean", "dirty"],
    ["waiting", "exam", "nurse", "clean", "dirty"],
    ["mantrap", "data", "cooling", "electrical", "redund"],
    ["landside", "airside", "security", "baggage", "gate"],
];
const MIN_MODELS: usize = 3;
/// Percent of a pack's rules that must stay exploratory; the gate asks 80.
const MIN_EXPLORATORY_PERCENT: usize = 85;
const MAX_PATH: usize = 4096;

const SEEDS: [(&str, &str); 5] = [
    ("office", include_str!("../data/rule-packs/office.json")),
    ("clinic", include_str!("../data/rule-packs/clinic.json")),
    ("hospital", include_str!("../data/rule-packs/hospital.json")),
    ("datacenter", include_str!("../data/rule-packs/datacenter.json")),
    ("airport", include_str!("../data/rule-packs/airport.json")),
];
const PROPOSALS: [(&str, &str); 3] = [
    ("claude-haiku", include_str!("../data/rule-packs/proposals/claude-haiku.json")),
    ("claude-opus", include_str!("../data/rule-packs/proposals/claude-opus.json")),
    ("claude-sonnet", include_str!("../data/rule-packs/proposals/claude-sonnet.json")),
];
const PROMPTS: [&str; 2] = [include_str!("../data/rule-packs/seed-prompt.md"), include_str!("../data/rule-packs/seed-prompt-followup.md")];

/// Why the library was not made: a code for the kind of trouble, and what to change.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

/// What the library is built from: the curated seeds by archetype, the models' proposals by label, and the two prompts.
pub struct Inputs<'a> {
    pub seeds: &'a [(&'a str, &'a str)],
    pub proposals: &'a [(&'a str, &'a str)],
    pub prompts: [&'a str; 2],
}

/// The files of the library, in memory and checked: the packs, the validation report, then the manifest.
pub struct Library {
    pub files: Vec<(&'static str, String)>,
    pub packs: Vec<PackSummary>,
    pub checks: Vec<Check>,
}

pub struct PackSummary {
    pub archetype: &'static str,
    pub rules: usize,
    pub conflicts: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: &'static str,
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Category {
    Program,
    Access,
    Adjacency,
    Flow,
    Separation,
    Security,
    Systems,
    Dimensional,
    Equipment,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Checkability {
    Deterministic,
    Heuristic,
    HumanReview,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ConflictStatus {
    Open,
    ResolvedInPack,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedRule {
    id: String,
    category: Category,
    subject: String,
    predicate: String,
    object: String,
    condition: Option<String>,
    classification: String,
    checkability: Checkability,
    confidence: f64,
    rationale: String,
    exceptions: Vec<String>,
    source_proposals: Vec<String>,
    deterministic_checker: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Position {
    proposal: String,
    position: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Conflict {
    id: String,
    topic: String,
    rule_ids: Vec<String>,
    positions: Vec<Position>,
    status: ConflictStatus,
    resolution: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    archetype: String,
    rules: Vec<SeedRule>,
    conflicts: Vec<Conflict>,
}

/// A model's own proposal as it was given: only its key and its confidence are used, the rest is read so a malformed file is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Proposal {
    key: String,
    category: String,
    subject: String,
    predicate: String,
    object: String,
    condition: Option<String>,
    checkability: String,
    confidence: f64,
    rationale: String,
    exceptions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalFile {
    archetypes: BTreeMap<String, Vec<Proposal>>,
}

struct Loaded {
    /// In `ARCHETYPES` order.
    seeds: Vec<Seed>,
    /// By label, sorted: each model's proposals by archetype.
    models: Vec<(String, BTreeMap<String, Vec<Proposal>>)>,
    /// `label#archetype/key` to the proposal's confidence.
    refs: BTreeMap<String, f64>,
}

fn invalid(label: &str, why: impl fmt::Display) -> Refusal {
    refuse("invalid_seed", format!("{label}: {why}"))
}

fn load(inputs: &Inputs) -> Result<Loaded, Refusal> {
    let mut models: Vec<(String, BTreeMap<String, Vec<Proposal>>)> = Vec::new();
    let mut refs = BTreeMap::new();
    for (label, text) in inputs.proposals {
        let file: ProposalFile = serde_json::from_str(text).map_err(|e| invalid(label, e))?;
        if models.iter().any(|(l, _)| l == label) {
            return Err(invalid(label, "two proposal files carry this label"));
        }
        for (arch, list) in &file.archetypes {
            if !ARCHETYPES.contains(&arch.as_str()) {
                return Err(invalid(label, format!("proposes for {arch:?}, which is not one of {}", ARCHETYPES.join(", "))));
            }
            for p in list {
                let key = format!("{label}#{arch}/{}", p.key);
                if refs.insert(key.clone(), p.confidence).is_some() {
                    return Err(invalid(label, format!("{key} is proposed twice")));
                }
            }
        }
        models.push((label.to_string(), file.archetypes));
    }
    models.sort_by(|a, b| a.0.cmp(&b.0));

    for (arch, _) in inputs.seeds {
        if !ARCHETYPES.contains(arch) {
            return Err(invalid(arch, format!("not an archetype of the library ({})", ARCHETYPES.join(", "))));
        }
    }
    let mut seeds = Vec::new();
    for arch in ARCHETYPES {
        let mut given = inputs.seeds.iter().filter(|(a, _)| *a == arch);
        let Some((_, text)) = given.next() else {
            return Err(invalid(arch, "no seed file: the library holds one for each of its five archetypes"));
        };
        if given.next().is_some() {
            return Err(invalid(arch, "two seed files for one archetype"));
        }
        let seed: Seed = serde_json::from_str(text).map_err(|e| invalid(arch, e))?;
        if seed.archetype != arch {
            return Err(invalid(arch, format!("the file says its archetype is {:?}", seed.archetype)));
        }
        seeds.push(seed);
    }
    Ok(Loaded { seeds, models, refs })
}

impl Loaded {
    /// The models that proposed something for this archetype.
    fn consulted(&self, arch: &str) -> Vec<&str> {
        self.models.iter().filter(|(_, m)| m.get(arch).is_some_and(|v| !v.is_empty())).map(|(l, _)| l.as_str()).collect()
    }

    fn proposals_for(&self, arch: &str) -> Vec<String> {
        self.refs.keys().filter(|k| k.contains(&format!("#{arch}/"))).cloned().collect()
    }
}

fn verdict(id: &'static str, name: &'static str, problems: &[String], pass: String) -> Check {
    match problems.split_first() {
        None => Check { id, name, status: Status::Pass, detail: pass },
        Some((first, [])) => Check { id, name, status: Status::Fail, detail: first.clone() },
        Some((first, rest)) => Check { id, name, status: Status::Fail, detail: format!("{first} (and {} more)", rest.len()) },
    }
}

fn warning(id: &'static str, name: &'static str, notes: &[String], pass: String) -> Check {
    match notes.split_first() {
        None => Check { id, name, status: Status::Pass, detail: pass },
        Some((first, [])) => Check { id, name, status: Status::Warn, detail: first.clone() },
        Some((first, rest)) => Check { id, name, status: Status::Warn, detail: format!("{first} (and {} more)", rest.len()) },
    }
}

fn id_form(id: &str) -> bool {
    id.len() >= 2
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

fn short(text: &str) -> bool {
    text.trim().chars().count() < 3
}

fn pack_text(s: &Seed) -> String {
    let mut text = String::new();
    for r in &s.rules {
        for part in [&r.subject, &r.predicate, &r.object, &r.rationale] {
            text.push_str(part);
            text.push(' ');
        }
        text.push_str(r.condition.as_deref().unwrap_or_default());
        text.push(' ');
        text.push_str(&r.exceptions.join(" "));
        text.push(' ');
    }
    for c in &s.conflicts {
        text.push_str(&c.topic);
        text.push(' ');
        text.push_str(&c.resolution);
        for p in &c.positions {
            text.push(' ');
            text.push_str(&p.position);
        }
    }
    text.to_lowercase()
}

fn is_soft(r: &SeedRule) -> bool {
    r.classification == "soft"
}

fn checks(l: &Loaded) -> Vec<Check> {
    let total: usize = l.seeds.iter().map(|s| s.rules.len()).sum();
    let mut out = Vec::new();

    out.push(verdict(
        "RP-001",
        "inputs-parse-strictly",
        &[],
        format!(
            "{} seed files and {} proposal files read with no unknown key, and each seed names its archetype",
            l.seeds.len(),
            l.models.len()
        ),
    ));

    let mut problems = Vec::new();
    for arch in ARCHETYPES {
        let n = l.consulted(arch).len();
        if n < MIN_MODELS {
            problems.push(format!("{arch}: {n} model(s) proposed rules for it; a seed from {MIN_MODELS} models needs three"));
        }
    }
    out.push(verdict(
        "RP-002",
        "three-models-per-pack",
        &problems,
        format!("every archetype has proposals from {} models", l.models.len()),
    ));

    let mut problems = Vec::new();
    for (i, s) in l.seeds.iter().enumerate() {
        if s.rules.len() < MIN_RULES[i] {
            problems.push(format!("{} has {} rules; at least {} are needed", s.archetype, s.rules.len(), MIN_RULES[i]));
        }
        let text = pack_text(s);
        for term in TOPICS[i] {
            if !text.contains(term) {
                problems.push(format!("{} says nothing of {term}", s.archetype));
            }
        }
    }
    out.push(verdict(
        "RP-003",
        "pack-size-and-topics",
        &problems,
        format!("{total} rules; every pack is large enough and covers its topics"),
    ));

    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for s in &l.seeds {
        for r in &s.rules {
            if !id_form(&r.id) {
                problems
                    .push(format!("{:?} is not a stable id: lower-case letters, digits, `_`, `.` and `-`, at least two characters", r.id));
            } else if !r.id.starts_with(&format!("{}.", s.archetype)) {
                problems.push(format!("{:?} must start with \"{}.\", the pack it is in", r.id, s.archetype));
            }
            if !seen.insert(r.id.as_str()) {
                problems.push(format!("{:?} is used twice", r.id));
            }
        }
    }
    out.push(verdict("RP-004", "rule-ids", &problems, format!("{total} ids, stable in form, each under its archetype and used once")));

    let mut problems = Vec::new();
    for s in &l.seeds {
        for r in &s.rules {
            match (r.classification.as_str(), r.deterministic_checker.as_deref()) {
                ("exploratory", None) => {}
                ("exploratory", Some(_)) => problems.push(format!("{:?} names a checker without being soft", r.id)),
                ("soft", None) => problems.push(format!("{:?} is soft and names no deterministic checker", r.id)),
                ("soft", Some(c)) if !KNOWN_CHECKERS.contains(&c) => {
                    problems.push(format!(
                        "{:?} names {c}, which the generator does not run (it runs {})",
                        r.id,
                        KNOWN_CHECKERS.join(", ")
                    ));
                }
                ("soft", Some(_)) => {}
                (other, _) => {
                    problems.push(format!("{:?} is {other:?}: a seeded rule is exploratory, or soft with a checker, never hard", r.id))
                }
            }
        }
    }
    out.push(verdict("RP-005", "classification", &problems, "no rule is hard; every soft rule names a check the generator runs".into()));

    let mut problems = Vec::new();
    for s in &l.seeds {
        for r in &s.rules {
            for (what, text) in [("subject", &r.subject), ("predicate", &r.predicate), ("object", &r.object), ("rationale", &r.rationale)] {
                if short(text) {
                    problems.push(format!("{:?} {what} is blank or too short", r.id));
                }
            }
            if r.condition.as_deref().is_some_and(short) {
                problems.push(format!("{:?} condition is blank or too short", r.id));
            }
            if r.exceptions.iter().any(|e| short(e)) {
                problems.push(format!("{:?} has a blank exception", r.id));
            }
            if !(0.0..=1.0).contains(&r.confidence) {
                problems.push(format!("{:?} confidence {} is outside 0 to 1", r.id, r.confidence));
            }
        }
    }
    out.push(verdict("RP-006", "rule-fields", &problems, format!("{total} rules with their text, confidence and exceptions in range")));

    let mut problems = Vec::new();
    for s in &l.seeds {
        let exploratory = s.rules.iter().filter(|r| r.classification == "exploratory").count();
        if exploratory * 100 < s.rules.len() * MIN_EXPLORATORY_PERCENT {
            problems.push(format!(
                "{}: {exploratory} of {} rules are exploratory; at least {MIN_EXPLORATORY_PERCENT}% must be, soft rules are the few a check enforces",
                s.archetype,
                s.rules.len()
            ));
        }
    }
    let soft: usize = l.seeds.iter().flat_map(|s| &s.rules).filter(|r| is_soft(r)).count();
    out.push(verdict("RP-007", "exploratory-by-default", &problems, format!("{} exploratory and {soft} soft", total - soft)));

    let mut problems = Vec::new();
    for s in &l.seeds {
        for r in &s.rules {
            if r.source_proposals.is_empty() {
                problems.push(format!("{:?} cites no proposal", r.id));
            }
            for p in &r.source_proposals {
                if !p.contains(&format!("#{}/", s.archetype)) {
                    problems.push(format!("{:?} cites {p:?}, which is not a proposal for the {} pack", r.id, s.archetype));
                } else if !l.refs.contains_key(p) {
                    problems.push(format!("{:?} cites {p:?}, which no model proposed", r.id));
                }
            }
        }
    }
    out.push(verdict(
        "RP-008",
        "sources-exist",
        &problems,
        format!("every rule cites proposals that were made, for its own archetype ({} proposals in all)", l.refs.len()),
    ));

    let mut problems = Vec::new();
    for s in &l.seeds {
        for r in &s.rules {
            let top = r.source_proposals.iter().filter_map(|p| l.refs.get(p)).copied().reduce(f64::max);
            if let Some(top) = top
                && r.confidence > top + 1e-9
            {
                problems.push(format!("{:?} is surer ({}) than every proposal it came from (at most {top})", r.id, r.confidence));
            }
        }
    }
    out.push(verdict("RP-009", "confidence-capped", &problems, "no rule is surer than the most confident model that proposed it".into()));

    let mut problems = Vec::new();
    for s in &l.seeds {
        let ids: BTreeSet<&str> = s.rules.iter().map(|r| r.id.as_str()).collect();
        for c in &s.conflicts {
            if !c.id.starts_with(&format!("{}.", s.archetype)) {
                problems.push(format!("conflict {:?} must start with \"{}.\"", c.id, s.archetype));
            }
            if c.positions.len() < 2 {
                problems.push(format!("conflict {:?} has {} position(s); a conflict has two sides", c.id, c.positions.len()));
            }
            if short(&c.topic) || short(&c.resolution) {
                problems.push(format!("conflict {:?} has a blank topic or resolution", c.id));
            }
            for rid in &c.rule_ids {
                if !ids.contains(rid.as_str()) {
                    problems.push(format!("conflict {:?} names rule {rid:?}, which is not in the {} pack", c.id, s.archetype));
                }
            }
            for p in &c.positions {
                if !p.proposal.contains(&format!("#{}/", s.archetype)) || !l.refs.contains_key(&p.proposal) {
                    problems.push(format!("conflict {:?} cites {:?}, which no model proposed for {}", c.id, p.proposal, s.archetype));
                }
                if short(&p.position) {
                    problems.push(format!("conflict {:?} has a blank position", c.id));
                }
            }
        }
    }
    let conflicts: usize = l.seeds.iter().map(|s| s.conflicts.len()).sum();
    out.push(verdict(
        "RP-010",
        "conflicts-resolve",
        &problems,
        format!("{conflicts} conflicts, each naming rules and proposals that exist"),
    ));

    let open: Vec<String> =
        l.seeds.iter().flat_map(|s| &s.conflicts).filter(|c| c.status == ConflictStatus::Open).map(|c| c.id.clone()).collect();
    let notes: Vec<String> =
        if open.is_empty() { vec![] } else { vec![format!("{} conflict(s) among the models stay open: {}", open.len(), open.join(", "))] };
    out.push(warning("RP-011", "open-conflicts", &notes, "every conflict among the models is resolved in its pack".into()));

    let mut notes = Vec::new();
    for s in &l.seeds {
        let cited: BTreeSet<&str> = s.rules.iter().flat_map(|r| &r.source_proposals).filter_map(|p| p.split('#').next()).collect();
        for label in l.consulted(&s.archetype) {
            if !cited.contains(label) {
                notes.push(format!("{} has no rule from {label}, though it proposed for it", s.archetype));
            }
        }
    }
    out.push(warning("RP-012", "every-model-contributes", &notes, "every model that proposed for a pack has a rule in it".into()));

    let unchecked = l.seeds.iter().flat_map(|s| &s.rules).filter(|r| r.checkability == Checkability::Deterministic && !is_soft(r)).count();
    let notes = if unchecked == 0 {
        vec![]
    } else {
        vec![format!(
            "{unchecked} rule(s) are deterministic to check but no generator check enforces them yet; only {soft} soft rule(s) are cross-checked"
        )]
    };
    out.push(warning(
        "RP-013",
        "deterministic-rules-have-checkers",
        &notes,
        "every deterministic rule is enforced by a generator check".into(),
    ));
    out
}

#[derive(Serialize)]
struct Provenance {
    model_refs: Vec<String>,
    source_proposals: Vec<String>,
}

#[derive(Serialize)]
struct RuleOut<'a> {
    id: &'a str,
    category: Category,
    subject: &'a str,
    predicate: &'a str,
    object: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<&'a str>,
    classification: &'a str,
    checkability: Checkability,
    confidence: f64,
    rationale: &'a str,
    exceptions: &'a [String],
    provenance: Provenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    deterministic_checker: Option<&'a str>,
}

#[derive(Serialize)]
struct Validation {
    rules: usize,
    exploratory: usize,
    soft: usize,
    conflicts: usize,
    conflicts_open: usize,
    proposals_considered: usize,
    proposals_cited_by_a_rule: usize,
    proposals_not_cited_by_a_rule: Vec<String>,
}

#[derive(Serialize)]
struct PackOut<'a> {
    schema_version: &'static str,
    pack_id: String,
    archetype: &'a str,
    status: &'static str,
    origin: &'static str,
    created_by: &'static str,
    models_consulted: Vec<&'a str>,
    prompt_hash: String,
    rules: Vec<RuleOut<'a>>,
    conflicts: &'a [Conflict],
    validation: Validation,
}

fn pretty<T: Serialize>(v: &T) -> Result<String, Refusal> {
    serde_json::to_string_pretty(v)
        .map(|mut t| {
            t.push('\n');
            t
        })
        .map_err(|e| refuse("generation_failed", format!("a document could not be written as JSON: {e}")))
}

fn prompt_hash(prompts: &[&str; 2], arch: &str) -> String {
    fnv1a64(&format!("{}\n--follow-up--\n{}\n--archetype--\n{arch}", prompts[0], prompts[1]))
}

fn pack<'a>(l: &'a Loaded, s: &'a Seed, prompts: &[&str; 2]) -> PackOut<'a> {
    let rules: Vec<RuleOut> = s
        .rules
        .iter()
        .map(|r| {
            let models: BTreeSet<String> = r.source_proposals.iter().filter_map(|p| p.split('#').next()).map(str::to_string).collect();
            RuleOut {
                id: &r.id,
                category: r.category,
                subject: &r.subject,
                predicate: &r.predicate,
                object: &r.object,
                condition: r.condition.as_deref(),
                classification: &r.classification,
                checkability: r.checkability,
                confidence: r.confidence,
                rationale: &r.rationale,
                exceptions: &r.exceptions,
                provenance: Provenance { model_refs: models.into_iter().collect(), source_proposals: r.source_proposals.clone() },
                deterministic_checker: r.deterministic_checker.as_deref(),
            }
        })
        .collect();
    let cited: BTreeSet<&str> = s.rules.iter().flat_map(|r| r.source_proposals.iter().map(String::as_str)).collect();
    let all = l.proposals_for(&s.archetype);
    let not_cited: Vec<String> = all.iter().filter(|p| !cited.contains(p.as_str())).cloned().collect();
    let validation = Validation {
        rules: s.rules.len(),
        exploratory: s.rules.iter().filter(|r| !is_soft(r)).count(),
        soft: s.rules.iter().filter(|r| is_soft(r)).count(),
        conflicts: s.conflicts.len(),
        conflicts_open: s.conflicts.iter().filter(|c| c.status == ConflictStatus::Open).count(),
        proposals_considered: all.len(),
        proposals_cited_by_a_rule: all.len() - not_cited.len(),
        proposals_not_cited_by_a_rule: not_cited,
    };
    PackOut {
        schema_version: SCHEMA_VERSION,
        pack_id: format!("{LIBRARY_ID}/{}", s.archetype),
        archetype: &s.archetype,
        status: "seed",
        origin: "frontier_model_seed",
        created_by: CREATED_BY,
        models_consulted: l.consulted(&s.archetype),
        prompt_hash: prompt_hash(prompts, &s.archetype),
        rules,
        conflicts: &s.conflicts,
        validation,
    }
}

fn capability_notes(l: &Loaded) -> Vec<String> {
    let labels: Vec<&str> = l.models.iter().map(|(label, _)| label.as_str()).collect();
    let soft: usize = l.seeds.iter().flat_map(|s| &s.rules).filter(|r| is_soft(r)).count();
    vec![
        format!(
            "All {} models consulted ({}) are Anthropic models, each asked in its own separate run by alias; their exact versions were not recorded. Their agreement is not independent evidence.",
            labels.len(),
            labels.join(", ")
        ),
        "The rules were curated from the stored proposals by an AI agent and were not reviewed by a person or checked against any published code, standard or guideline. None is a requirement.".into(),
        format!("No rule is hard. {soft} are soft, each tied to a generator check that runs on every generated plan; all others are exploratory."),
        "Confidence is each model's own rating, capped by the proposals a rule came from. It is not calibrated.".into(),
        "No rule is backed by a corpus of real plans: the corpus evidence count is zero throughout.".into(),
        "The prompt hash covers the two prompt files and the archetype. It does not pin a model version or its sampling, so rerunning the models may give other proposals; this library only rebuilds the stored ones.".into(),
    ]
}

/// The inputs this build carries: the five seeds, the three models' proposals and the two prompts.
pub fn embedded() -> Inputs<'static> {
    Inputs { seeds: &SEEDS, proposals: &PROPOSALS, prompts: PROMPTS }
}

/// The library's files, or why there are none. Touches no file.
pub fn build_from(inputs: &Inputs) -> Result<Library, Refusal> {
    let l = load(inputs)?;
    let checks = checks(&l);
    if let Some(first) = checks.iter().find(|c| c.status == Status::Fail) {
        let more = checks.iter().filter(|c| c.status == Status::Fail).count() - 1;
        let more = if more > 0 { format!(" (and {more} more checks failed)") } else { String::new() };
        return Err(refuse("rule_violation", format!("{} {}: {}{more}", first.id, first.name, first.detail)));
    }
    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let mut files: Vec<(&'static str, String)> = Vec::new();
    let mut listed = Vec::new();
    let mut packs = Vec::new();
    for (i, s) in l.seeds.iter().enumerate() {
        let text = pretty(&pack(&l, s, &inputs.prompts))?;
        listed.push(json!({
            "archetype": s.archetype,
            "path": PACK_FILES[i],
            "rules": s.rules.len(),
            "conflicts": s.conflicts.len(),
            "hash": fnv1a64(&text),
        }));
        packs.push(PackSummary { archetype: ARCHETYPES[i], rules: s.rules.len(), conflicts: s.conflicts.len() });
        files.push((PACK_FILES[i], text));
    }
    let report = pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "library_id": LIBRARY_ID,
        "summary": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "checks": checks.iter().map(|c| json!({"id": c.id, "name": c.name, "status": c.status.name(), "detail": c.detail})).collect::<Vec<_>>(),
        "capability_notes": capability_notes(&l),
    }))?;
    let manifest = pretty(&json!({
        "schema_version": SCHEMA_VERSION,
        "library_id": LIBRARY_ID,
        "created_by": CREATED_BY,
        "hash_algorithm": "fnv1a64",
        "models_consulted": l.models.iter().map(|(label, _)| label).collect::<Vec<_>>(),
        "packs": listed,
        "validation_report": {"path": REPORT, "hash": fnv1a64(&report)},
    }))?;
    files.push((REPORT, report));
    files.push((MANIFEST, manifest));
    Ok(Library { files, packs, checks })
}

/// The library built from what this build carries.
pub fn build() -> Result<Library, Refusal> {
    build_from(&embedded())
}

fn put(dir: &Path, name: &str, text: &str, made: &mut Vec<PathBuf>) -> Result<PathBuf, (PathBuf, std::io::Error)> {
    let at = dir.join(name);
    if let Some(parent) = at.parent()
        && parent != dir
        && !parent.exists()
    {
        std::fs::create_dir(parent).map_err(|e| (parent.to_path_buf(), e))?;
        made.push(parent.to_path_buf());
    }
    std::fs::write(&at, text).map_err(|e| (at.clone(), e))?;
    Ok(at)
}

/// Writes the files to `dir`, made if need be; a file that can't be written takes the ones before it away again.
pub fn write(dir: &Path, files: &[(&'static str, String)]) -> Result<(), Refusal> {
    let unwritable = |what: &Path, e: std::io::Error| refuse("output_unwritable", format!("{}: {e}", what.display()));
    std::fs::create_dir_all(dir).map_err(|e| unwritable(dir, e))?;
    let mut written: Vec<PathBuf> = Vec::new();
    let mut made: Vec<PathBuf> = Vec::new();
    for (name, text) in files {
        match put(dir, name, text, &mut made) {
            Ok(at) => written.push(at),
            Err((at, e)) => {
                for done in &written {
                    let _ = std::fs::remove_file(done);
                }
                for d in made.iter().rev() {
                    let _ = std::fs::remove_dir(d);
                }
                return Err(unwritable(&at, e));
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    path: String,
}

fn run(p: &Value) -> Result<Value, Refusal> {
    let given: Params = serde_json::from_value(p.clone()).map_err(|e| {
        let why: String = e.to_string().chars().take(200).collect();
        refuse("invalid_params", format!("{why}; `rule_pack` takes `path`, the directory to write the library to"))
    })?;
    if given.path.trim().is_empty() || given.path.len() > MAX_PATH || given.path.contains('\0') {
        return Err(refuse("invalid_params", format!("`path` is the directory to write: not empty, at most {MAX_PATH} bytes")));
    }
    let lib = build()?;
    let dir = PathBuf::from(&given.path);
    write(&dir, &lib.files)?;
    let count = |s: Status| lib.checks.iter().filter(|c| c.status == s).count();
    Ok(json!({
        "path": dir,
        "library_id": LIBRARY_ID,
        "packs": lib.packs.iter().map(|p| json!({"archetype": p.archetype, "rules": p.rules, "conflicts": p.conflicts})).collect::<Vec<_>>(),
        "checks": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "files": lib.files.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
    }))
}

/// The `rule_pack` command: the seed library to a directory.
pub fn rule_pack(_: &mut Session, p: &Value) -> Result<Value, String> {
    run(p).map_err(|r| r.to_string())
}
