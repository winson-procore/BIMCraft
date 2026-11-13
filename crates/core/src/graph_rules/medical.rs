//! The medical-day pack's evaluators, MD-001 to MD-011: a department of treatment bays, treatment and isolation rooms, utility rooms
//! and corridors, with no way in from outside. Read off a graph whose spaces have kinds. Every link is a door (any operation but
//! `open`) or an open link; links count one by one, so two doors between the same rooms are two links.

use std::collections::{BTreeMap, BTreeSet};

use super::{Check, Graph, Pack, Rule, verdict};

/// A link between two spaces of the graph: `true` when it is a door.
type Link<'a> = (&'a str, bool);

struct Plan<'a> {
    kind: BTreeMap<&'a str, &'a str>,
    near: BTreeMap<&'a str, Vec<Link<'a>>>,
}

/// Every rule of the medical-day pack, judged on `graph`.
pub(super) fn check(pack: &Pack, graph: &Graph) -> Result<Vec<Check>, String> {
    let mut kind = BTreeMap::new();
    for n in &graph.nodes {
        let k = n.kind.as_deref().ok_or_else(|| format!("node `{}` has no `kind`", n.id))?;
        if !pack.kinds.contains_key(k) {
            return Err(format!("node `{}` has the kind `{k}`, which is not one of the pack's", n.id));
        }
        kind.insert(n.id.as_str(), k);
    }
    let mut near: BTreeMap<&str, Vec<Link>> = kind.keys().map(|k| (*k, Vec::new())).collect();
    let mut outside = graph.exterior_doors;
    for e in &graph.edges {
        let k = e.kind.to_lowercase();
        if k != "door" && k != "open" {
            continue;
        }
        let known = kind.contains_key(e.from.as_str()) && kind.contains_key(e.to.as_str());
        if e.exterior || !known {
            outside += 1;
            continue;
        }
        let door = k == "door" && e.operation.as_deref().map(str::to_lowercase).as_deref() != Some("open");
        near.entry(e.from.as_str()).or_default().push((e.to.as_str(), door));
        near.entry(e.to.as_str()).or_default().push((e.from.as_str(), door));
    }
    let plan = Plan { kind, near };
    let mut checks = Vec::new();
    for rule in &pack.rules {
        let (problems, ok) = plan.rule(rule, graph, outside)?;
        checks.push(verdict(rule, problems, ok));
    }
    Ok(checks)
}

impl<'a> Plan<'a> {
    fn of(&self, kinds: &[&str]) -> Vec<&'a str> {
        self.kind.iter().filter(|(_, k)| kinds.contains(k)).map(|(i, _)| *i).collect()
    }

    fn corridor(&self, id: &str) -> bool {
        self.kind.get(id) == Some(&"corridor")
    }

    fn near(&self, id: &str) -> &[Link<'a>] {
        self.near.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    fn door_to_corridor(&self, id: &str) -> bool {
        self.near(id).iter().any(|(x, door)| self.corridor(x) && *door)
    }

    fn rule(&self, rule: &Rule, _graph: &Graph, outside: usize) -> Result<(Vec<String>, String), String> {
        let mut bad = Vec::new();
        let ok;
        match rule.id.as_str() {
            "MD-001" => {
                ok = "every treatment bay opens to a corridor and has no door";
                for i in self.of(&["treatment_bay"]) {
                    let n = self.near(i);
                    if !n.iter().any(|(x, door)| self.corridor(x) && !door) || n.iter().any(|(_, door)| *door) {
                        bad.push(format!("treatment bay `{i}` is not open to a corridor alone"));
                    }
                }
            }
            "MD-002" => {
                ok = "every treatment room has a door to a corridor";
                for i in self.of(&["treatment_room"]) {
                    if !self.door_to_corridor(i) {
                        bad.push(format!("treatment room `{i}` has no door to a corridor"));
                    }
                }
            }
            "MD-003" => {
                ok = "every isolation room has its anteroom and washroom";
                for i in self.of(&["isolation"]) {
                    let ante =
                        self.near(i).iter().any(|(x, door)| *door && self.kind.get(x) == Some(&"anteroom") && self.door_to_corridor(x));
                    let wash =
                        self.near(i).iter().any(|(x, door)| *door && self.kind.get(x) == Some(&"washroom") && self.near(x).len() == 1);
                    if !(self.door_to_corridor(i) && ante && wash) {
                        bad.push(format!("isolation room `{i}` lacks a corridor door, an anteroom or a washroom of its own"));
                    }
                }
            }
            "MD-004" => {
                ok = "clean and soiled utility are apart and each has a door to a corridor";
                for i in self.of(&["clean_utility", "soiled_utility"]) {
                    let other = if self.kind.get(i) == Some(&"clean_utility") { "soiled_utility" } else { "clean_utility" };
                    if self.near(i).iter().any(|(x, _)| self.kind.get(x) == Some(&other)) || !self.door_to_corridor(i) {
                        bad.push(format!("utility room `{i}` is joined to its opposite or has no door to a corridor"));
                    }
                }
            }
            "MD-005" => {
                ok = "every medication room has one door, to a corridor";
                for i in self.of(&["medication"]) {
                    if self.near(i).len() != 1 || !self.door_to_corridor(i) {
                        bad.push(format!("medication room `{i}` has not exactly one link, a door to a corridor"));
                    }
                }
            }
            "MD-006" => {
                ok = "every washroom has doors only, one each way";
                for i in self.of(&["washroom"]) {
                    let n = self.near(i);
                    let corridors = n.iter().filter(|(x, _)| self.corridor(x)).count();
                    let others = n.len() - corridors;
                    if n.is_empty() || n.iter().any(|(_, door)| !door) || corridors > 1 || others > 1 {
                        bad.push(format!("washroom `{i}` has an open link, no link or more than one door each way"));
                    }
                }
            }
            "MD-007" => {
                ok = "every office and meeting room has a door to a corridor and no open link";
                for i in self.of(&["office", "meeting"]) {
                    if !self.door_to_corridor(i) || self.near(i).iter().any(|(_, door)| !door) {
                        bad.push(format!("`{i}` has no door to a corridor or has an open link"));
                    }
                }
            }
            "MD-008" => {
                ok = "every staff work area is open to a corridor or another and has no door";
                for i in self.of(&["staff_work"]) {
                    let n = self.near(i);
                    let open = n.iter().any(|(x, door)| !door && matches!(self.kind.get(x), Some(&"corridor") | Some(&"staff_work")));
                    if !open || n.iter().any(|(_, door)| *door) {
                        bad.push(format!("staff work area `{i}` is not open alone to a corridor or staff work area"));
                    }
                }
            }
            "MD-009" => {
                ok = "the corridors are one connected set";
                let cs = self.of(&["corridor"]);
                match cs.first() {
                    None => bad.push("the plan has no corridors".to_string()),
                    Some(first) => {
                        let mut seen = BTreeSet::from([*first]);
                        let mut todo = vec![*first];
                        while let Some(here) = todo.pop() {
                            for (x, _) in self.near(here) {
                                if self.corridor(x) && seen.insert(*x) {
                                    todo.push(x);
                                }
                            }
                        }
                        bad.extend(cs.iter().filter(|c| !seen.contains(*c)).map(|c| format!("corridor `{c}` is cut off from the others")));
                    }
                }
            }
            "MD-010" => {
                ok = "every room reaches a corridor";
                let cs = self.of(&["corridor"]);
                let mut seen: BTreeSet<&str> = cs.iter().copied().collect();
                let mut todo = cs;
                while let Some(here) = todo.pop() {
                    for (x, _) in self.near(here) {
                        if seen.insert(*x) {
                            todo.push(x);
                        }
                    }
                }
                bad.extend(
                    self.kind
                        .iter()
                        .filter(|(i, k)| !seen.contains(*i) && **k != "shaft")
                        .map(|(i, _)| format!("`{i}` reaches no corridor")),
                );
            }
            "MD-011" => {
                ok = "every link has a room on both sides";
                if outside > 0 {
                    bad.push(format!("{outside} link(s) have a room on one side only"));
                }
            }
            other => return Err(format!("the pack's rule {other} has no evaluator")),
        }
        Ok((bad, ok.to_string()))
    }
}
