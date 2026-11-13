//! The office-pattern pack's evaluators: a lobby at one end with the only way in from outside, a corridor with rooms off it, and
//! an open office at its far end. Read off a graph whose spaces have kinds; double-swing doors are swing doors here.

use std::collections::{BTreeSet, VecDeque};

use super::{Check, Edge, Graph, Pack, Rule, World, verdict};

/// Every rule of the office-pattern pack, judged on `graph`.
pub(super) fn check(pack: &Pack, graph: &Graph) -> Result<Vec<Check>, String> {
    let world = World::of(pack, graph);
    pack.rules.iter().map(|r| world.pattern(r)).collect()
}

impl<'a> World<'a> {
    fn pattern(&self, rule: &Rule) -> Result<Check, String> {
        let (problems, ok) = match rule.id.as_str() {
            "OP-001" => self.kinds_known(),
            "OP-002" => self.edges_name_spaces(),
            "OP-003" => self.counts(),
            "OP-004" => self.areas(),
            "OP-005" => self.connections(),
            "OP-006" => self.one_way_in(rule)?,
            "OP-007" => self.entry_space(rule)?,
            "OP-008" | "OP-009" | "OP-010" => self.door_to(rule)?,
            "OP-011" => self.at_the_end(rule)?,
            "OP-012" => self.all_reached(),
            other => return Err(format!("the pack's rule {other} has no evaluator")),
        };
        Ok(verdict(rule, problems, ok))
    }

    /// The edges inside the plan that touch `id`, with the space at their other end.
    fn near(&self, id: &str) -> Vec<(&'a str, &'a Edge)> {
        self.inside
            .iter()
            .filter_map(|e| {
                if e.from == id {
                    Some((e.to.as_str(), *e))
                } else if e.to == id {
                    Some((e.from.as_str(), *e))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Exactly one door to the outside, and it opens off a space of `kind`.
    fn one_way_in(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let kind = Self::word(rule, "kind")?;
        let ways: Vec<&Edge> = self.graph.edges.iter().filter(|e| e.exterior).collect();
        let mut problems = Vec::new();
        if ways.len() != 1 {
            problems.push(format!("{} doors to the outside, the pattern has one", ways.len()));
        }
        for e in ways {
            let inner = if e.from == "exterior" { &e.to } else { &e.from };
            if self.kind_of(inner) != Some(kind) {
                problems.push(format!("the door to the outside opens off `{inner}`, not a {kind}"));
            }
        }
        Ok((problems, format!("one door to the outside, on the {kind}")))
    }

    /// Each space of the `kinds` has a door (not an open link) to a space of kind `to`.
    fn door_to(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (kinds, to) = (Self::words(rule, "kinds")?, Self::word(rule, "to")?);
        let mut problems = Vec::new();
        for n in &self.graph.nodes {
            if !n.kind.as_deref().is_some_and(|k| kinds.contains(&k)) {
                continue;
            }
            let served = self.near(&n.id).iter().any(|(x, e)| e.kind == "door" && self.kind_of(x) == Some(to));
            if !served {
                problems.push(format!("`{}` has no door to a {to}", n.id));
            }
        }
        Ok((problems, format!("every {} has a door to the {to}", kinds.join(", "))))
    }

    /// The open office joins a space of kind `to` and none of the `not` kinds.
    fn at_the_end(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (kinds, to, not) = (Self::words(rule, "kinds")?, Self::word(rule, "to")?, Self::words(rule, "not")?);
        let mut problems = Vec::new();
        for n in &self.graph.nodes {
            if !n.kind.as_deref().is_some_and(|k| kinds.contains(&k)) {
                continue;
            }
            let near = self.near(&n.id);
            if !near.iter().any(|(x, _)| self.kind_of(x) == Some(to)) {
                problems.push(format!("`{}` does not join the {to}", n.id));
            }
            for (x, _) in &near {
                if self.kind_of(x).is_some_and(|k| not.contains(&k)) {
                    problems.push(format!("`{}` joins `{x}`, a `{}`", n.id, self.kind_of(x).unwrap_or("space of no kind")));
                }
            }
        }
        Ok((problems, format!("every {} joins the {to}, and no {}", kinds.join(", "), not.join(" or "))))
    }

    /// Every space is reached from the entry by doors and open links.
    fn all_reached(&self) -> (Vec<String>, String) {
        let ok = "every space is reached from the entry".to_string();
        let Some(entry) = self.graph.entry.as_deref().filter(|e| self.kind.contains_key(e)) else {
            return (vec!["there is no entry to reach anything from".into()], ok);
        };
        let access = self.access();
        let mut seen = BTreeSet::from([entry]);
        let mut queue = VecDeque::from([entry]);
        while let Some(here) = queue.pop_front() {
            for &there in access.get(here).into_iter().flatten() {
                if seen.insert(there) {
                    queue.push_back(there);
                }
            }
        }
        let problems = self
            .graph
            .nodes
            .iter()
            .filter(|n| !seen.contains(n.id.as_str()))
            .map(|n| format!("`{}` is not reached from `{entry}`", n.id))
            .collect();
        (problems, ok)
    }
}
