//! Door swings judged on the built model, as a receiver reads the drawing: each swing door's arc (`Model::opening_marks`) sweeps
//! floor inside the rooms BIMCraft measures, and no two doors sweep the same floor. The plans are seeds a delivery once drew
//! wrong: a closet door swinging out through the closet's far wall, and doors whose swings met.

use super::{Interior, Request, build};
use crate::engine::Session;
use crate::family::Mark;
use crate::geom::{P2, contains};
use crate::model::Element;
use crate::rooms::Rooms;

/// The grid the swept floor is counted on, mm, and how much of it may lie out or be shared before it counts (0.02 m2).
const STEP: f64 = 50.0;
const ALLOWED: f64 = 20_000.0;

fn request(archetype: &str, seed: u64, rooms: i64, shape: &str, interior: Interior, bumps: bool) -> Request {
    let mut r = Request::new(archetype.into(), seed, rooms, 0, Some(shape.into()), false);
    (r.interior, r.bumps) = (interior, bumps);
    r
}

/// The model a request builds, from its script.
fn model(req: &Request) -> Session {
    let built = build(req, false).unwrap_or_else(|r| panic!("{r}"));
    let script = built.files.iter().find(|(n, _)| *n == "script.bimcraft.txt").map(|(_, t)| t.clone()).expect("a script");
    let mut s = Session::new();
    s.script(&script).expect("the script runs");
    s
}

/// Each swing's sector, as its centre and the chords of its arc.
fn sectors(s: &Session) -> Vec<(String, Vec<P2>)> {
    let m = &s.model;
    let mut out = Vec::new();
    for (_, e) in m.elements.iter() {
        let Element::Door(d) = e else { continue };
        for mark in m.opening_marks(e) {
            if let Mark::Arc { centre, pts } = mark {
                let mut poly = vec![centre];
                poly.extend(pts);
                out.push((d.mark.clone(), poly));
            }
        }
    }
    out
}

/// The grid points strictly inside a sector.
fn swept(poly: &[P2]) -> Vec<P2> {
    let (mut lo, mut hi) = (P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in poly {
        (lo.0, lo.1, hi.0, hi.1) = (lo.0.min(p.0), lo.1.min(p.1), hi.0.max(p.0), hi.1.max(p.1));
    }
    let mut out = Vec::new();
    let mut x = (lo.0 / STEP).ceil() * STEP;
    while x <= hi.0 {
        let mut y = (lo.1 / STEP).ceil() * STEP;
        while y <= hi.1 {
            let p = P2(x + STEP / 2.0, y + STEP / 2.0);
            if contains(poly, p) {
                out.push(p);
            }
            y += STEP;
        }
        x += STEP;
    }
    out
}

/// What is wrong with the plan's swings: floor a swing sweeps outside every room, and floor two swings both sweep.
fn trouble(s: &Session) -> Vec<String> {
    let rooms = Rooms::new(&s.model);
    let outlines: Vec<Vec<P2>> = s
        .model
        .elements
        .iter()
        .filter_map(|(_, e)| if let Element::Room(r) = e { rooms.region(r).map(|g| g.outer) } else { None })
        .collect();
    let cell = STEP * STEP;
    let all = sectors(s);
    let mut out = Vec::new();
    for (mark, poly) in &all {
        let off = swept(poly).iter().filter(|p| !outlines.iter().any(|o| contains(o, **p))).count() as f64 * cell;
        if off > ALLOWED {
            out.push(format!("{mark} swings {:.2} m2 outside the rooms", off / 1e6));
        }
    }
    for (i, (a, pa)) in all.iter().enumerate() {
        for (b, pb) in &all[i + 1..] {
            if a == b {
                continue;
            }
            let shared = swept(pa).iter().filter(|p| contains(pb, **p)).count() as f64 * cell;
            if shared > ALLOWED {
                out.push(format!("{a} and {b} sweep {:.2} m2 of the same floor", shared / 1e6));
            }
        }
    }
    out
}

#[test]
fn a_closet_door_swings_inside_the_rooms() {
    let s = model(&request("house", 82, 10, "narrow_bar", Interior::Jogged, true));
    assert_eq!(trouble(&s), Vec::<String>::new());
}

#[test]
fn no_two_doors_sweep_the_same_floor() {
    for req in [
        request("hospital", 2, 19, "corner_notch", Interior::Aligned, true),
        request("school", 38, 17, "side_nub", Interior::Aligned, true),
        request("office-pattern", 53, 24, "opposite_wings", Interior::Jogged, false),
        request("school", 54, 16, "corner_notch", Interior::Aligned, false),
    ] {
        let s = model(&req);
        assert_eq!(trouble(&s), Vec::<String>::new(), "{} seed {}", req.archetype, req.seed);
    }
}
