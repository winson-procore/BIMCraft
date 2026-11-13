//! Which way a house's swing doors open and which jamb they hang from: rules read off the traced house
//! (`data/house-swing.json`, SW- ids, all exploratory), what the checker judges and what the generator sets.
//!
//! A rule names a pair of room kinds (sorted), the kind the door opens into (`opens_into`) and the `hinge` (`start` or `end`).
//! The one pair with `exterior` in it is the exterior object: it applies to every exterior swing door, whatever its room's
//! kind. Its `opens_into` is `exterior` when the door opens out, any other value when it opens in, toward its own room.
//! `either` means that half is not enforced. A door with no `opens_into` or `hinge` is not judged.

use std::sync::OnceLock;

use serde::Deserialize;

use super::HouseGraph;
use super::place::{Layout, Rect};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwingRule {
    pub id: String,
    pub pair: [String; 2],
    pub opens_into: String,
    pub hinge: String,
    /// Always `exploratory`: read off one plan.
    pub status: String,
    /// Where in the traced house the rule is read from; for people reading the file.
    #[allow(dead_code)]
    pub provenance: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[allow(dead_code)]
    schema_version: String,
    #[allow(dead_code)]
    source: String,
    #[allow(dead_code)]
    note: String,
    swing: Vec<SwingRule>,
}

/// The embedded rules, parsed once.
pub fn rules() -> Result<&'static [SwingRule], String> {
    static RULES: OnceLock<Result<File, String>> = OnceLock::new();
    match RULES.get_or_init(|| {
        serde_json::from_str(include_str!("../data/house-swing.json")).map_err(|e| format!("the embedded house swing rules: {e}"))
    }) {
        Ok(f) => Ok(&f.swing),
        Err(e) => Err(e.clone()),
    }
}

/// Unit 1's rules, read off its script by the same definition (`data/house-swing-unit1.json`).
fn unit1_rules() -> Result<&'static [SwingRule], String> {
    static RULES: OnceLock<Result<File, String>> = OnceLock::new();
    match RULES.get_or_init(|| {
        serde_json::from_str(include_str!("../data/house-swing-unit1.json")).map_err(|e| format!("the embedded Unit 1 swing rules: {e}"))
    }) {
        Ok(f) => Ok(&f.swing),
        Err(e) => Err(e.clone()),
    }
}

/// Whether an exterior rule's `opens_into` says the door opens in (any value but `exterior` and `either`).
fn inward(rule: &SwingRule, word: &str) -> bool {
    rule.pair.iter().any(|k| k == "exterior") && word != "exterior" && word != "either"
}

/// What the checker judges by: the traced house's rules, each half (`opens_into`, `hinge`) left to `either` where a pair that
/// Unit 1 also has says something else. A pair only one trace has keeps that trace's rule, and where the two traces agree
/// the rule is as it was.
pub fn judged_rules() -> Result<&'static [SwingRule], String> {
    static JUDGED: OnceLock<Result<Vec<SwingRule>, String>> = OnceLock::new();
    match JUDGED.get_or_init(|| {
        let unit1 = unit1_rules()?;
        let relax = |rule: &SwingRule, ours: &str, theirs: &str| {
            if ours == theirs || (inward(rule, ours) && inward(rule, theirs)) { ours.to_string() } else { "either".to_string() }
        };
        Ok(rules()?
            .iter()
            .map(|r| match unit1.iter().find(|u| u.pair == r.pair) {
                Some(u) => SwingRule {
                    opens_into: relax(r, &r.opens_into, &u.opens_into),
                    hinge: relax(r, &r.hinge, &u.hinge),
                    ..r.clone()
                },
                None => r.clone(),
            })
            .collect())
    }) {
        Ok(v) => Ok(v),
        Err(e) => Err(e.clone()),
    }
}

/// The judged rule for two room kinds, in either order.
pub fn judged_rule_for(a: &str, b: &str) -> Option<&'static SwingRule> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    judged_rules().ok()?.iter().find(|r| r.pair[0] == lo && r.pair[1] == hi)
}

/// The judged exterior rule.
pub fn judged_exterior() -> Option<&'static SwingRule> {
    judged_rules().ok()?.iter().find(|r| r.pair.iter().any(|k| k == "exterior"))
}

/// The rule for two room kinds, in either order.
pub fn rule_for(a: &str, b: &str) -> Option<&'static SwingRule> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    rules().ok()?.iter().find(|r| r.pair[0] == lo && r.pair[1] == hi)
}

/// The exterior object: the rule whose pair has `exterior` in it.
pub fn exterior_rule() -> Option<&'static SwingRule> {
    rules().ok()?.iter().find(|r| r.pair.iter().any(|k| k == "exterior"))
}

fn holds(r: &Rect, p: [f64; 2]) -> bool {
    p[0] > r.x0 as f64 && p[0] < r.x1 as f64 && p[1] > r.y0 as f64 && p[1] < r.y1 as f64
}

/// Sets each swing door's flips, and what the extractor will say of it, from the rules. A door whose host wall is not found
/// exactly once is an error, so the try is skipped.
pub fn orient(g: &HouseGraph, l: &mut Layout) -> Result<(), String> {
    let exterior = exterior_rule();
    for i in 0..l.doors.len() {
        let at = [l.doors[i].at[0] as f64, l.doors[i].at[1] as f64];
        let hosts: Vec<_> = l
            .walls
            .iter()
            .filter(|w| {
                let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = dx.hypot(dy);
                if len < 1.0 {
                    return false;
                }
                let s = ((at[0] - a[0]) * dx + (at[1] - a[1]) * dy) / len;
                let d = ((at[0] - a[0]) * dy - (at[1] - a[1]) * dx).abs() / len;
                d <= 1.0 && s >= 0.0 && s <= len
            })
            .collect();
        let swings = l.doors[i].op == "swing";
        let [w] = hosts.as_slice() else {
            if swings {
                return Err(format!("door D{} has {} host walls under it, not one", i + 1, hosts.len()));
            }
            continue;
        };
        let (a, b) = ([w.a[0] as f64, w.a[1] as f64], [w.b[0] as f64, w.b[1] as f64]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        let half = if w.exterior { 150.0 } else { 50.0 };
        l.doors[i].normal = n;
        l.doors[i].half = half;
        if !swings {
            l.doors[i].opens_into = None;
            l.doors[i].hinge = None;
            continue;
        }
        let k = half + 150.0;
        let room_at = |sign: f64| {
            let p = [at[0] + sign * n[0] * k, at[1] + sign * n[1] * k];
            l.rects.iter().position(|r| holds(r, p))
        };
        let (left, right) = (room_at(1.0), room_at(-1.0));
        // The default is the left side and the start jamb; a rule may ask for the other. The target is the room (None:
        // outside) the door should open into, when a rule enforces one.
        let mut hinge_end = false;
        let mut target: Option<Option<usize>> = None;
        match (left, right) {
            (Some(x), Some(y)) if x != y => {
                let (kx, ky) = (g.rooms[x].kind.name(), g.rooms[y].kind.name());
                if let Some(r) = rule_for(kx, ky) {
                    hinge_end = r.hinge == "end";
                    let rk = r.opens_into.as_str();
                    if kx != ky && rk == kx {
                        target = Some(Some(x));
                    } else if kx != ky && rk == ky {
                        target = Some(Some(y));
                    }
                }
            }
            (Some(x), None) | (None, Some(x)) => {
                if let Some(r) = exterior {
                    hinge_end = r.hinge == "end";
                    target = match r.opens_into.as_str() {
                        "either" => None,
                        "exterior" => Some(None),
                        _ => Some(Some(x)),
                    };
                }
            }
            _ => {}
        }
        let face = target.is_some_and(|t| t != left);
        let side = if face { right } else { left };
        let d = &mut l.doors[i];
        d.flip_facing = face;
        d.flip_hand = hinge_end;
        d.opens_into = Some(side.map_or("exterior".to_string(), |r| g.rooms[r].id.clone()));
        d.hinge = Some(if hinge_end { "end" } else { "start" });
    }
    Ok(())
}

/// How far a tag's text reaches from the point it names, mm: a circle is cruder than its box and safe.
const TAG_REACH: f64 = 380.0;

/// A door tag the default offset puts under a swung leaf, moved: `(the door's index, the new offset)`. A tag the leaf of a
/// door nearby would cover is hidden by its mask and clips the leaf, so each is tried on either side of its wall at growing
/// distances, and the first that keeps clear of every leaf is taken.
pub fn tag_moves(l: &Layout) -> Vec<(usize, [f64; 2])> {
    let doors = &l.doors;
    // Each swing leaf as a box in its wall's frame: along the wall, and out from its centre line on the swing side.
    let leaf_hit = |j: usize, p: [f64; 2]| {
        let d = &doors[j];
        if d.op != "swing" || d.normal == [0.0, 0.0] {
            return false;
        }
        let u = [d.normal[1], -d.normal[0]];
        let (dx, dy) = (p[0] - d.at[0] as f64, p[1] - d.at[1] as f64);
        let (along, out) = (dx * u[0] + dy * u[1], dx * d.normal[0] + dy * d.normal[1]);
        let w = d.width as f64;
        let side = if d.flip_facing { -1.0 } else { 1.0 };
        let out = out * side;
        let (ax, ay) = ((along.abs() - w / 2.0).max(0.0), (-out).max(out - (d.half + w)).max(0.0));
        ax.hypot(ay) < TAG_REACH
    };
    let mut moves = Vec::new();
    for (i, d) in doors.iter().enumerate() {
        if d.normal == [0.0, 0.0] {
            continue;
        }
        // The default: the side opposite the leaf for a door, 400 mm past the face.
        let first = if d.flip_facing { 1.0 } else { -1.0 };
        let u = [d.normal[1], -d.normal[0]];
        let at = |sign: f64, far: f64, shift: f64| {
            let k = sign * (d.half + far);
            [d.at[0] as f64 + d.normal[0] * k + u[0] * shift, d.at[1] as f64 + d.normal[1] * k + u[1] * shift]
        };
        let clear = |sign: f64, far: f64, shift: f64| !(0..doors.len()).any(|j| leaf_hit(j, at(sign, far, shift)));
        if clear(first, 400.0, 0.0) {
            continue;
        }
        // Nearest first: the same side, then the other, farther out and farther along the wall.
        let mut found = None;
        'search: for far in [400.0, 700.0, 1000.0] {
            for shift in [0.0, 600.0, -600.0, 900.0, -900.0] {
                for sign in [first, -first] {
                    if clear(sign, far, shift) {
                        found = Some((sign, far, shift));
                        break 'search;
                    }
                }
            }
        }
        if let Some((sign, far, shift)) = found {
            let k = sign * (d.half + far);
            moves.push((i, [(d.normal[0] * k + u[0] * shift).round(), (d.normal[1] * k + u[1] * shift).round()]));
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_parses_with_stable_exploratory_ids() {
        let r = rules().expect("parses");
        let ids: Vec<&str> = r.iter().map(|r| r.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert!(ids.iter().all(|i| i.strip_prefix("SW-").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))));
        assert!(r.iter().all(|r| r.status == "exploratory"));
    }

    #[test]
    fn the_four_rules_are_the_traced_houses() {
        let got: Vec<(&str, &str, &str, &str, &str)> = rules()
            .expect("parses")
            .iter()
            .map(|r| (r.id.as_str(), r.pair[0].as_str(), r.pair[1].as_str(), r.opens_into.as_str(), r.hinge.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("SW-001", "bath", "hall", "bath", "start"),
                ("SW-002", "bedroom", "hall", "bedroom", "start"),
                ("SW-003", "exterior", "kitchen", "kitchen", "end"),
                ("SW-004", "hall", "living", "hall", "end"),
            ]
        );
        assert_eq!(rule_for("hall", "bedroom").map(|r| r.id.as_str()), Some("SW-002"));
        assert!(rule_for("bath", "bedroom").is_none());
        assert_eq!(exterior_rule().map(|r| r.id.as_str()), Some("SW-003"));
    }
}
