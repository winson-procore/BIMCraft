//! Room tags that do not fit. A room's tag (name, number, area; [`room_tag_box`]) is drawn in the room when its box lies inside the
//! room's cell, the largest rectangle of the room out to the wall centre lines, shrunk by 150 mm. When it does not, it is drawn
//! outside the plan, 1000 mm clear of the outline on the side nearest the room, with one straight leader to the centre of the
//! cell. This module decides who gets a leader and where the tag stands; the `room` command's `tag` says it to the drawing.
//!
//! The cell is found as the done check finds it: the room's outline as the topology extractor reads it back from the scratch
//! project, its largest rectangle, pushed out to the wall and separation centre lines.

use serde_json::Value;

use super::emit::{Drawn, names};
use super::family::Family;
use super::layout::Laid;
use super::links::outline;
use super::plan::Room;
use crate::render::{room_tag_box, room_tag_height, room_tag_leader};

/// Where a leader room's tag stands and where its leader ends (the room's point), whole mm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagAt {
    pub tag: [i64; 2],
    pub point: [i64; 2],
}

/// The scale the tags are laid out at: a plan is drawn at 1 : 100 unless it says otherwise.
pub const SCALE: f64 = 100.0;
/// The cell is shrunk by this on every side for the tag.
pub const MARGIN: f64 = 150.0;
/// A leader's tag stands at least this far from the outline.
pub const CLEAR: f64 = 1000.0;
/// Where the first row of tags stands from the outline, and the gap between tags.
const SET_BACK: f64 = 1100.0;
const GAP: f64 = 200.0;
const ROWS: usize = 8;
const STEP: f64 = 100.0;

type Pt = (f64, f64);
type Rect = [f64; 4];

fn inside(poly: &[Pt], x: f64, y: f64) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[(i + poly.len() - 1) % poly.len()], poly[i]);
        if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1) {
            c = !c;
        }
    }
    c
}

fn pseg(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = dx * dx + dy * dy;
    let t = if l == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l).clamp(0.0, 1.0) };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// Closed segments `ab` and `cd` meet.
fn seg_cross(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let o = |p: Pt, q: Pt, r: Pt| {
        let v = (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
        if v.abs() < 1e-6 {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    };
    let on = |p: Pt, q: Pt, r: Pt| {
        p.0.min(r.0) - 1e-6 <= q.0 && q.0 <= p.0.max(r.0) + 1e-6 && p.1.min(r.1) - 1e-6 <= q.1 && q.1 <= p.1.max(r.1) + 1e-6
    };
    let (o1, o2, o3, o4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    (o1 != o2 && o3 != o4) || (o1 == 0 && on(a, c, b)) || (o2 == 0 && on(a, d, b)) || (o3 == 0 && on(c, a, d)) || (o4 == 0 && on(c, b, d))
}

fn corners(r: Rect) -> [Pt; 4] {
    [(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])]
}

fn rect_edges(r: Rect) -> [(Pt, Pt); 4] {
    let c = corners(r);
    [(c[3], c[0]), (c[0], c[1]), (c[1], c[2]), (c[2], c[3])]
}

fn in_rect(p: Pt, r: Rect) -> bool {
    r[0] <= p.0 && p.0 <= r[2] && r[1] <= p.1 && p.1 <= r[3]
}

fn seg_rect(a: Pt, b: Pt, r: Rect) -> bool {
    in_rect(a, r) || in_rect(b, r) || rect_edges(r).iter().any(|(p, q)| seg_cross(a, b, *p, *q))
}

fn pt_rect(p: Pt, r: Rect) -> f64 {
    (r[0] - p.0).max(0.0).max(p.0 - r[2]).hypot((r[1] - p.1).max(0.0).max(p.1 - r[3]))
}

fn rect_seg(r: Rect, a: Pt, b: Pt) -> f64 {
    if seg_rect(a, b, r) {
        return 0.0;
    }
    corners(r).iter().map(|c| pseg(*c, a, b)).chain([pt_rect(a, r), pt_rect(b, r)]).fold(f64::MAX, f64::min)
}

fn overlap(r: Rect, s: Rect, tol: f64) -> bool {
    r[2].min(s[2]) - r[0].max(s[0]) > tol && r[3].min(s[3]) - r[1].max(s[1]) > tol
}

fn mid(r: Rect) -> Pt {
    ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0)
}

fn tenth(v: f64) -> f64 {
    (v * 10.0).round_ties_even() / 10.0
}

/// The largest axis-parallel rectangle inside a rectilinear polygon; of equal ones (to the mm²), the one holding `prefer`.
fn largest_rect(poly: &[Pt], prefer: Option<Pt>) -> Option<Rect> {
    let uniq = |f: fn(&Pt) -> f64| {
        let mut v: Vec<f64> = poly.iter().map(|p| tenth(f(p))).collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    };
    let (xs, ys) = (uniq(|p| p.0), uniq(|p| p.1));
    if xs.len() < 2 || ys.len() < 2 {
        return None;
    }
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    let ins: Vec<Vec<bool>> =
        (0..nx).map(|i| (0..ny).map(|j| inside(poly, (xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0)).collect()).collect();
    let mut best: Option<(Rect, (i64, bool))> = None;
    for i0 in 0..nx {
        for i1 in i0 + 1..=nx {
            for j0 in 0..ny {
                for j1 in j0 + 1..=ny {
                    if !(i0..i1).all(|i| (j0..j1).all(|j| ins[i][j])) {
                        continue;
                    }
                    let r = [xs[i0], ys[j0], xs[i1], ys[j1]];
                    let key = (((r[2] - r[0]) * (r[3] - r[1])).round_ties_even() as i64, prefer.is_some_and(|p| in_rect(p, r)));
                    if best.is_none_or(|(_, k)| key > k) {
                        best = Some((r, key));
                    }
                }
            }
        }
    }
    best.map(|(r, _)| r)
}

/// Each side of a finish-face rectangle pushed out to the nearest parallel wall or separation centre line within 200 mm that runs
/// along it.
fn expand(r: Rect, segs: &[(Pt, Pt)]) -> Rect {
    let search = |vertical: bool, coord: f64, lo: f64, hi: f64, sign: f64| {
        let mut best: Option<f64> = None;
        for (a, b) in segs {
            let (c, s0, s1) = if vertical && (a.0 - b.0).abs() < 0.5 {
                (a.0, a.1.min(b.1), a.1.max(b.1))
            } else if !vertical && (a.1 - b.1).abs() < 0.5 {
                (a.1, a.0.min(b.0), a.0.max(b.0))
            } else {
                continue;
            };
            let d = (c - coord) * sign;
            if (-0.5..=200.0).contains(&d) && hi.min(s1) - lo.max(s0) > 1.0 {
                best = Some(best.map_or(d, |x| x.min(d)));
            }
        }
        best.unwrap_or(0.0).max(0.0)
    };
    [
        r[0] - search(true, r[0], r[1], r[3], -1.0),
        r[1] - search(false, r[1], r[0], r[2], -1.0),
        r[2] + search(true, r[2], r[1], r[3], 1.0),
        r[3] + search(false, r[3], r[0], r[2], 1.0),
    ]
}

/// A room's cell: its largest rectangle, out to the wall centre lines.
pub fn cell(poly: &[Pt], prefer: Option<Pt>, segs: &[(Pt, Pt)]) -> Option<Rect> {
    largest_rect(poly, prefer).map(|r| expand(r, segs))
}

/// How much room is left round a tag whose box is `w` by `bh` in `cell`: negative when it does not fit.
pub fn fit_margin(cell: Rect, w: f64, bh: f64) -> f64 {
    ((cell[2] - cell[0]) - 2.0 * MARGIN - w).min((cell[3] - cell[1]) - 2.0 * MARGIN - bh)
}

/// The sides of a rectilinear outline, each with the way it faces: `E`, `W`, `N` or `S`.
fn outline_sides(outline: &[Pt]) -> Vec<(Pt, Pt, char)> {
    (0..outline.len())
        .map(|i| {
            let (a, b) = (outline[(i + outline.len() - 1) % outline.len()], outline[i]);
            let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let d = if a.0 == b.0 {
                if inside(outline, mx + 10.0, my) { 'W' } else { 'E' }
            } else if inside(outline, mx, my + 10.0) {
                'S'
            } else {
                'N'
            };
            (a, b, d)
        })
        .collect()
}

/// What stands on the plan already when a tag is placed: tags inside rooms, and the tags put outside with their leaders.
struct Board<'a> {
    outline: &'a [Pt],
    sides: &'a [(Pt, Pt, char)],
    fixed: &'a [Rect],
    placed: Vec<(Rect, (Pt, Pt))>,
}

impl Board<'_> {
    /// Whether a tag with box `b` and a leader from it to `end` may stand: the done check's tests, one by one.
    fn allows(&self, b: Rect, anchor: Pt, end: Pt, want: &[char]) -> Option<(Pt, Pt)> {
        let pts = corners(b).into_iter().chain([mid(b), anchor]);
        if pts.into_iter().any(|p| inside(self.outline, p.0, p.1)) {
            return None;
        }
        let near = |f: &dyn Fn(Pt, Pt) -> f64| {
            let d: Vec<(f64, char)> = self.sides.iter().map(|(p, q, c)| (f(*p, *q), *c)).collect();
            let least = d.iter().map(|x| x.0).fold(f64::MAX, f64::min);
            (least, d.iter().filter(|x| x.0 <= least + 1.0).map(|x| x.1).collect::<Vec<char>>())
        };
        let (least, got) = near(&|p, q| rect_seg(b, p, q));
        if least < CLEAR || !want.iter().any(|c| got.contains(c)) {
            return None;
        }
        if self.fixed.iter().any(|f| overlap(b, *f, 0.5)) || self.placed.iter().any(|(o, _)| overlap(b, grow(*o, GAP), 0.5)) {
            return None;
        }
        let (start, end) = room_tag_leader(b, crate::geom::P2(end.0, end.1));
        let (start, end) = ((start.0, start.1), (end.0, end.1));
        let others = self.fixed.iter().copied().chain(self.placed.iter().map(|(o, _)| *o));
        if others.into_iter().any(|o| seg_rect(start, end, o)) {
            return None;
        }
        if self.placed.iter().any(|(_, (p, q))| seg_cross(start, end, *p, *q) || seg_rect(*p, *q, b)) {
            return None;
        }
        Some((start, end))
    }
}

fn grow(r: Rect, by: f64) -> Rect {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}

/// Where the tag of a room with its cell centred on `cc` goes: the first place, on the rows facing the side of the outline
/// nearest the room, from the room's own distance along it and then sliding either way, where [`Board::allows`] says it may.
fn spot(board: &Board, cc: Pt, lines: &[String; 3], h: f64) -> Option<([i64; 2], (Pt, Pt))> {
    let sides = board.sides;
    let dist: Vec<f64> = sides.iter().map(|(a, b, _)| pseg(cc, *a, *b)).collect();
    let least = dist.iter().copied().fold(f64::MAX, f64::min);
    let want: Vec<char> = sides.iter().zip(&dist).filter(|(_, d)| **d <= least + 1.0).map(|((_, _, c), _)| *c).collect();
    let probe = room_tag_box(crate::geom::P2(0.0, 0.0), h, lines);
    let (w, bh) = (probe[2] - probe[0], probe[3] - probe[1]);
    // The box's middle stands this far over the anchor.
    let rise = mid(probe).1;
    // The sides nearest the room are where its tag may stand.
    for ((a, b, facing), _) in sides.iter().copied().zip(&dist).filter(|(_, d)| **d <= least + 1.0) {
        let vertical = a.0 == b.0;
        let (lo, hi) = if vertical { (a.1.min(b.1), a.1.max(b.1)) } else { (a.0.min(b.0), a.0.max(b.0)) };
        let start = if vertical { cc.1 } else { cc.0 }.clamp(lo, hi);
        let reach = (hi - lo) + w + bh;
        for row in 0..ROWS {
            let away = SET_BACK + row as f64 * (bh + GAP + 100.0);
            let steps = (reach / STEP) as usize + 1;
            for s in 0..=2 * steps {
                let along = start + if s % 2 == 0 { (s / 2) as f64 } else { -(s.div_ceil(2) as f64) } * STEP;
                let centre = match facing {
                    'N' => (along, a.1 + away + bh / 2.0),
                    'S' => (along, a.1 - away - bh / 2.0),
                    'E' => (a.0 + away + w / 2.0, along),
                    _ => (a.0 - away - w / 2.0, along),
                };
                let anchor = (centre.0.round(), (centre.1 - rise).round());
                let b4 = room_tag_box(crate::geom::P2(anchor.0, anchor.1), h, lines);
                if let Some(leader) = board.allows(b4, anchor, cc, &want) {
                    return Some(([anchor.0 as i64, anchor.1 as i64], leader));
                }
            }
        }
    }
    None
}

/// Which rooms' tags do not fit, and where each of those goes with its leader: none for the rooms that fit. `extracted` is the
/// topology extractor's reading of the plan's scratch project, whose room outlines and areas are what the drawing will have.
pub fn plan(
    fam: &Family,
    rooms: &[Room],
    laid: &Laid,
    d: &Drawn,
    extracted: &Value,
    points: &[[i64; 2]],
) -> Result<Vec<Option<TagAt>>, String> {
    let label = names(fam, rooms);
    let h = room_tag_height(SCALE);
    let nodes = extracted["nodes"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut segs: Vec<(Pt, Pt)> = d.walls.iter().map(|w| ((w.a[0] as f64, w.a[1] as f64), (w.b[0] as f64, w.b[1] as f64))).collect();
    for run in &d.separators {
        segs.extend(run.windows(2).map(|p| ((p[0][0] as f64, p[0][1] as f64), (p[1][0] as f64, p[1][1] as f64))));
    }
    let mut polys: Vec<Vec<Pt>> = Vec::new();
    let mut lines: Vec<[String; 3]> = Vec::new();
    for (name, number) in &label {
        let node = nodes.iter().find(|n| n["id"].as_str() == Some(number)).ok_or_else(|| format!("the room `{name}` was not read back"))?;
        let mut ring: Vec<Pt> =
            node["boundary"].as_array().into_iter().flatten().filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64()?))).collect();
        if ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        if ring.len() < 4 {
            return Err(format!("the room `{name}` has no outline to place its tag in"));
        }
        polys.push(ring);
        lines.push([name.clone(), number.clone(), format!("{:.2} m²", node["area_m2"].as_f64().unwrap_or(0.0))]);
    }
    let mut cells: Vec<Rect> = Vec::new();
    for (i, poly) in polys.iter().enumerate() {
        let at = points.get(i).map(|p| (p[0] as f64, p[1] as f64));
        cells.push(cell(poly, at, &segs).ok_or_else(|| format!("the room `{}` has no rectangle in it", label[i].0))?);
    }
    let size = |i: usize| {
        let b = room_tag_box(crate::geom::P2(0.0, 0.0), h, &lines[i]);
        (b[2] - b[0], b[3] - b[1])
    };
    let leader: Vec<usize> = (0..rooms.len()).filter(|i| fit_margin(cells[*i], size(*i).0, size(*i).1) < 0.0).collect();
    let mut out = vec![None; rooms.len()];
    if leader.is_empty() {
        return Ok(out);
    }
    let ring = outline(&laid.rects).map_err(|e| format!("no outline: {}", e.0))?;
    let ring: Vec<Pt> = ring.into_iter().map(|(x, y)| (x as f64, y as f64)).collect();
    let sides = outline_sides(&ring);
    // The leader ends at the centre of the room's cell, so the point is that: and the cell is looked for again with it as the point
    // the rooms with equal largest rectangles prefer.
    let mut ends: Vec<Pt> = Vec::new();
    let mut at: Vec<[i64; 2]> = Vec::new();
    for &i in &leader {
        let mut c = cells[i];
        let mut point = [mid(c).0.round() as i64, mid(c).1.round() as i64];
        for _ in 0..3 {
            c = cell(&polys[i], Some((point[0] as f64, point[1] as f64)), &segs).unwrap_or(c);
            let next = [mid(c).0.round() as i64, mid(c).1.round() as i64];
            if next == point {
                break;
            }
            point = next;
        }
        let m = mid(c);
        if (m.0 - point[0] as f64).abs() > 20.0
            || (m.1 - point[1] as f64).abs() > 20.0
            || !inside(&polys[i], point[0] as f64, point[1] as f64)
        {
            return Err(format!("the cell of room `{}` has no steady centre", label[i].0));
        }
        ends.push((point[0] as f64, point[1] as f64));
        at.push(point);
    }
    let fixed: Vec<Rect> = (0..rooms.len())
        .filter(|i| !leader.contains(i))
        .filter_map(|i| points.get(i).map(|p| room_tag_box(crate::geom::P2(p[0] as f64, p[1] as f64), h, &lines[i])))
        .collect();
    // The rooms in number order; if that order leaves one with no place, the other way round.
    for order in [(0..leader.len()).collect::<Vec<_>>(), (0..leader.len()).rev().collect::<Vec<_>>()] {
        let mut board = Board { outline: &ring, sides: &sides, fixed: &fixed, placed: Vec::new() };
        let mut got = Vec::new();
        let mut failed = None;
        for &k in &order {
            let i = leader[k];
            match spot(&board, ends[k], &lines[i], h) {
                Some((tag, l)) => {
                    let b = room_tag_box(crate::geom::P2(tag[0] as f64, tag[1] as f64), h, &lines[i]);
                    board.placed.push((b, l));
                    got.push((i, TagAt { tag, point: at[k] }));
                }
                None => {
                    failed = Some(label[i].0.clone());
                    break;
                }
            }
        }
        if failed.is_none() {
            for (i, t) in got {
                out[i] = Some(t);
            }
            return Ok(out);
        }
        if order.len() < 2 {
            return Err(format!("no place outside the plan for the tag of `{}`", failed.unwrap_or_default()));
        }
    }
    Err("no place outside the plan for every tag that does not fit".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Vec<Pt> {
        vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
    }

    #[test]
    fn a_closet_a_hall_and_a_room_that_fits() {
        let h = room_tag_height(100.0);
        let size = |lines: [&str; 3]| {
            let b = room_tag_box(crate::geom::P2(0.0, 0.0), h, &lines.map(String::from));
            (b[2] - b[0], b[3] - b[1])
        };
        let (w, bh) = size(["Closet 1", "108", "3.56 m²"]);
        assert!(fit_margin([0.0, 0.0, 3950.0, 900.0], w, bh) < 0.0);
        let (w, bh) = size(["Closet 2", "109", "2.64 m²"]);
        assert!(fit_margin([0.0, 0.0, 800.0, 3300.0], w, bh) < 0.0);
        let (w, bh) = size(["Hall", "104", "16.62 m²"]);
        assert!(fit_margin([0.0, 0.0, 13850.0, 1200.0], w, bh) < 0.0);
        let (w, bh) = size(["Living", "101", "9.00 m²"]);
        assert!(fit_margin([0.0, 0.0, 3000.0, 3000.0], w, bh) >= 0.0);
    }

    #[test]
    fn the_cell_is_the_largest_rectangle_out_to_the_centre_lines() {
        // A room 3800 x 2800 inside walls 3 m apart on both ways, 125 mm out on the west, 65 on the others.
        let poly = vec![(125.0, 65.0), (3935.0, 65.0), (3935.0, 2935.0), (125.0, 2935.0)];
        let segs = vec![
            ((0.0, 0.0), (0.0, 3000.0)),
            ((4000.0, 0.0), (4000.0, 3000.0)),
            ((0.0, 0.0), (4000.0, 0.0)),
            ((0.0, 3000.0), (4000.0, 3000.0)),
        ];
        assert_eq!(cell(&poly, None, &segs), Some([0.0, 0.0, 4000.0, 3000.0]));
        // An L: the larger arm, and of equal arms the one holding the preferred point.
        let l = vec![(0.0, 0.0), (2000.0, 0.0), (2000.0, 1000.0), (1000.0, 1000.0), (1000.0, 2000.0), (0.0, 2000.0)];
        assert_eq!(largest_rect(&l, Some((500.0, 1500.0))), Some([0.0, 0.0, 1000.0, 2000.0]));
        assert_eq!(largest_rect(&l, Some((1500.0, 500.0))), Some([0.0, 0.0, 2000.0, 1000.0]));
        assert_eq!(largest_rect(&rect(10.0, 10.0)[..2], None), None);
    }

    #[test]
    fn a_tag_stands_outside_a_square_plan_on_the_nearest_side_and_clear_of_it() {
        let ring = rect(6000.0, 6000.0);
        let sides = outline_sides(&ring);
        let board = Board { outline: &ring, sides: &sides, fixed: &[], placed: Vec::new() };
        let lines = ["Closet".to_string(), "101".to_string(), "1.50 m²".to_string()];
        let h = room_tag_height(100.0);
        let cc = (500.0, 3000.0);
        let (tag, (start, end)) = spot(&board, cc, &lines, h).expect("a place");
        let b = room_tag_box(crate::geom::P2(tag[0] as f64, tag[1] as f64), h, &lines);
        // West of the plan, 1000 mm or more clear of its west side.
        assert!(b[2] <= -CLEAR, "{b:?}");
        assert_eq!(end, cc);
        assert!((start.0 - b[2]).abs() < 1e-6, "{start:?} {b:?}");
        // A second tag, the same room's twin, is put clear of the first, and its leader crosses neither.
        let mut board = board;
        board.placed.push((b, (start, end)));
        let (t2, _) = spot(&board, (500.0, 3100.0), &lines, h).expect("another place");
        let b2 = room_tag_box(crate::geom::P2(t2[0] as f64, t2[1] as f64), h, &lines);
        assert!(!overlap(b, b2, 0.5), "{b:?} {b2:?}");
    }
}
