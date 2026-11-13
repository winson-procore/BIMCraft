//! The studio-office pack's evaluators: a reception, a corridor and a passage of WCs, a meeting room and a store off the
//! corridor, and a studio the corridor reaches by a door and the passage by an open link. Read off a graph whose spaces
//! have kinds; it needs no roles. Double-swing is a means of its own here, which the other packs fold into swing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{Check, Edge, Graph, Pack, Rule, World, verdict};

/// Every rule of the studio-office pack, judged on `graph`.
pub(super) fn check(pack: &Pack, graph: &Graph) -> Result<Vec<Check>, String> {
    let world = World::of(pack, graph);
    pack.rules.iter().map(|r| world.studio(r)).collect()
}

/// How an inside edge joins its two spaces: `open`, or the door's operation (double-sliding as sliding).
fn means(e: &Edge) -> Option<&str> {
    match (e.kind.as_str(), e.operation.as_deref()) {
        ("open", _) => Some("open"),
        (_, Some("double-sliding")) => Some("sliding"),
        (_, op) => op,
    }
}

impl<'a> World<'a> {
    fn studio(&self, rule: &Rule) -> Result<Check, String> {
        let (problems, ok) = match rule.id.as_str() {
            "SO-001" => self.kinds_known(),
            "SO-002" => self.edges_name_spaces(),
            "SO-003" => self.counts(),
            "SO-004" => self.areas(),
            "SO-005" => self.means_apart(),
            "SO-006" => self.entry_space(rule)?,
            "SO-007" => self.reach_all(rule)?,
            "SO-008" => self.joins_only(rule)?,
            "SO-009" => self.passage_open(rule)?,
            "SO-010" => self.corridor_serves(rule)?,
            "SO-011" => self.reception_joins(rule)?,
            "SO-012" => self.largest(rule)?,
            other => return Err(format!("the pack's rule {other} has no evaluator")),
        };
        Ok(verdict(rule, problems, ok))
    }

    /// The edges inside the plan that touch `id`, with the space at their other end.
    fn touching(&self, id: &str) -> Vec<(&'a str, &'a Edge)> {
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

    /// The halls that have a space of kind `with` off them, and the halls that have none: the passages and the corridors.
    fn halls_by(&self, with: &str) -> (BTreeSet<&'a str>, BTreeSet<&'a str>) {
        let (mut yes, mut no) = (BTreeSet::new(), BTreeSet::new());
        for h in self.of_kind("hall") {
            if self.touching(h).iter().any(|(x, _)| self.kind_of(x) == Some(with)) {
                yes.insert(h);
            } else {
                no.insert(h);
            }
        }
        (yes, no)
    }

    /// Each door or open edge joins a pair of kinds the pack lists, by a means it lists for them; a double-swing door is
    /// only reception to a corridor or a hall to a meeting room, and a WC or a store has a plain swing door.
    fn means_apart(&self) -> (Vec<String>, String) {
        let mut allowed: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
        for c in &self.pack.connections {
            let (a, b) = (c.pair[0].as_str(), c.pair[1].as_str());
            allowed.entry((a.min(b), a.max(b))).or_default().extend(c.means.iter().map(String::as_str));
        }
        let (passages, _) = self.halls_by("bath");
        let mut problems = Vec::new();
        for e in &self.inside {
            let (Some(a), Some(b)) = (self.known(&e.from), self.known(&e.to)) else { continue };
            let Some(m) = means(e) else {
                problems.push(format!("`{}` to `{}`: the door has no `operation`", e.from, e.to));
                continue;
            };
            if !allowed.get(&(a.min(b), a.max(b))).is_some_and(|set| set.contains(m)) {
                problems.push(format!("`{}` ({a}) to `{}` ({b}) by {m} is not allowed", e.from, e.to));
            } else if m == "double-swing" && (passages.contains(e.from.as_str()) || passages.contains(e.to.as_str())) {
                problems.push(format!("`{}` to `{}`: the passage has plain swing doors", e.from, e.to));
            }
        }
        (problems, format!("all {} door and open edges join kinds the pack allows, by an allowed means", self.inside.len()))
    }

    /// Every space of the `targets` kinds is reached from the entry without going through a space of the blocked kinds.
    fn reach_all(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (targets, blocked) = (Self::words(rule, "targets")?, Self::words(rule, "through_blocked")?);
        let ok = format!("every {} is reached from the entry without passing through {}", targets.join(", "), blocked.join(" or "));
        let Some(entry) = self.graph.entry.as_deref().filter(|e| self.kind.contains_key(e)) else {
            return Ok((vec!["there is no entry to reach anything from".into()], ok));
        };
        let access = self.access();
        let mut seen = BTreeSet::from([entry]);
        let mut queue = VecDeque::from([entry]);
        while let Some(here) = queue.pop_front() {
            for &there in access.get(here).into_iter().flatten() {
                if seen.insert(there) && !self.kind_of(there).is_some_and(|k| blocked.contains(&k)) {
                    queue.push_back(there);
                }
            }
        }
        let problems = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.kind.as_deref().is_some_and(|k| targets.contains(&k)) && !seen.contains(n.id.as_str()))
            .map(|n| format!("`{}` is not reached from `{entry}` except through the studio", n.id))
            .collect();
        Ok((problems, ok))
    }

    /// A hall with a `with` space off it is joined to every `to` space by an open link, and by no door.
    fn passage_open(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (with, to) = (Self::word(rule, "with")?, Self::word(rule, "to")?);
        let (passages, _) = self.halls_by(with);
        let mut problems = Vec::new();
        for p in passages {
            let ends: Vec<_> = self.touching(p).into_iter().filter(|(x, _)| self.kind_of(x) == Some(to)).collect();
            if !ends.iter().any(|(_, e)| e.kind == "open") {
                problems.push(format!("the passage `{p}` has no open link to the studio"));
            }
            for (x, e) in &ends {
                if e.kind == "door" {
                    problems.push(format!("the passage `{p}` has a door to `{x}`, where it is open"));
                }
            }
        }
        Ok((problems, format!("every passage is open to the {to}, with no door to it")))
    }

    /// Each space of the `kinds` has a door to a hall that has no `hall_without` space off it, and one of those halls joins a
    /// `hall_joins` space.
    fn corridor_serves(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (kinds, without, joins) = (Self::words(rule, "kinds")?, Self::word(rule, "hall_without")?, Self::word(rule, "hall_joins")?);
        let (_, corridors) = self.halls_by(without);
        let mut problems = Vec::new();
        for n in &self.graph.nodes {
            if !n.kind.as_deref().is_some_and(|k| kinds.contains(&k)) {
                continue;
            }
            let served = self.touching(&n.id).iter().any(|(x, e)| e.kind == "door" && corridors.contains(x));
            if !served {
                problems.push(format!("`{}` has no door to a corridor", n.id));
            }
        }
        let ends = corridors.iter().any(|c| self.touching(c).iter().any(|(x, _)| self.kind_of(x) == Some(joins)));
        if !corridors.is_empty() && !ends {
            problems.push(format!("no corridor joins the {joins}"));
        }
        Ok((problems, format!("every {} has a door to a corridor, which joins the {joins}", kinds.join(" and "))))
    }

    /// A space of `kind` joins a hall with a `with` space off it (a passage) and one without (a corridor).
    fn reception_joins(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let (kind, with) = (Self::word(rule, "kind")?, Self::word(rule, "with")?);
        let (passages, corridors) = self.halls_by(with);
        let mut problems = Vec::new();
        for id in self.of_kind(kind) {
            let near = self.touching(id);
            if !near.iter().any(|(x, _)| corridors.contains(x)) {
                problems.push(format!("`{id}` joins no corridor"));
            }
            if !near.iter().any(|(x, _)| passages.contains(x)) {
                problems.push(format!("`{id}` joins no passage"));
            }
        }
        Ok((problems, format!("every {kind} joins a corridor and a passage")))
    }

    /// The space of `kind` has the biggest area of the plan.
    fn largest(&self, rule: &Rule) -> Result<(Vec<String>, String), String> {
        let kind = Self::word(rule, "kind")?;
        let biggest = self.graph.nodes.iter().filter_map(|n| Some((n, n.area_m2?))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(n, _)| n);
        let problems = match biggest {
            Some(n) if n.kind.as_deref() != Some(kind) => {
                vec![format!("`{}` is the largest space, and it is not the {kind}", n.id)]
            }
            _ => Vec::new(),
        };
        Ok((problems, format!("the {kind} is the largest space")))
    }
}
