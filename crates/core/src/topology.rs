//! The topology extractor: a model's 2D topology graph from its geometry alone, in the shape of `topology-graph.json`.
//!
//! Spaces are the model's enclosed rooms. A door joins the rooms on either side of its host wall; a room separation line
//! joins the rooms it parts and a wall with no door in it makes them adjacent; one room only beside a door makes it an
//! exterior door and gives the entry. Nothing is guessed: a room no walls enclose, or a door with no room beside it, is a
//! typed diagnostic. Sides are found by sampling points either side of a line and asking which room holds each.

mod expect;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::geom::{self, P2, Region};
use crate::ids::Id;
use crate::model::{Element, Guid, Model, Operation, Room, RoomArea, TOL, Unbound, region_holds};
use crate::rooms::Rooms;
use crate::walls::{self, Axis};

/// The least length two rooms must share, mm, for an open or adjacent edge: a stub of separation line touching a third room
/// is not a link.
pub const MIN_SHARED: f64 = 300.0;
/// How far past a door's wall face its two probe points lie, mm.
pub const DOOR_PROBE: f64 = 150.0;
/// How far past a wall's face (adjacent edges) the probe points lie, mm.
const WALL_PROBE: f64 = 100.0;
/// How far either side of a separation line the probe points lie, mm.
const SEPARATOR_PROBE: f64 = 50.0;
/// The distance between samples along a line, mm.
pub const SAMPLE_STEP: f64 = 100.0;
/// The most samples along one line.
pub const MAX_SAMPLES: usize = 500;

/// A room that has a boundary.
struct Space {
    id: String,
    level: Guid,
    region: Region,
    /// min x, min y, max x, max y of the outer ring.
    bbox: [f64; 4],
}

struct Spaces<'a> {
    m: &'a Model,
    list: Vec<Space>,
}

impl Spaces<'_> {
    /// The space on `level` whose region holds `p`.
    fn room_at(&self, level: Guid, p: P2) -> Option<usize> {
        if !p.is_finite() {
            return None;
        }
        self.list.iter().position(|s| {
            p.0 >= s.bbox[0]
                && p.0 <= s.bbox[2]
                && p.1 >= s.bbox[1]
                && p.1 <= s.bbox[3]
                && self.m.same(s.level, level)
                && region_holds(&s.region, p)
        })
    }

    /// The spaces either side of a line `a` to `b` at `off` from it: `at(s)` is the point `s` along it, `side(s, ±off)`.
    fn shared(&self, level: Guid, len: f64, off: f64, side: impl Fn(f64, f64) -> P2) -> BTreeMap<(usize, usize), f64> {
        let mut out = BTreeMap::new();
        if !(len.is_finite() && len > 0.0) {
            return out;
        }
        let n = ((len / SAMPLE_STEP).ceil() as usize).clamp(1, MAX_SAMPLES);
        let step = len / n as f64;
        for i in 0..n {
            let s = (i as f64 + 0.5) * step;
            if let (Some(a), Some(b)) = (self.room_at(level, side(s, off)), self.room_at(level, side(s, -off)))
                && a != b
            {
                *out.entry((a.min(b), a.max(b))).or_insert(0.0) += step;
            }
        }
        out
    }
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn ring_json(r: &[P2]) -> Value {
    Value::Array(r.iter().map(|p| json!([round(p.0), round(p.1)])).collect())
}

fn diagnostic(code: &str, extra: Value) -> Value {
    let mut d = extra;
    d["code"] = json!(code);
    d
}

/// The model's topology graph.
pub fn extract(m: &Model) -> Value {
    let mut diagnostics: Vec<Value> = Vec::new();
    let rooms = Rooms::new(m);
    let mut placed: Vec<(Id, &Room)> =
        m.elements.iter().filter_map(|(id, e)| if let Element::Room(r) = e { Some((*id, r)) } else { None }).collect();
    placed.sort_by_key(|(id, _)| *id);

    // Ids: the room number when set and unique, else room-<element id>.
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, r) in &placed {
        *count.entry(r.number.as_str()).or_insert(0) += 1;
    }
    let mut list = Vec::new();
    let mut nodes = Vec::new();
    for (id, r) in &placed {
        let number = if r.number.trim().is_empty() { None } else { Some(r.number.as_str()) };
        match rooms.bound(r, m.room_area) {
            Ok(region) => {
                let name = match number {
                    Some(n) if count.get(n) == Some(&1) => n.to_string(),
                    _ => format!("room-{id}"),
                };
                let bbox = region
                    .outer
                    .iter()
                    .fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]);
                nodes.push(json!({
                    "id": name, "number": r.number, "element_id": id, "guid": m.guid(*id).to_string(), "name": r.name,
                    "level": m.level(r.level).map(|l| l.name.clone()).unwrap_or_default(),
                    "boundary": ring_json(&region.outer),
                    "holes": region.holes.iter().map(|h| ring_json(h)).collect::<Vec<_>>(),
                    "area_m2": round(region.area() / 1e6),
                }));
                list.push(Space { id: name, level: r.level, region, bbox });
            }
            Err(why) => {
                let (code, message) = match why {
                    Unbound::NotEnclosed => (
                        "room_not_enclosed",
                        format!(
                            "room `{}` ({}) has no loop of walls or room separation lines round its point: close it, or move the point",
                            r.number, r.name
                        ),
                    ),
                    Unbound::Shared(other) => (
                        "room_shares_space",
                        format!("room `{}` ({}) is in the space room {other} has: move its point or separate them", r.number, r.name),
                    ),
                };
                diagnostics.push(diagnostic(code, json!({"room": r.number, "element_id": id, "message": message})));
            }
        }
    }
    let spaces = Spaces { m, list };

    let (mut edges, exterior, access) = door_edges(m, &spaces, &mut diagnostics);
    let mut linked: BTreeSet<(usize, usize)> = access;
    for ((a, b), len) in separator_edges(m, &spaces) {
        if len >= MIN_SHARED {
            edges.push(json!({"from": spaces.list[a].id, "to": spaces.list[b].id, "kind": "open", "length_mm": round(len)}));
            linked.insert((a, b));
        }
    }
    for ((a, b), len) in wall_adjacency(m, &spaces) {
        if len >= MIN_SHARED && !linked.contains(&(a, b)) {
            edges.push(json!({"from": spaces.list[a].id, "to": spaces.list[b].id, "kind": "adjacent", "length_mm": round(len)}));
        }
    }
    let key = |e: &Value| {
        let kind = match e["kind"].as_str() {
            Some("door") => 0,
            Some("open") => 1,
            _ => 2,
        };
        (kind, e["from"].as_str().unwrap_or("").to_string(), e["to"].as_str().unwrap_or("").to_string(), e["door_id"].as_u64().unwrap_or(0))
    };
    edges.sort_by_key(key);

    let entry = entry_of(&exterior);
    diagnostics.sort_by_key(|d| (d["code"].as_str().unwrap_or("").to_string(), d["element_id"].as_u64().unwrap_or(0)));
    json!({
        "schema_version": crate::generate::graph::SCHEMA_VERSION,
        "graph_kind": "extracted",
        "source": m.name,
        "nodes": nodes,
        "edges": edges,
        "entry": entry,
        "exterior_doors": exterior.iter().map(|d| d.json.clone()).collect::<Vec<_>>(),
        "footprint": footprint(m, &spaces),
        "diagnostics": diagnostics,
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    path: Option<String>,
    expect: Option<String>,
}

const MAX_PATH: usize = 4096;

fn path_ok(what: &str, path: &str) -> Result<(), String> {
    if path.trim().is_empty() || path.len() > MAX_PATH || path.contains('\0') {
        return Err(format!("`{what}` is a file: not empty, at most {MAX_PATH} bytes"));
    }
    Ok(())
}

/// The `topology` command: the graph, with `path` the same written there as JSON, and with `expect` its comparison with a traced
/// plan's expected file (`comparison`).
pub fn topology(s: &mut crate::engine::Session, p: &Value) -> Result<Value, String> {
    let given: Params = serde_json::from_value(p.clone()).map_err(|e| {
        let why: String = e.to_string().chars().take(200).collect();
        format!("{why}; `topology` takes `path?`, a file to write the graph to, and `expect?`, an expected file to compare it with")
    })?;
    if let Some(path) = &given.path {
        path_ok("path", path)?;
    }
    if let Some(path) = &given.expect {
        path_ok("expect", path)?;
    }
    let mut doc = extract(&s.model);
    if let Some(path) = given.path {
        let text = serde_json::to_string_pretty(&doc).map_err(|e| format!("{path}: {e}"))?;
        std::fs::write(&path, text + "\n").map_err(|e| format!("{path}: {e}"))?;
        doc["path"] = json!(path);
    }
    if let Some(path) = given.expect {
        let mut c = expect::compare(&doc, &expect::read_expected(&path)?);
        c["expect"] = json!(path);
        doc["comparison"] = c;
    }
    Ok(doc)
}

/// A door with a room on one side only.
struct Exterior {
    room: String,
    door: Id,
    named_entry: bool,
    swing: bool,
    width: f64,
    json: Value,
}

type DoorEdges = (Vec<Value>, Vec<Exterior>, BTreeSet<(usize, usize)>);

fn door_edges(m: &Model, spaces: &Spaces, diagnostics: &mut Vec<Value>) -> DoorEdges {
    let (mut edges, mut exterior, mut access) = (Vec::new(), Vec::new(), BTreeSet::new());
    let mut doors: Vec<(Id, &Element)> = m.elements.iter().filter(|(_, e)| matches!(e, Element::Door(_))).map(|(id, e)| (*id, e)).collect();
    doors.sort_by_key(|(id, _)| *id);
    for (id, e) in doors {
        let Element::Door(o) = e else { continue };
        let Some(w) = m.wall(o.host) else { continue };
        let dims = match m.opening_dims(e) {
            Ok(d) => d,
            Err(why) => {
                diagnostics.push(diagnostic("door_type_unknown", json!({"element_id": id, "mark": o.mark, "message": why})));
                continue;
            }
        };
        let axis = Axis::of(w);
        let (c, t) = (walls::centre(m, w), m.thickness(w));
        let reach = t / 2.0 + DOOR_PROBE;
        let at = |side: f64| spaces.room_at(w.level, axis.at(o.offset, c + side));
        let (left, right) = (at(reach), at(-reach));
        let operation = dims.operation.unwrap_or(Operation::Swing);
        let mut fields = json!({
            "door_id": id, "door_guid": m.guid(id).to_string(), "mark": o.mark,
            "family": o.family.clone().unwrap_or_else(|| o.type_name.clone()), "type": o.type_name,
            "operation": operation.name(), "width_mm": round(dims.width),
        });
        // A swing door records the room on its swing side (the wall's left, `flip_facing` the right; `exterior` when no room
        // is there) and the jamb its leaf hangs from. Any other operation leaves both keys out.
        if matches!(operation, Operation::Swing | Operation::DoubleSwing) {
            let side = if o.flip_facing { right } else { left };
            fields["opens_into"] = json!(side.map_or("exterior", |i| spaces.list[i].id.as_str()));
            fields["hinge"] = json!(if o.flip_hand { "end" } else { "start" });
        }
        let with = |mut base: Value| {
            if let (Some(b), Some(f)) = (base.as_object_mut(), fields.as_object()) {
                b.extend(f.clone());
            }
            base
        };
        match (left, right) {
            (Some(a), Some(b)) if a != b => {
                let (a, b) = (a.min(b), a.max(b));
                access.insert((a, b));
                edges.push(with(json!({"from": spaces.list[a].id, "to": spaces.list[b].id, "kind": "door"})));
            }
            (Some(a), None) | (None, Some(a)) => {
                let edge = with(json!({"from": spaces.list[a].id, "to": "exterior", "kind": "door", "exterior": true}));
                exterior.push(Exterior {
                    room: spaces.list[a].id.clone(),
                    door: id,
                    named_entry: [o.type_name.as_str(), o.family.as_deref().unwrap_or_default()]
                        .iter()
                        .any(|n| n.to_lowercase().contains("entry")),
                    swing: matches!(operation, Operation::Swing | Operation::DoubleSwing),
                    width: dims.width,
                    json: edge.clone(),
                });
                edges.push(edge);
            }
            (None, None) => diagnostics.push(diagnostic(
                "door_without_room",
                json!({"element_id": id, "mark": o.mark, "message": "no room on either side of this door: place rooms on both sides, or check its host wall"}),
            )),
            // The same room both sides: a door in a wall inside one room links nothing.
            _ => {}
        }
    }
    (edges, exterior, access)
}

/// The entry: the room an exterior door opens into. Of several, a door whose type or family is named an entry first (a "Main Entry"
/// says what it is for, where a service exit may be the wider), then a swing door (single or double), then the widest, then the
/// lowest id.
fn entry_of(exterior: &[Exterior]) -> Value {
    let best = exterior.iter().min_by(|a, b| {
        b.named_entry.cmp(&a.named_entry).then(b.swing.cmp(&a.swing)).then(b.width.total_cmp(&a.width)).then(a.door.cmp(&b.door))
    });
    best.map_or(Value::Null, |e| json!({"room": e.room, "door_id": e.door}))
}

/// What each pair of rooms shares along room separation lines.
fn separator_edges(m: &Model, spaces: &Spaces) -> BTreeMap<(usize, usize), f64> {
    let mut out = BTreeMap::new();
    for e in m.elements.values() {
        let Element::RoomSeparator(rs) = e else { continue };
        let d = rs.end - rs.start;
        let len = d.length();
        if !(len.is_finite() && len > 0.0) {
            continue;
        }
        let n = d.perp() * (1.0 / len);
        for (k, v) in spaces.shared(rs.level, len, SEPARATOR_PROBE, |s, off| rs.start + d * (s / len) + n * off) {
            *out.entry(k).or_insert(0.0) += v;
        }
    }
    out
}

/// What each pair of rooms shares along walls.
fn wall_adjacency(m: &Model, spaces: &Spaces) -> BTreeMap<(usize, usize), f64> {
    let mut out = BTreeMap::new();
    for e in m.elements.values() {
        let Element::Wall(w) = e else { continue };
        if !w.room_bounding {
            continue;
        }
        let axis = Axis::of(w);
        let (c, t) = (walls::centre(m, w), m.thickness(w));
        let reach = t / 2.0 + WALL_PROBE;
        for (k, v) in spaces.shared(w.level, axis.length(), reach, |s, off| axis.at(s, c + off)) {
            *out.entry(k).or_insert(0.0) += v;
        }
    }
    out
}

/// The shell's outline on the lowest level that has rooms: the largest outline of the level's room-bounding lines at the wall
/// centres, counter-clockwise, its dangling stubs trimmed and only its corners kept.
fn footprint(m: &Model, spaces: &Spaces) -> Value {
    let Some(level) = spaces.list.iter().map(|s| s.level).min_by(|a, b| m.elevation(*a).total_cmp(&m.elevation(*b))) else {
        return Value::Null;
    };
    let lines = m.room_lines_in(level, RoomArea::Centre, None);
    let faces = geom::faces(&lines, TOL);
    let Some(outline) = faces.iter().filter(|f| geom::area(f) < 0.0).min_by(|a, b| geom::area(a).total_cmp(&geom::area(b))) else {
        return Value::Null;
    };
    let mut ring: Vec<P2> = outline.iter().rev().copied().collect();
    // A dangling stub is walked out and back: drop the tip and the duplicate it leaves, until none is left.
    loop {
        let n = ring.len();
        if n < 3 {
            break;
        }
        let spike = (0..n).find(|&i| ring[(i + n - 1) % n].dist(ring[(i + 1) % n]) <= TOL);
        match spike {
            Some(i) => {
                ring.remove(i);
                let n = ring.len();
                let j = (i + n - 1) % n;
                if ring[j].dist(ring[i % n]) <= TOL {
                    ring.remove(i % n);
                }
            }
            None => break,
        }
    }
    ring.dedup_by(|a, b| a.dist(*b) <= TOL);
    let ring = geom::corners(&ring);
    if ring.len() < 3 {
        return Value::Null;
    }
    let area = geom::area(&ring).abs();
    let bbox =
        ring.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]);
    let box_area = (bbox[2] - bbox[0]) * (bbox[3] - bbox[1]);
    json!({
        "outline": ring_json(&ring),
        "area_m2": round(area / 1e6),
        "area_over_bbox": if box_area > 0.0 { (area / box_area * 1e6).round() / 1e6 } else { 0.0 },
        "bbox": bbox.iter().map(|v| round(*v)).collect::<Vec<_>>(),
    })
}
