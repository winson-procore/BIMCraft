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
    assert_eq!(n.get("A-WALL").copied(), Some(drawn(&m, &d, "wall", false)), "{n:?}");
    assert_eq!(n.get("A-WALL-PATT").copied(), Some(drawn(&m, &d, "wall", true)), "{n:?}");
    for layer in ["A-DOOR", "A-GLAZ", "A-GRID", "A-AREA-IDEN", "A-ANNO-DIMS", "A-ANNO-SYMB", "A-FURN"] {
        assert!(n.get(layer).is_some_and(|k| *k > 0), "nothing on {layer}: {n:?}");
    }
    let aia = |l: &str| l.len() > 2 && ["A-", "S-", "P-", "E-", "M-", "G-"].iter().any(|p| l.starts_with(p));
    assert!(n.keys().all(|l| aia(l)), "{n:?}");
    // Each layer an entity is on is in the LAYER table.
    assert!(n.keys().all(|l| r.layers.iter().any(|t| t.name == *l)));
    // Everything the plan draws but pictures is an entity.
    let drawable = d
        .prims
        .iter()
        .filter(|p| match p {
            Prim::Line { pts, .. } => pts.len() >= 2,
            Prim::Fill { rings, .. } => rings.iter().any(|r| r.len() >= 3),
            Prim::Image { .. } => false,
        })
        .count();
    assert_eq!(r.entities.len(), drawable);
}

#[test]
fn walls_are_weighted_polylines_their_poche_a_solid_hatch_and_grids_dashed_in_mm() {
    let (m, d) = sample();
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let closed = d.prims.iter().filter(|p| matches!(p, Prim::Line { closed: true, pts, .. } if pts.len() >= 2)).count();
    assert!(closed > 0);
    assert_eq!(r.entities.iter().filter(|e| matches!(e.shape, Shape::Polyline { closed: true, .. })).count(), closed);
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
    assert!(lo.dist(blo) < 0.01 && hi.dist(bhi) < 0.01, "{lo:?} {hi:?} against {blo:?} {bhi:?}");
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
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert_eq!(r.entities.len(), 1);
    assert_eq!(r.entities[0].shape, Shape::Polyline { pts: vec![P2(-500.0, 500.0), P2(-400.0, 500.0)], closed: false });
}
