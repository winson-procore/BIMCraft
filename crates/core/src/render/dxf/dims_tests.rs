//! A plan's dimensions as DIMENSION entities: read back from the DXF text as a plain group-code walk, not through the core's
//! reader, which only the last tests use.

use std::collections::BTreeMap;

use super::tests::{per_layer, sample};
use super::*;
use crate::dxf;
use crate::engine::Session;
use crate::geom::P2;
use crate::model::Element;
use crate::render::{Part, plan};

pub(super) type Pairs = Vec<(i32, String)>;

/// What a DXF's text holds in `section` ("TABLES", "BLOCKS", "ENTITIES"): each record's name (its group 0) and its other pairs.
pub(super) fn records(text: &str, section: &str) -> Vec<(String, Pairs)> {
    let lines: Vec<&str> = text.lines().collect();
    let (mut out, mut inside, mut next) = (Vec::new(), false, 0);
    while let (Some(code), Some(value)) = (lines.get(next), lines.get(next + 1)) {
        next += 2;
        let (code, value) = (code.trim().parse::<i32>().unwrap_or(-1), value.trim_end_matches('\r'));
        match (code, value) {
            (0, "SECTION") => inside = lines.get(next + 1).is_some_and(|n| n.trim() == section),
            (0, "ENDSEC") => inside = false,
            (0, name) if inside => out.push((name.to_string(), Vec::new())),
            (code, value) if inside => {
                if let Some((_, pairs)) = out.last_mut() {
                    pairs.push((code, value.to_string()));
                }
            }
            _ => {}
        }
    }
    out
}

pub(super) fn group(pairs: &Pairs, code: i32) -> Option<&str> {
    pairs.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
}

pub(super) fn number(pairs: &Pairs, code: i32) -> Option<f64> {
    group(pairs, code).and_then(|v| v.parse().ok())
}

/// Each DIMENSION in the ENTITIES, its group pairs.
fn dimensions(text: &str) -> Vec<Pairs> {
    records(text, "ENTITIES").into_iter().filter(|(k, _)| k == "DIMENSION").map(|(_, p)| p).collect()
}

/// The distance between its two extension-line points, groups 13 and 14.
fn measured(pairs: &Pairs) -> f64 {
    let at = |a: i32, b: i32| P2(number(pairs, a).unwrap_or(f64::NAN), number(pairs, b).unwrap_or(f64::NAN));
    at(13, 23).dist(at(14, 24))
}

/// Two vertical grid lines 3000 apart, as references a dimension can measure between.
fn two_grids(s: &mut Session) -> serde_json::Value {
    for x in [0, 3000] {
        s.execute("grid", &serde_json::json!({"start": [x, -2000], "end": [x, 8000]})).unwrap();
    }
    let ids: Vec<_> =
        s.model.elements.iter().filter(|(_, e)| e.category() == "grid").map(|(id, _)| serde_json::json!({"id": id})).collect();
    serde_json::Value::Array(ids)
}

const STROKES: [&str; 10] = ["LWPOLYLINE", "POLYLINE", "LINE", "ARC", "CIRCLE", "HATCH", "SOLID", "SPLINE", "ELLIPSE", "POINT"];

fn is_dimension(m: &Model, owner: Option<crate::model::Id>) -> bool {
    owner.and_then(|id| m.elements.get(id)).is_some_and(|e| matches!(e, Element::Dimension(_)))
}

#[test]
fn a_plans_dimensions_go_out_as_dimension_entities_on_the_dims_layer() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let dims = dimensions(&text);
    assert_eq!(m.elements.iter().filter(|(_, e)| matches!(e, Element::Dimension(_))).count(), 3);
    // One for each segment: 10300, 7300, and the third's two, 6000 and 4000.
    assert_eq!(dims.len(), 4, "{dims:?}");
    assert!(dims.iter().all(|p| group(p, 8) == Some("A-ANNO-DIMS")));
    let spans: Vec<f64> = dims.iter().map(measured).collect();
    for want in [10_300.0, 7_300.0, 6_000.0, 4_000.0] {
        assert!(spans.iter().any(|s| (s - want).abs() < 0.5), "{want} in {spans:?}");
    }
    assert!(spans.iter().all(|s| *s > 0.0 && s.is_finite()), "{spans:?}");
    // Aligned, by group 70's low bits; the number is the TEXT beside it, so the entity's own is blank.
    assert!(dims.iter().all(|p| number(p, 70).is_some_and(|t| (t as i64) & 0x0F == 1)), "{dims:?}");
    assert!(dims.iter().all(|p| group(p, 1) == Some(" ")));
}

#[test]
fn a_linear_dimension_is_a_rotated_one_along_its_axis() {
    let mut s = Session::new();
    let refs = two_grids(&mut s);
    s.execute("dimension", &serde_json::json!({"kind": "linear", "orient": "horizontal", "refs": refs, "at": [0, -2000]})).unwrap();
    let m = crate::link::seen(&s.model);
    let d = plan(&m, m.levels()[0].0, 800, 600);
    let text = dxf(&d, &m);
    let dims = dimensions(&text);
    let [one] = &dims[..] else { panic!("{dims:?}") };
    assert_eq!(number(one, 70).map(|t| t as i64 & 0x0F), Some(0));
    assert!(number(one, 50).is_some_and(|a| a.abs() < 1e-6), "{one:?}");
    // Along the axis it measures: the reference points are 3000 apart east and level north.
    assert!((measured(one) - 3000.0).abs() < 0.5, "{}", measured(one));
    assert!(dxf::read_text(&text).unwrap().left_out.is_empty());
}

#[test]
fn a_dimension_names_its_own_style_and_the_table_holds_it() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let table: Vec<_> = records(&text, "TABLES").into_iter().filter(|(k, _)| k == "DIMSTYLE").collect();
    let scale = crate::graphics::scale_of(&m, m.levels()[0].0);
    for p in dimensions(&text) {
        let style = group(&p, 3).unwrap_or_default();
        assert!(!style.is_empty() && !style.eq_ignore_ascii_case("standard"), "{style:?}");
        let (_, rec) = table.iter().find(|(_, r)| group(r, 2) == Some(style)).unwrap_or_else(|| panic!("no style {style}"));
        // Sizes in mm of the model, as the text beside it is: 2.5 mm of paper at the plan's scale.
        assert!((number(rec, 140).unwrap_or(0.0) - 2.5 * scale).abs() < 0.5, "{rec:?}");
        for code in [41, 42, 44] {
            assert!(number(rec, code).is_some_and(|a| a > 0.0), "{code} in {rec:?}");
        }
    }
}

#[test]
fn a_style_name_is_one_a_dxf_allows_and_the_same_in_the_table_and_the_entity() {
    let name = "Odd: <name>/\"x\"; a,b=c?*|\\";
    let mut s = Session::new();
    s.execute("dimension_type", &serde_json::json!({"name": name, "like": "Diagonal - 2.5mm Arial"})).unwrap();
    let refs = two_grids(&mut s);
    s.execute("dimension", &serde_json::json!({"refs": refs, "at": [0, -2000], "type": name})).unwrap();
    let m = crate::link::seen(&s.model);
    let d = plan(&m, m.levels()[0].0, 800, 600);
    let text = dxf(&d, &m);
    let dims = dimensions(&text);
    let style = group(&dims[0], 3).unwrap_or_default().to_string();
    assert!(!style.is_empty() && !style.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=']), "{style:?}");
    assert!(records(&text, "TABLES").iter().any(|(k, r)| k == "DIMSTYLE" && group(r, 2) == Some(style.as_str())));
    assert!(dxf::read_text(&text).unwrap().left_out.is_empty());
}

#[test]
fn the_dims_layer_carries_no_strokes_in_the_drawing_once_its_dimensions_are_entities() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let on_layer: Vec<String> =
        records(&text, "ENTITIES").into_iter().filter(|(_, p)| group(p, 8) == Some("A-ANNO-DIMS")).map(|(k, _)| k).collect();
    assert!(on_layer.iter().any(|k| k == "TEXT" || k == "MTEXT"), "{on_layer:?}");
    assert!(on_layer.iter().all(|k| !STROKES.contains(&k.as_str())), "{on_layer:?}");
}

#[test]
fn a_dimensions_witness_lines_and_ticks_are_its_own_block_which_the_entity_names() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let named: Vec<String> = dimensions(&text).iter().filter_map(|p| group(p, 2)).filter(|n| !n.is_empty()).map(str::to_string).collect();
    // One block for each dimension of the plan, held by the first of its segments.
    assert_eq!(named.len(), 3, "{named:?}");
    let blocks = records(&text, "BLOCKS");
    let table = records(&text, "TABLES");
    for name in &named {
        assert!(name.starts_with('*'), "an anonymous block: {name}");
        assert!(blocks.iter().any(|(k, p)| k == "BLOCK" && group(p, 2) == Some(name.as_str())), "no BLOCK {name}");
        assert!(table.iter().any(|(k, p)| k == "BLOCK_RECORD" && group(p, 2) == Some(name.as_str())), "no BLOCK_RECORD {name}");
    }
    // Each stroke the plan draws for a dimension is in a block, which a reader takes back as the dimension's lines.
    let owned = (d.prims.iter().zip(&d.owners).zip(&d.parts))
        .filter(|((p, o), part)| {
            **part != Part::Text && is_dimension(&m, **o) && !matches!(p, Prim::Fill { color, .. } if *color == crate::render::PAPER)
        })
        .count();
    let held = blocks.iter().filter(|(k, _)| STROKES.contains(&k.as_str())).count();
    assert!(owned > 0 && held >= owned, "{held} strokes held for {owned} drawn");
    let r = dxf::read_text(&text).unwrap();
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
    let n: BTreeMap<&str, usize> = per_layer(&r);
    assert!(n.get("A-ANNO-DIMS").is_some_and(|k| *k >= owned), "{n:?}");
}

#[test]
fn a_dimension_the_export_does_not_state_keeps_its_strokes() {
    // A radial dimension is not an entity yet: its lines stay in the drawing as they were.
    let mut s = Session::new();
    s.execute("column", &serde_json::json!({"at": [2000, 3000], "type": "Round 300"})).unwrap();
    let col = s.model.elements.iter().find(|(_, e)| e.category() == "column").map(|(id, _)| *id).unwrap();
    s.execute("dimension", &serde_json::json!({"kind": "radial", "refs": [{"id": col}], "at": [2500, 3500]})).unwrap();
    let m = crate::link::seen(&s.model);
    let d = plan(&m, m.levels()[0].0, 800, 600);
    let text = dxf(&d, &m);
    assert!(dimensions(&text).is_empty());
    let strokes =
        records(&text, "ENTITIES").into_iter().filter(|(k, p)| group(p, 8) == Some("A-ANNO-DIMS") && STROKES.contains(&k.as_str())).count();
    assert!(strokes > 0);
}

#[test]
fn dimensions_that_cannot_be_placed_keep_their_strokes() {
    let (m, mut d) = sample();
    d.view.scale = f64::NAN;
    assert!(dimensions(&dxf(&d, &m)).is_empty());
    let (m, mut d) = sample();
    for r in &mut d.dimensions {
        r.from = P2(f64::NAN, 0.0);
    }
    let text = dxf(&d, &m);
    assert!(dimensions(&text).is_empty());
    assert!(records(&text, "ENTITIES").iter().any(|(k, p)| k == "LWPOLYLINE" && group(p, 8) == Some("A-ANNO-DIMS")));
}

#[test]
fn the_plan_keeps_what_it_says_of_each_dimension_beside_its_strokes() {
    let (_, d) = sample();
    assert_eq!(d.dimensions.len(), 4);
    assert!(d.dimensions.iter().all(|r| r.owner.is_some() && r.measure > 0.0 && r.text_height > 0.0 && r.style.starts_with("Diagonal")));
}
