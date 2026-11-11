//! What was checked and how it went. Each rule has a stable id and says what it found; a failed rule means no files are
//! written, so a report on disk holds passes and warnings only. The rules are data-shaped on purpose (roles and edge kinds
//! a predicate reads), so a later slice can load them from a versioned pack.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::graph::{SCHEMA_VERSION, Topology};
use super::layout::Plan;
use super::program::{Archetype, Role};
use super::script::{REQUIRED, Script};

pub const RULE_SET: &str = "slice1-hard-rules/1";
/// The least commands a generated script holds: a plan with its walls, doors, rooms and annotation is never shorter.
pub const MIN_COMMANDS: usize = 30;
/// How far from the requested area the plan may land before the report warns.
const AREA_TOLERANCE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

fn check(id: &'static str, name: &'static str, problems: Vec<String>, ok: String) -> Check {
    match problems.first() {
        None => Check { id, name, status: Status::Pass, detail: ok },
        Some(first) => Check {
            id,
            name,
            status: Status::Fail,
            detail: format!("{first}{}", if problems.len() > 1 { format!(" (and {} more)", problems.len() - 1) } else { String::new() }),
        },
    }
}

fn is_private(role: Role) -> bool {
    matches!(role, Role::Private | Role::Clinical)
}

fn is_open(role: Role) -> bool {
    matches!(role, Role::Circulation | Role::Shared | Role::Public)
}

/// The spaces reached from `from` over door and open edges only.
fn reached(t: &Topology, from: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    let mut next: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in t.access() {
        next.entry(e.from.as_str()).or_default().push(e.to.as_str());
        next.entry(e.to.as_str()).or_default().push(e.from.as_str());
    }
    let mut seen: BTreeSet<String> = from.into_iter().collect();
    let mut frontier: Vec<String> = seen.iter().cloned().collect();
    while let Some(here) = frontier.pop() {
        for &there in next.get(here.as_str()).map(Vec::as_slice).unwrap_or_default() {
            if seen.insert(there.to_string()) {
                frontier.push(there.to_string());
            }
        }
    }
    seen
}

/// The rules on the access graph, which a plan must meet however it is laid out.
pub fn graph_checks(t: &Topology, archetype: Archetype) -> Vec<Check> {
    let role = |id: &str| t.node(id).map(|n| n.role);
    let missing: Vec<String> = t
        .edges
        .iter()
        .flat_map(|e| [&e.from, &e.to])
        .filter(|id| t.node(id).is_none())
        .map(|id| format!("an edge names `{id}`, which is not a space"))
        .collect();
    let open: Vec<String> = t.nodes.iter().filter(|n| is_open(n.role)).map(|n| n.id.clone()).collect();
    let from_open = reached(t, open);
    let lost: Vec<String> = t
        .nodes
        .iter()
        .filter(|n| is_private(n.role) && !from_open.contains(&n.id))
        .map(|n| format!("`{}` cannot be reached from a circulation, shared or public space", n.id))
        .collect();
    let joined = |a: fn(Role) -> bool, b: fn(Role) -> bool| -> Vec<String> {
        t.access()
            .filter(|e| role(&e.from).is_some_and(a) && role(&e.to).is_some_and(b))
            .map(|e| format!("`{}` opens straight into `{}`", e.from, e.to))
            .collect()
    };
    let private_private = joined(|r| r == Role::Private, |r| r == Role::Private);
    let clinical_clinical = joined(|r| r == Role::Clinical, |r| r == Role::Clinical);
    let mixed: Vec<String> = joined(|r| r == Role::Private, |r| r == Role::Clinical)
        .into_iter()
        .chain(joined(|r| r == Role::Clinical, |r| r == Role::Private))
        .collect();
    let from_entry = reached(t, [t.entry.clone()]);
    let cut_off: Vec<String> = t
        .nodes
        .iter()
        .filter(|n| !from_entry.contains(&n.id))
        .map(|n| format!("`{}` cannot be reached from the entry `{}`", n.id, t.entry))
        .collect();
    let access = t.access().count();
    vec![
        check("GR-001", "edges-name-spaces", missing, format!("{} edges, all between spaces", t.edges.len())),
        check(
            "GR-002",
            "private-reachable",
            lost,
            "every private and clinical space is reached from a circulation, shared or public one".into(),
        ),
        check("GR-003", "private-not-to-private", private_private, "no private space opens into another".into()),
        check("GR-004", "clinical-not-to-clinical", clinical_clinical, "no clinical space opens into another".into()),
        check("GR-005", "private-not-to-clinical", mixed, "no private space opens into a clinical one".into()),
        check(
            "GR-006",
            "entry-connected",
            cut_off,
            format!("all {} spaces are reached from `{}` over {access} door and open edges", t.nodes.len(), t.entry),
        ),
        program_check(t, archetype),
    ]
}

/// The kinds a program must hold at least once, each group with how many of them it needs.
fn minimum(archetype: Archetype) -> Vec<(&'static str, &'static [&'static str], usize)> {
    match archetype {
        Archetype::Office => vec![
            ("an entry or lobby", &["entry_lobby"], 1),
            ("a corridor", &["corridor"], 1),
            ("an open office", &["open_office"], 1),
            ("a conference room", &["conference"], 1),
            ("a private office or focus room", &["private_office", "focus_room"], 1),
            ("a restroom", &["restroom"], 1),
            ("a support space (IT, storage or pantry)", &["it_storage", "pantry"], 1),
        ],
        Archetype::Clinic => vec![
            ("an entry or lobby", &["entry_lobby"], 1),
            ("a waiting room", &["waiting"], 1),
            ("a corridor", &["corridor"], 1),
            ("two exam rooms", &["exam"], 2),
            ("a restroom", &["restroom"], 1),
            ("a staff or nurse space", &["staff_room", "nurse_station"], 1),
            ("a clean, support or storage space", &["clean_storage"], 1),
        ],
    }
}

fn program_check(t: &Topology, archetype: Archetype) -> Check {
    let problems: Vec<String> = minimum(archetype)
        .into_iter()
        .filter(|(_, kinds, n)| t.nodes.iter().filter(|node| kinds.contains(&node.kind.as_str())).count() < *n)
        .map(|(what, _, _)| format!("the {} program has no {what}", archetype.name()))
        .collect();
    check("PR-001", "minimum-program", problems, format!("the {} minimum program is present", archetype.name()))
}

/// The rules on the script and on what BIMCraft did with it. `replies` are the commands' answers, line for line.
pub fn run_checks(plan: &Plan, script: &Script, replies: &[Value], model: Result<(), String>) -> Vec<Check> {
    let ids: BTreeSet<&str> = script.lines.iter().map(|l| l.id).collect();
    let mut problems: Vec<String> =
        REQUIRED.iter().filter(|id| !ids.contains(*id)).map(|id| format!("the script has no `{id}` command")).collect();
    if script.lines.len() < MIN_COMMANDS {
        problems.push(format!("the script has {} commands, fewer than {MIN_COMMANDS}", script.lines.len()));
    }
    let commands = check("SC-001", "script-commands", problems, format!("{} commands, every required one among them", script.lines.len()));
    let own: Vec<String> = script
        .lines
        .iter()
        .filter(|l| l.id == "generate" || l.params.get("path").is_some())
        .map(|l| format!("`{}` takes a file or runs the generator, which a script must not", l.id))
        .collect();
    let closed = check("SC-002", "script-is-self-contained", own, "no command names a file or runs the generator".into());
    let ran = check(
        "RT-001",
        "script-runs",
        if replies.len() == script.lines.len() {
            Vec::new()
        } else {
            vec![format!("{} of {} commands ran", replies.len(), script.lines.len())]
        },
        format!("all {} commands ran in a fresh project", script.lines.len()),
    );
    let unbounded: Vec<String> = script
        .lines
        .iter()
        .zip(replies)
        .filter_map(|(l, r)| {
            let space = l.room_of.as_ref()?;
            let net = r.get("area_m2").and_then(Value::as_f64).unwrap_or(0.0);
            match r.get("warning") {
                Some(w) => Some(format!("the room of `{space}` is unbounded: {}", w.as_str().unwrap_or_default())),
                None if net <= 0.0 => Some(format!("the room of `{space}` has no area")),
                None => None,
            }
        })
        .collect();
    let rooms = check("RT-002", "rooms-bounded", unbounded, format!("all {} rooms are bounded by walls", plan.cells.len() + 1));
    let model = check("RT-003", "model-valid", model.err().into_iter().collect(), "the project passes the model's own check".into());
    let deviation = (plan.gross_m2() - plan.requested_m2).abs() / plan.requested_m2.max(f64::MIN_POSITIVE);
    let area = if deviation <= AREA_TOLERANCE {
        Check {
            id: "GM-001",
            name: "area-fidelity",
            status: Status::Pass,
            detail: format!("{} m2 gross for {} m2 asked", plan.gross_m2(), plan.requested_m2),
        }
    } else {
        Check {
            id: "GM-001",
            name: "area-fidelity",
            status: Status::Warn,
            detail: format!(
                "{} m2 gross for {} m2 asked: the plan's depth is held between 12 and 24 m",
                plan.gross_m2(),
                plan.requested_m2
            ),
        }
    };
    vec![commands, closed, ran, rooms, model, area]
}

/// The report: checks, warnings, what the generator could not do, and scores a ranking can start from.
pub fn json(plan: &Plan, checks: &[Check], script: &Script, replies: &[Value]) -> Value {
    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let measured: Vec<Value> = script
        .lines
        .iter()
        .zip(replies)
        .filter_map(|(l, r)| Some(json!({"space": l.room_of.as_ref()?, "net_area_m2": r.get("area_m2")?})))
        .collect();
    let net: f64 = measured.iter().filter_map(|m| m["net_area_m2"].as_f64()).sum();
    let required = minimum(plan.archetype).len();
    let passing = checks.iter().filter(|c| c.status != Status::Fail).count();
    json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": plan.archetype.name(),
        "seed": plan.seed,
        "rule_set": RULE_SET,
        "summary": {"pass": count(Status::Pass), "warn": count(Status::Warn), "fail": count(Status::Fail)},
        "checks": checks.iter().map(|c| json!({"id": c.id, "name": c.name, "status": c.status.name(), "detail": c.detail})).collect::<Vec<_>>(),
        "warnings": checks.iter().filter(|c| c.status == Status::Warn).map(|c| format!("{}: {}", c.id, c.detail)).collect::<Vec<_>>(),
        "capability_notes": [
            "Areas in the topology and building graphs are gross, measured to wall centre lines; `measured_rooms` holds the net areas BIMCraft computed for each room.",
            "One storey and one rectangular footprint: a corridor across the middle and a band of rooms on each side. No stairs, shafts, structure or MEP.",
            "Every room has one door, to the corridor; the lobby is also the building's entry. Door swing and hand are BIMCraft's defaults.",
            "The rules are hard-coded in this slice (rule set slice1-hard-rules/1); versioned rule packs are future work.",
        ],
        "scores": {
            "rule_checks_passing": format!("{passing}/{}", checks.len()),
            "program_groups_required": required,
            "room_count": plan.cells.len() + 1,
            "net_to_gross": if plan.gross_m2() > 0.0 { (net / plan.gross_m2() * 1000.0).round() / 1000.0 } else { 0.0 },
        },
        "measured_rooms": measured,
    })
}

pub fn failures(checks: &[Check]) -> Vec<&Check> {
    checks.iter().filter(|c| c.status == Status::Fail).collect()
}
