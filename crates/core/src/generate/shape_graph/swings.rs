//! Where a swing door's leaves sweep, as its family draws them: each leaf's arc is centred on its hinge, a frame width inside the
//! jamb on the face of the wall, and turns from along the wall to square off it, as long as the leaf. `flip_hand` mirrors it
//! along the wall and `flip_facing` across. A swing must lie in the room it opens into and clear of every other door's swing.

use super::links::Box4;

type P = [f64; 2];

/// The families' `Frame Width`: how far inside its jamb a leaf hangs.
const FRAME: f64 = 50.0;
/// Half an interior wall: how far a room's face lies inside its box.
const HALF_WALL: f64 = 65.0;
/// How far a swing may pass a face, or two swings meet, and still be clear, mm.
const SLACK: f64 = 1.0;
/// Chords an arc is taken as.
const CHORDS: usize = 12;

/// A door standing in a wall: its centre, the wall's direction, the wall's thickness and the door's width.
pub struct Placed {
    pub at: P,
    pub u: P,
    pub thickness: f64,
    pub width: f64,
}

/// The leaves of a swing family: each hinge's place along the wall from the middle (its own axes) and the leaf's length.
fn leaves(family: &str, w: f64) -> Vec<(f64, f64)> {
    let (left, right) = (-w / 2.0 + FRAME, w / 2.0 - FRAME);
    match family {
        "Double Door" => vec![(left, w / 2.0), (right, w / 2.0)],
        "Unequal Double Door" => vec![(left, w * 2.0 / 3.0), (right, w / 3.0)],
        _ => vec![(left, w)],
    }
}

/// Each leaf's swept quarter disc, as a convex polygon: the hinge, then the arc from along the wall to square off it.
pub fn sectors(family: &str, d: &Placed, flip_facing: bool, flip_hand: bool) -> Vec<Vec<P>> {
    let (fx, fy) = (if flip_hand { -1.0 } else { 1.0 }, if flip_facing { -1.0 } else { 1.0 });
    let n = [-d.u[1], d.u[0]];
    let place = |x: f64, y: f64| [d.at[0] + d.u[0] * fx * x + n[0] * fy * y, d.at[1] + d.u[1] * fx * x + n[1] * fy * y];
    let face = d.thickness / 2.0;
    leaves(family, d.width)
        .into_iter()
        .map(|(hinge, len)| {
            // A leaf on the left jamb turns toward +x, one on the right toward -x.
            let toward = if hinge < 0.0 { 1.0 } else { -1.0 };
            let mut poly = vec![place(hinge, face)];
            poly.extend((0..=CHORDS).map(|i| {
                let a = std::f64::consts::FRAC_PI_2 * i as f64 / CHORDS as f64;
                place(hinge + toward * len * a.cos(), face + len * a.sin())
            }));
            poly
        })
        .collect()
}

/// Whether `p` is in the room made of `boxes` (on the walls' centre lines), inside its faces: a box's side that another box of
/// the room continues across is no face.
fn in_room(boxes: &[Box4], p: P) -> bool {
    boxes.iter().any(|b| {
        let (x0, y0, x1, y1) = (b.0 as f64, b.1 as f64, (b.0 + b.2) as f64, (b.1 + b.3) as f64);
        let open = |side: usize| {
            boxes.iter().any(|o| {
                let (ox0, oy0, ox1, oy1) = (o.0 as f64, o.1 as f64, (o.0 + o.2) as f64, (o.1 + o.3) as f64);
                o != b
                    && match side {
                        0 => ox1 == x0 && oy0 <= p[1] && p[1] <= oy1,
                        1 => ox0 == x1 && oy0 <= p[1] && p[1] <= oy1,
                        2 => oy1 == y0 && ox0 <= p[0] && p[0] <= ox1,
                        _ => oy0 == y1 && ox0 <= p[0] && p[0] <= ox1,
                    }
            })
        };
        let inset = |side: usize| if open(side) { 0.0 } else { HALF_WALL - SLACK };
        p[0] >= x0 + inset(0) && p[0] <= x1 - inset(1) && p[1] >= y0 + inset(2) && p[1] <= y1 - inset(3)
    })
}

/// Whether every point of the swing is in the room.
pub fn inside(boxes: &[Box4], swing: &[Vec<P>]) -> bool {
    swing.iter().flatten().all(|p| in_room(boxes, *p))
}

/// Whether two convex polygons overlap by more than the slack (separating axes).
fn overlap(p: &[P], q: &[P]) -> bool {
    for poly in [p, q] {
        for (i, a) in poly.iter().enumerate() {
            let b = poly[(i + 1) % poly.len()];
            let axis = [a[1] - b[1], b[0] - a[0]];
            let len = axis[0].hypot(axis[1]);
            if len < 1e-9 {
                continue;
            }
            let span = |s: &[P]| {
                s.iter()
                    .map(|v| (v[0] * axis[0] + v[1] * axis[1]) / len)
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
            };
            let ((plo, phi), (qlo, qhi)) = (span(p), span(q));
            if phi <= qlo + SLACK || qhi <= plo + SLACK {
                return false;
            }
        }
    }
    true
}

/// Whether two doors' swings sweep the same floor.
pub fn clash(a: &[Vec<P>], b: &[Vec<P>]) -> bool {
    a.iter().any(|p| b.iter().any(|q| overlap(p, q)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door(at: P, u: P) -> Placed {
        Placed { at, u, thickness: 130.0, width: 900.0 }
    }

    #[test]
    fn a_single_door_sweeps_a_quarter_disc_from_its_hinge_on_the_face() {
        let s = sectors("Single Door", &door([0.0, 0.0], [1.0, 0.0]), false, false);
        assert_eq!(s.len(), 1);
        let p = &s[0];
        assert_eq!(p[0], [-400.0, 65.0]);
        assert!((p[1][0] - 500.0).abs() < 1e-9 && (p[1][1] - 65.0).abs() < 1e-9, "{:?}", p[1]);
        assert!((p[CHORDS + 1][0] + 400.0).abs() < 1e-9 && (p[CHORDS + 1][1] - 965.0).abs() < 1e-9, "{:?}", p[CHORDS + 1]);
        // Flipped: the hinge on the other jamb, the swing to the wall's other side.
        let f = sectors("Single Door", &door([0.0, 0.0], [1.0, 0.0]), true, true);
        assert_eq!(f[0][0], [400.0, -65.0]);
    }

    #[test]
    fn a_double_door_sweeps_two_half_discs_from_its_two_jambs() {
        let s = sectors("Double Door", &door([0.0, 0.0], [1.0, 0.0]), false, false);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0][0], s[1][0]), ([-400.0, 65.0], [400.0, 65.0]));
        // Each leaf reaches 450 square off the wall.
        assert!(s.iter().all(|p| (p[CHORDS + 1][1] - 515.0).abs() < 1e-9));
    }

    #[test]
    fn a_swing_past_a_rooms_face_is_outside_it() {
        // A closet 700 deep behind a wall along y = 0: a 900 door into it passes its back wall.
        let closet = [(-1000, 0, 2000, 700)];
        let s = sectors("Single Door", &door([0.0, 0.0], [1.0, 0.0]), false, false);
        assert!(!inside(&closet, &s));
        let room = [(-1000, 0, 2000, 1200)];
        assert!(inside(&room, &s));
        // The same room in two boxes: their seam is no face.
        let halves = [(-1000, 0, 1000, 1200), (0, 0, 1000, 1200)];
        assert!(inside(&halves, &s));
    }

    #[test]
    fn two_doors_facing_across_a_corner_clash() {
        let a = sectors("Single Door", &door([0.0, 0.0], [1.0, 0.0]), false, false);
        let b = sectors("Single Door", &door([-300.0, 500.0], [0.0, 1.0]), true, false);
        assert!(clash(&a, &b));
        let far = sectors("Single Door", &door([3000.0, 0.0], [1.0, 0.0]), false, false);
        assert!(!clash(&a, &far));
    }
}
