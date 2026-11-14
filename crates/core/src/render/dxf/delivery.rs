//! The delivery drawing rules a plan's DXF keeps under the `delivery` CAD profile: each room's outline closed on
//! `A-AREA-BNDY` (the room separation lines are not drawn: the outlines of the rooms they part meet along them), a door's
//! swing an `ARC` rather than the chords it is drawn as in the picture, and a block's attributes only its `MARK` (in
//! [`super::blocks`]).

use std::collections::BTreeSet;

use super::*;
use crate::family::Mark;
use crate::rooms::Rooms;

/// The layer a room's outline goes on.
const OUTLINE_LAYER: &str = "A-AREA-BNDY";

/// How far a stroke's points may lie from an arc's chords, mm, and still be that swing: the plan is in pixels of `f32`.
const NEAR: f64 = 1.0;

/// An outline's pen: hundredths of a mm, and its colour.
const OUTLINE_WEIGHT: i32 = 18;
const OUTLINE_COLOR: Rgb = Rgb(0x19, 0x19, 0x19);

/// The outline of each room the drawing letters or draws, closed, written to `ents` in model space; the drawing's extents grown
/// by them. A room that isn't enclosed has none. `shift` is how far the drawing has been moved.
pub(super) fn outlines(d: &Drawing, m: &Model, shift: P2, sh: &mut Shared, ents: &mut String, mut ext: Option<[P2; 2]>) -> Option<[P2; 2]> {
    let mut rooms: BTreeSet<Id> = d.lettering.iter().filter_map(|l| l.owner).collect();
    rooms.extend(d.owners.iter().flatten().copied());
    let found = Rooms::new(m);
    for id in rooms {
        let Some(Element::Room(room)) = m.elements.get(id) else { continue };
        let Some(region) = found.region(room) else { continue };
        let mut pts: Vec<P2> = region.outer.iter().map(|p| P2(p.0 + shift.0, p.1 + shift.1)).collect();
        if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) < 0.01) {
            pts.pop();
        }
        if pts.len() < 3 || !finite(&pts) {
            continue;
        }
        let ink = Ink {
            layer: OUTLINE_LAYER.to_string(),
            ltype: None,
            color: OUTLINE_COLOR,
            shape: Shape::Line { pts, closed: true, weight: OUTLINE_WEIGHT },
        };
        ext = ink.grow(ext);
        sh.layers.insert(ink.layer.clone());
        put(ents, sh.w.handle(), MODEL_SPACE, &ink);
    }
    ext
}

/// The arcs of door or window `owner`'s swing, as its centre and the chords it is drawn as, in the model's axes.
pub(super) fn swings(m: &Model, owner: Id) -> Vec<(P2, Vec<P2>)> {
    let Some(e @ (Element::Door(_) | Element::Window(_))) = m.elements.get(owner) else { return Vec::new() };
    m.opening_marks(e)
        .into_iter()
        .filter_map(|mark| match mark {
            Mark::Arc { centre, pts } if pts.len() >= 2 => Some((centre, pts)),
            _ => None,
        })
        .collect()
}

/// `ink` as the arc it is the chords of, if one of `arcs` (moved by `shift` like the drawing) is: counter-clockwise from the
/// angle of its first end to its last, or the other way round for one drawn clockwise (a mirrored door).
pub(super) fn as_arc(ink: &Ink, arcs: &[(P2, Vec<P2>)], shift: P2) -> Option<Ink> {
    let Shape::Line { pts, closed: false, weight } = &ink.shape else { return None };
    arcs.iter().find_map(|(centre, chords)| {
        let moved = |p: &P2| P2(p.0 + shift.0, p.1 + shift.1);
        if chords.len() != pts.len() || !chords.iter().zip(pts).all(|(c, p)| moved(c).dist(*p) <= NEAR) {
            return None;
        }
        let c = moved(centre);
        let (a, b, z) = (pts[0] - c, pts[1] - c, pts[pts.len() - 1] - c);
        let radius = a.0.hypot(a.1);
        if !(radius.is_finite() && radius > 0.0) {
            return None;
        }
        let angle = |v: P2| v.1.atan2(v.0).to_degrees().rem_euclid(360.0);
        let turns_left = a.0 * b.1 - a.1 * b.0 > 0.0;
        let (from, to) = if turns_left { (angle(a), angle(z)) } else { (angle(z), angle(a)) };
        Some(Ink {
            layer: ink.layer.clone(),
            ltype: ink.ltype.clone(),
            color: ink.color,
            shape: Shape::Arc { centre: c, radius, from, to, weight: *weight },
        })
    })
}
