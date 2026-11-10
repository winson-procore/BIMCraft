//! The sample house's plan written as DXF and read back with the core's own reader: its layers, what each holds, its unit and
//! its extents.

use std::collections::BTreeMap;

use super::*;
use crate::dxf::{self, Color, Shape};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use crate::render::{Part, Prim, View, plan};

fn sample() -> (std::sync::Arc<Model>, Drawing) {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let m = crate::link::seen(&s.model);
    let level = m.levels()[0].0;
    let d = plan(&m, level, 1600, 1000);
    (m, d)
}

fn per_layer(d: &dxf::Dxf) -> BTreeMap<&str, usize> {
    let mut n = BTreeMap::new();
    for e in &d.entities {
        *n.entry(e.layer.as_str()).or_insert(0) += 1;
    }
    n
}

/// How many of the lines (`patterns` false), or the fills and pattern lines, elements of `kind` draw, lettering left out.
fn drawn(m: &Model, d: &Drawing, kind: &str, patterns: bool) -> usize {
    (d.prims.iter().zip(&d.owners).zip(&d.parts))
        .filter(|((_, o), _)| o.and_then(|id| m.elements.get(id)).is_some_and(|e| e.category() == kind))
        .filter(|((p, _), part)| match p {
            _ if **part == Part::Text => false,
            Prim::Line { pts, .. } => pts.len() >= 2 && patterns == matches!(part, Part::Pattern | Part::LinePattern),
            Prim::Fill { rings, .. } => patterns && rings.iter().any(|r| r.len() >= 3),
            Prim::Image { .. } => false,
        })
        .count()
}

#[test]
fn the_sample_houses_plan_reads_back_on_aia_layers_in_millimetres() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let r = dxf::read_text(&text).unwrap();
    assert_eq!((r.version.as_str(), r.unit.as_str(), r.scale), ("AC1032", "mm", Some(1.0)));
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
    let n = per_layer(&r);
    for layer in ["A-DOOR", "A-GLAZ", "A-GRID", "A-AREA-IDEN", "A-ANNO-DIMS", "A-ANNO-SYMB", "A-FURN"] {
        assert!(n.get(layer).is_some_and(|k| *k > 0), "nothing on {layer}: {n:?}");
    }
    let aia = |l: &str| l.len() > 2 && ["A-", "S-", "P-", "E-", "M-", "G-"].iter().any(|p| l.starts_with(p));
    assert!(n.keys().all(|l| aia(l)), "{n:?}");
    // Each layer an entity is on is in the LAYER table.
    assert!(n.keys().all(|l| r.layers.iter().any(|t| t.name == *l)));
    // Everything the plan draws but pictures, paper and lettering (which is text, below) is an entity, one for each primitive where
    // no paper lies over what is drawn before it, and a piece or more of it where it does.
    let mut want: BTreeMap<String, usize> = BTreeMap::new();
    for (i, p) in d.prims.iter().enumerate() {
        let drawable = match p {
            _ if d.parts[i] == Part::Text => false,
            Prim::Line { pts, .. } => pts.len() >= 2,
            Prim::Fill { rings, color } => *color != crate::render::PAPER && rings.iter().any(|r| r.len() >= 3),
            Prim::Image { .. } => false,
        };
        if drawable {
            let name = on(layer(&m, d.owners[i]), d.parts[i], matches!(p, Prim::Fill { .. }));
            *want.entry(name).or_insert(0) += 1;
        }
    }
    for (name, k) in &want {
        let got = n.get(name.as_str()).copied().unwrap_or(0);
        if UNTOUCHED.contains(&name.as_str()) {
            assert_eq!(got, *k, "{name}");
        } else {
            assert!(got >= *k, "{name}: {got} entities for {k} primitives");
        }
    }
    // Lettering is one text entity for each string the plan set, not one for each stroke of it.
    let texts: BTreeMap<String, usize> = d.lettering.iter().fold(BTreeMap::new(), |mut n, l| {
        *n.entry(on(layer(&m, l.owner), Part::Text, false)).or_insert(0) += 1;
        n
    });
    for (name, k) in &texts {
        let got = r.entities.iter().filter(|e| e.layer == *name && matches!(e.shape, Shape::Text { .. })).count();
        assert!(got >= *k, "{name}: {got} texts for {k} strings");
    }
    assert!(n.keys().all(|l| want.contains_key(*l) || texts.contains_key(*l)), "a layer nothing is drawn on: {n:?}");
    assert!(!n.contains_key("A-DOOR-PATT") && !n.contains_key("A-GLAZ-PATT"), "those held only paper: {n:?}");
}

/// Layers no paper lies over in the sample's plan: what is drawn on them goes out one primitive, one entity (lettering aside, which
/// is a text for each string). Paper is laid over the rest: the walls' poché and lines and the floor's at the openings, a
/// dimension's line under its text, the roof's outline under a dimension's text and the tags, and whatever a tag, a mark or a
/// camera covers.
const UNTOUCHED: [&str; 7] = ["A-GRID", "A-GRID-IDEN", "A-AREA-IDEN", "A-ANNO-TEXT", "A-FLOR-CASE", "A-DOOR", "A-GLAZ"];

#[test]
fn walls_are_weighted_polylines_their_poche_a_solid_hatch_and_grids_dashed_in_mm() {
    let (m, d) = sample();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    // Where no paper lies over a closed line it stays one; a cut one is open pieces.
    let kept = |layer: &str| UNTOUCHED.contains(&layer);
    let closed = (d.prims.iter().zip(&d.owners).zip(&d.parts))
        .filter(|((p, o), part)| {
            matches!(p, Prim::Line { closed: true, pts, .. } if pts.len() >= 2) && kept(&on(layer(&m, **o), **part, false))
        })
        .count();
    assert!(closed > 0);
    assert_eq!(r.entities.iter().filter(|e| matches!(e.shape, Shape::Polyline { closed: true, .. }) && kept(&e.layer)).count(), closed);
    let walls: Vec<_> = r.entities.iter().filter(|e| e.layer == "A-WALL").collect();
    assert!(walls.iter().all(|e| e.weight.is_some_and(|w| w > 0.0) && matches!(e.color, Color::True(..))), "{:?}", walls[0]);
    let poche: Vec<_> = r.entities.iter().filter(|e| e.layer == "A-WALL-PATT" && matches!(e.shape, Shape::Fill { .. })).collect();
    assert!(!poche.is_empty() && poche.iter().all(|e| matches!(e.shape, Shape::Fill { solid: true, .. })));
    // A grid's dashes, px in the drawing, are mm in the file.
    let (dash, px) = (d.prims.iter().zip(&d.owners))
        .find_map(|(p, o)| match (p, o.and_then(|id| m.elements.get(id))) {
            (Prim::Line { dash: Some(dash), .. }, Some(Element::Grid(_))) => Some((dash.clone(), d.view.scale)),
            _ => None,
        })
        .unwrap();
    let grid = r.entities.iter().find(|e| e.layer == "A-GRID" && e.linetype.is_some()).unwrap();
    let lt = r.linetypes.iter().find(|l| Some(&l.name) == grid.linetype.as_ref()).unwrap();
    let want = f64::from(dash[0]) / px;
    assert!((lt.dashes[0] - want).abs() < 0.01, "{:?} against {want}", lt.dashes);
    assert!(lt.dashes[1] < 0.0, "a gap: {:?}", lt.dashes);
}

#[test]
fn its_extents_are_the_models_and_its_points_the_models_millimetres() {
    let (m, d) = sample();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let [lo, hi] = r.extents.unwrap();
    let [blo, bhi] = r.bounds().unwrap();
    // The entities' box is the extents' but for a text, which `bounds` takes as its point and the extents as the strokes it is drawn in.
    assert!(
        lo.0 <= blo.0 + 0.01 && lo.1 <= blo.1 + 0.01 && hi.0 >= bhi.0 - 0.01 && hi.1 >= bhi.1 - 0.01,
        "{lo:?} {hi:?} against {blo:?} {bhi:?}"
    );
    let not_text = r.entities.iter().filter(|e| !matches!(e.shape, Shape::Text { .. }));
    let [nlo, nhi] = dxf::union(not_text.map(|e| e.shape.bounds())).unwrap();
    assert!(lo.0 <= nlo.0 + 0.01 && hi.0 >= nhi.0 - 0.01, "{lo:?} {hi:?} against {nlo:?} {nhi:?}");
    // The box round every point the plan draws, taken back to the model's mm.
    let pts = d.prims.iter().filter(|p| !matches!(p, Prim::Image { .. })).flat_map(Prim::points).map(|p| d.view.mm(p));
    let [plo, phi] = dxf::union(pts.map(|p| [p, p])).unwrap();
    assert!(lo.dist(plo) < 0.01 && hi.dist(phi) < 0.01, "{lo:?} {hi:?} against {plo:?} {phi:?}");
    // The sample's outer walls, 300 mm thick, are centred on 0 to 10 m east and 0 to 7 m north.
    assert!(lo.0 <= -150.0 && lo.1 <= -150.0 && hi.0 >= 10_150.0 && hi.1 >= 7_150.0, "{lo:?} {hi:?}");
    // The west wall's faces lie on x = -150 and 150, to a hundredth of a mm.
    let on = |x: f64| {
        r.entities.iter().filter(|e| e.layer == "A-WALL").any(|e| match &e.shape {
            Shape::Polyline { pts, .. } => pts.iter().any(|p| (p.0 - x).abs() < 0.01),
            _ => false,
        })
    };
    assert!(on(-150.0) && on(150.0));
}

#[test]
fn layers_follow_the_owners_category_and_lettering_goes_on_its_annotation_layer() {
    let (m, _) = sample();
    let first = |kind: &str| m.elements.iter().find(|(_, e)| e.category() == kind).map(|(id, _)| *id);
    let at = |kind: &str| layer(&m, first(kind));
    assert_eq!(
        [at("wall"), at("door"), at("window"), at("room"), at("dimension"), at("grid"), at("tag")],
        ["A-WALL", "A-DOOR", "A-GLAZ", "A-AREA-IDEN", "A-ANNO-DIMS", "A-GRID", "A-ANNO-SYMB"]
    );
    assert_eq!(layer(&m, None), "A-ANNO-MISC");
    assert_eq!(layer(&m, Some(Id::MAX)), "A-ANNO-MISC");
    assert_eq!(on("A-WALL", Part::Cut, true), "A-WALL-PATT");
    assert_eq!(on("A-WALL", Part::Cut, false), "A-WALL");
    assert_eq!(on("A-DOOR", Part::Text, false), "A-DOOR-IDEN");
    assert_eq!(on("A-ANNO-DIMS", Part::Text, false), "A-ANNO-DIMS");
    assert_eq!(on("A-ANNO-SYMB", Part::Projection, true), "A-ANNO-SYMB");
}

fn paper(r: &dxf::Entity) -> bool {
    matches!((&r.shape, r.color), (Shape::Fill { .. }, Color::True(255, 255, 255)))
}

/// The gaps the plan's doors and windows clear in their walls, as paper fills: rings in mm.
fn gaps(m: &Model, d: &Drawing) -> Vec<Vec<P2>> {
    (d.prims.iter().zip(&d.owners))
        .filter_map(|(p, o)| match p {
            Prim::Fill { rings, color } if *color == crate::render::PAPER && matches!(layer(m, *o), "A-DOOR" | "A-GLAZ") => {
                rings.first().map(|r| r.iter().map(|q| d.view.mm(*q)).collect::<Vec<P2>>())
            }
            _ => None,
        })
        .collect()
}

/// `ring` scaled about its centre by `k`.
fn scaled(ring: &[P2], k: f64) -> Vec<P2> {
    let c = crate::geom::centroid(ring);
    ring.iter().map(|p| c + (*p - c) * k).collect()
}

#[test]
fn no_fill_of_the_paper_goes_out_as_a_hatch() {
    let (m, d) = sample();
    let paper_fills = d.prims.iter().filter(|p| matches!(p, Prim::Fill { color, .. } if *color == crate::render::PAPER)).count();
    assert!(paper_fills > 20, "the plan masks with paper: {paper_fills}");
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let white: Vec<_> = r.entities.iter().filter(|e| paper(e)).map(|e| e.layer.as_str()).collect();
    assert!(white.is_empty(), "{white:?}");
}

#[test]
fn the_poche_is_open_where_a_door_and_a_window_are_and_solid_elsewhere() {
    let (m, d) = sample();
    let gaps = gaps(&m, &d);
    assert!(gaps.len() >= 7, "the sample has doors and windows: {}", gaps.len());
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let poche = Color::True(crate::render::POCHE.0, crate::render::POCHE.1, crate::render::POCHE.2);
    let held =
        |p: P2, fills: &[Vec<Vec<P2>>]| fills.iter().any(|rings| rings.iter().filter(|r| crate::geom::contains(r, p)).count() % 2 == 1);
    let drawn: Vec<Vec<Vec<P2>>> = d
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Fill { rings, color } if *color == crate::render::POCHE => {
                Some(rings.iter().map(|r| r.iter().map(|q| d.view.mm(*q)).collect()).collect())
            }
            _ => None,
        })
        .collect();
    let exported: Vec<Vec<Vec<P2>>> = (r.entities.iter())
        .filter_map(|e| match &e.shape {
            Shape::Fill { rings, .. } if e.color == poche => Some(rings.clone()),
            _ => None,
        })
        .collect();
    let (mut open, mut solid) = (0, 0);
    for i in -4..110 {
        for j in -4..80 {
            let p = P2(f64::from(i) * 97.0 + 13.7, f64::from(j) * 97.0 + 21.3);
            if !held(p, &drawn) {
                continue;
            }
            if gaps.iter().any(|g| crate::geom::contains(&scaled(g, 0.98), p)) {
                assert!(!held(p, &exported), "poché in an opening at {p:?}");
                open += 1;
            } else if !gaps.iter().any(|g| crate::geom::contains(&scaled(g, 1.02), p)) {
                assert!(held(p, &exported), "poché lost from the solid wall at {p:?}");
                solid += 1;
            }
        }
    }
    assert!(open > 20 && solid > 100, "{open} points in openings, {solid} in solid wall");
}

#[test]
fn a_walls_lines_stop_at_a_jamb() {
    let (m, d) = sample();
    let gaps: Vec<Vec<P2>> = gaps(&m, &d).iter().map(|g| scaled(g, 0.98)).collect();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let mut sampled = 0;
    for e in r.entities.iter().filter(|e| e.layer == "A-WALL") {
        let Shape::Polyline { pts, .. } = &e.shape else { continue };
        for w in pts.windows(2) {
            let n = (w[0].dist(w[1]) / 10.0).ceil().max(1.0) as usize;
            for k in 0..=n {
                let p = w[0] + (w[1] - w[0]) * (k as f64 / n as f64);
                assert!(!gaps.iter().any(|g| crate::geom::contains(g, p)), "a wall's line crosses an opening at {p:?}");
                sampled += 1;
            }
        }
    }
    assert!(sampled > 100, "{sampled}");
}

#[test]
fn the_glass_and_the_leaf_still_go_out() {
    let (m, d) = sample();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let n = per_layer(&r);
    assert_eq!(n.get("A-DOOR").copied(), Some(drawn(&m, &d, "door", false)), "{n:?}");
    assert_eq!(n.get("A-GLAZ").copied(), Some(drawn(&m, &d, "window", false)), "{n:?}");
    // A door's swing, a quarter circle, is there whole.
    assert!(r.entities.iter().any(|e| e.layer == "A-DOOR" && matches!(&e.shape, Shape::Polyline { pts, .. } if pts.len() >= 25)));
}

#[test]
fn writing_a_dxf_leaves_the_drawing_alone() {
    let (m, d) = sample();
    let points = |d: &Drawing| d.prims.iter().flat_map(Prim::points).collect::<Vec<_>>();
    let (before, n) = (points(&d), d.prims.len());
    let text = dxf(&d, &m);
    assert_eq!((points(&d), d.prims.len()), (before, n));
    assert_eq!(dxf(&d, &m), text);
}

/// A plan saved as DXF and imported into a fresh project, drawn again: the middle of a window is open and its glass is there.
#[test]
fn a_plan_saved_and_imported_again_is_open_at_its_windows_and_has_its_glass() {
    let (mut s, path) = (Session::new(), std::env::temp_dir().join(format!("bimcraft-paper-openings-{}.dxf", std::process::id())));
    s.script(crate::sample::SAMPLE).unwrap();
    s.execute("save", &serde_json::json!({"path": path})).unwrap();
    let (m, d) = sample();
    let mut t = Session::new();
    t.execute("import_cad", &serde_json::json!({"path": path, "position": "origin"})).unwrap();
    let level = t.model.levels()[0].0;
    let back = plan(&t.model, level, 1600, 1000);
    let png = tiny_skia::Pixmap::decode_png(&crate::render::png(&back).unwrap()).unwrap();
    let at = |p: P2| {
        let (x, y) = back.view.px(p);
        let c = png.pixel(x as u32, y as u32).unwrap().demultiply();
        crate::render::Rgb(c.red(), c.green(), c.blue())
    };
    let windows: Vec<Vec<P2>> = (d.prims.iter().zip(&d.owners))
        .filter_map(|(p, o)| match p {
            Prim::Fill { rings, color } if *color == crate::render::PAPER && layer(&m, *o) == "A-GLAZ" => {
                rings.first().map(|r| r.iter().map(|q| d.view.mm(*q)).collect())
            }
            _ => None,
        })
        .collect();
    assert!(!windows.is_empty());
    for g in &windows {
        let c = crate::geom::centroid(g);
        // Halfway between the glass line in the middle and the one on a face, and a quarter of the way along.
        let quarter = c + (g[0] - c) * 0.25;
        assert_eq!(at(quarter), crate::render::PAPER, "the window's gap at {quarter:?}");
        assert_ne!(at(c), crate::render::PAPER, "its glass at {c:?}");
        assert_ne!(at(c), crate::render::POCHE, "its glass at {c:?}");
    }
}

#[test]
fn an_empty_drawing_and_one_of_bad_points_write_a_file_that_reads() {
    let m = Model::new("Empty");
    let view = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 100, height: 100 };
    let mut d = Drawing::new(view);
    assert!(dxf::read_text(&dxf(&d, &m)).unwrap().entities.is_empty());
    d.push(Prim::Line { pts: vec![(f32::NAN, 0.0), (1.0, 1.0)], width: 1.0, color: crate::render::INK, dash: None, closed: false });
    d.push(Prim::Line { pts: vec![(0.0, 0.0)], width: 1.0, color: crate::render::INK, dash: Some(vec![]), closed: false });
    d.push(Prim::Fill { rings: vec![vec![(0.0, 0.0), (1.0, 0.0)]], color: crate::render::INK });
    d.push(Prim::Line {
        pts: vec![(0.0, 0.0), (10.0, 0.0)],
        width: f32::INFINITY,
        color: crate::render::INK,
        dash: Some(vec![f32::NAN, -1.0, 3.0]),
        closed: false,
    });
    // Paper that is not a shape, or has no area, masks nothing.
    let paper = crate::render::PAPER;
    d.push(Prim::Fill { rings: vec![vec![(f32::NAN, 0.0), (5.0, 5.0), (0.0, 5.0)]], color: paper });
    d.push(Prim::Fill { rings: vec![vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)]], color: paper });
    d.push(Prim::Fill { rings: vec![vec![(0.0, 0.0), (f32::INFINITY, 0.0), (10.0, 5.0)]], color: paper });
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert_eq!(r.entities.len(), 1);
    assert_eq!(r.entities[0].shape, Shape::Polyline { pts: vec![P2(-500.0, 500.0), P2(-400.0, 500.0)], closed: false });
}

/// The layer, string and height (mm) of each text in `r`.
fn texts(r: &dxf::Dxf) -> Vec<(&str, &str, f64)> {
    r.entities
        .iter()
        .filter_map(|e| match &e.shape {
            Shape::Text { text, height, .. } => Some((e.layer.as_str(), text.as_str(), *height)),
            _ => None,
        })
        .collect()
}

const SET: [(&str, &str); 4] =
    [("Kitchen", "A-AREA-IDEN"), ("10300", "A-ANNO-DIMS"), ("6000", "A-ANNO-DIMS"), ("Oak floor", "A-ANNO-TEXT")];

#[test]
fn the_plan_keeps_its_lettering_beside_the_strokes_it_is_drawn_in() {
    let (m, d) = sample();
    for (words, want) in SET {
        let l = d.lettering.iter().find(|l| l.text.eq_ignore_ascii_case(words)).unwrap_or_else(|| panic!("no lettering of {words}"));
        assert!(l.height > 0.0 && l.height.is_finite(), "{words}: {}", l.height);
        assert_eq!(on(layer(&m, l.owner), Part::Text, false), want, "{words}");
    }
    // Each string is drawn by several strokes too, which the picture keeps drawing.
    assert!(d.parts.iter().filter(|p| **p == Part::Text).count() > d.lettering.len());
}

#[test]
fn lettering_goes_out_as_text_in_mm_on_its_owners_layer_and_not_as_strokes() {
    let (m, d) = sample();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
    let all = texts(&r);
    for (words, want) in SET {
        let (layer, _, height) =
            all.iter().find(|(_, t, _)| t.eq_ignore_ascii_case(words)).unwrap_or_else(|| panic!("no text {words}: {all:?}"));
        assert_eq!(*layer, want, "{words}");
        // The capital's height in mm: px over the view's px to the mm.
        let l = d.lettering.iter().find(|l| l.text.eq_ignore_ascii_case(words)).unwrap();
        assert!((height - f64::from(l.height) / d.view.scale).abs() < 0.01, "{words}: {height}");
        assert!(*height > 1.0 && *height < 1000.0, "{words}: {height}");
    }
    // The room names and the note have no stroke left under them.
    for layer in ["A-AREA-IDEN", "A-ANNO-TEXT"] {
        assert!(r.entities.iter().filter(|e| e.layer == layer).all(|e| matches!(e.shape, Shape::Text { .. })), "{layer}");
    }
}

#[test]
fn the_extents_still_hold_the_strokes_the_text_replaced() {
    let (m, d) = sample();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let [lo, hi] = r.extents.unwrap();
    let ink: Vec<P2> = (d.prims.iter().zip(&d.parts))
        .filter(|(_, part)| **part == Part::Text)
        .flat_map(|(p, _)| p.points())
        .map(|p| d.view.mm(p))
        .collect();
    assert!(!ink.is_empty());
    assert!(ink.iter().all(|p| p.0 >= lo.0 - 0.01 && p.0 <= hi.0 + 0.01 && p.1 >= lo.1 - 0.01 && p.1 <= hi.1 + 0.01));
}

/// The plan of the first level of a project holding a note saying `words`.
fn noted(words: &str) -> (std::sync::Arc<Model>, Drawing) {
    let mut s = Session::new();
    s.execute("text", &serde_json::json!({"text": words, "at": [1000, 2000]})).unwrap();
    let m = crate::link::seen(&s.model);
    let d = plan(&m, m.levels()[0].0, 800, 600);
    (m, d)
}

#[test]
fn a_note_of_several_lines_is_one_mtext_that_reads_back_as_its_lines() {
    let (m, d) = noted("First line\nSecond \\ {line}");
    let text = dxf(&d, &m);
    assert_eq!(text.lines().filter(|l| l.trim() == "MTEXT").count(), 1, "one MTEXT");
    let r = dxf::read_text(&text).unwrap();
    let lines: Vec<_> = texts(&r).into_iter().filter(|(layer, ..)| *layer == "A-ANNO-TEXT").collect();
    assert_eq!(lines.iter().map(|(_, t, _)| *t).collect::<Vec<_>>(), ["First line", "Second \\ {line}"], "{lines:?}");
    assert!(lines.iter().all(|(layer, ..)| *layer == "A-ANNO-TEXT"));
    // The first line's capital top is at the note's point, 1000 and 2000 mm, the next one down a line's pitch.
    let notes: Vec<(P2, f64)> = (r.entities.iter().filter(|e| e.layer == "A-ANNO-TEXT"))
        .filter_map(|e| match &e.shape {
            Shape::Text { at, height, .. } => Some((*at, *height)),
            _ => None,
        })
        .collect();
    let ((a, h), (b, _)) = (notes[0], notes[1]);
    assert!((a.0 - 1000.0).abs() < 1.0 && (a.1 - (2000.0 - h)).abs() < 1.0, "{a:?} {h}");
    assert!(b.1 < a.1 && (a.1 - b.1 - 1.6 * h).abs() < 1.0, "{a:?} {b:?}");
}

#[test]
fn a_single_line_note_is_a_text_whose_top_left_is_the_notes_point() {
    let (m, d) = noted("Oak floor");
    let text = dxf(&d, &m);
    let r = dxf::read_text(&text).unwrap();
    let mine: Vec<_> = r.entities.iter().filter(|e| e.layer == "A-ANNO-TEXT").collect();
    let [Shape::Text { at, height, text, align, angle, font }] = &mine.iter().map(|e| e.shape.clone()).collect::<Vec<_>>()[..] else {
        panic!("{mine:?}")
    };
    assert_eq!(font.as_deref(), Some("Arial"));
    assert_eq!(text, "Oak floor");
    // An MTEXT's lines read back on their baselines, a capital below the note's point.
    assert_eq!(*align, dxf::Align { h: dxf::Horizontal::Left, v: dxf::Vertical::Baseline });
    assert!((at.0 - 1000.0).abs() < 1.0 && (at.1 - (2000.0 - height)).abs() < 1.0 && angle.abs() < 1e-6, "{at:?} {angle}");
    // A 2.5 mm capital on paper, at the plan's scale.
    let scale = crate::graphics::scale_of(&m, m.levels()[0].0);
    assert!((height - 2.5 * scale).abs() < 0.5, "{height} at 1:{scale}");
}

#[test]
fn lettering_in_a_typeface_goes_as_an_mtext_with_its_run_and_the_stroked_as_a_text() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let r = dxf::read_text(&text).unwrap();
    let font_of = |words: &str| {
        r.entities.iter().find_map(|e| match &e.shape {
            Shape::Text { text, font, .. } if text.eq_ignore_ascii_case(words) => Some(font.clone()),
            _ => None,
        })
    };
    // A dimension's number is laid out in the type's face; a room's name is drawn in our single-stroke lettering.
    assert_eq!(font_of("10300"), Some(Some("Arial".to_string())));
    assert_eq!(font_of("Kitchen"), Some(None));
    assert!(text.contains("{\\fArial|b0|i0|c0|p34;10300}"));
}

#[test]
fn a_typefaces_name_cannot_break_out_of_its_run() {
    let m = Model::new("Empty");
    let view = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 100, height: 100 };
    let mut d = Drawing::new(view);
    d.lettering.push(crate::render::Lettering {
        text: "Fine".into(),
        at: (50.0, 50.0),
        height: 10.0,
        angle: 0.0,
        align: dxf::Align { h: dxf::Horizontal::Left, v: dxf::Vertical::Baseline },
        owner: None,
        color: crate::render::INK,
        font: Some("Ev;il|\\{x}\n".into()),
        from: 0,
    });
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert_eq!(texts(&r).iter().map(|(_, t, _)| *t).collect::<Vec<_>>(), ["Fine"]);
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
}

#[test]
fn characters_outside_ascii_go_out_as_utf8_which_the_2018_dxf_is() {
    let (m, d) = noted("Caf\u{e9} 24 m\u{b2}");
    let text = dxf(&d, &m);
    assert!(text.contains("Caf\u{e9} 24 m\u{b2}") && !text.contains("\\U+"));
    let r = dxf::read_text(&text).unwrap();
    assert!(texts(&r).iter().any(|(_, t, _)| *t == "Caf\u{e9} 24 m\u{b2}"), "{:?}", texts(&r));
}

#[test]
fn lettering_that_is_not_a_shape_is_left_out_without_harm() {
    let m = Model::new("Empty");
    let view = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 100, height: 100 };
    let mut d = Drawing::new(view);
    let ok = crate::render::Lettering {
        text: "Fine".into(),
        at: (50.0, 50.0),
        height: 10.0,
        angle: 0.0,
        align: dxf::Align { h: dxf::Horizontal::Left, v: dxf::Vertical::Baseline },
        owner: None,
        color: crate::render::INK,
        font: None,
        from: 0,
    };
    for bad in [
        crate::render::Lettering { at: (f32::NAN, 0.0), ..ok.clone() },
        crate::render::Lettering { height: 0.0, ..ok.clone() },
        crate::render::Lettering { height: f32::INFINITY, ..ok.clone() },
        crate::render::Lettering { angle: f32::NAN, ..ok.clone() },
        crate::render::Lettering { text: "  ".into(), ..ok.clone() },
    ] {
        d.lettering.push(bad);
    }
    d.lettering.push(ok);
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert_eq!(texts(&r).iter().map(|(_, t, _)| *t).collect::<Vec<_>>(), ["Fine"]);
    // A view that maps nothing writes no text.
    d.view.scale = f64::NAN;
    assert!(dxf::read_text(&dxf(&d, &m)).unwrap().entities.is_empty());
}
