//! A section and an elevation written as DXF, read back with the core's own reader: the vector drawing in the view's own plane,
//! its lettering as text, and the picture of what lies beyond owned up to rather than traced.

use std::collections::BTreeMap;

use super::tests::{per_layer, sample};
use super::*;
use crate::dxf::{self, Shape};
use crate::model::Element;
use crate::render::{section_drawn, section_fit};

/// The drawing of the view called `name` in the sample, fitted as `save` fits it, and how many pictures it is over.
fn view(name: &str) -> (std::sync::Arc<Model>, Drawing, usize) {
    let (m, _) = sample();
    let id = m
        .elements
        .iter()
        .find_map(|(id, e)| match e {
            Element::Section(v) | Element::Elevation(v) if v.name == name => Some(*id),
            _ => None,
        })
        .unwrap();
    let (d, beyond) = section_drawn(&m, id, section_fit(&m, id, 1600, 1000)).unwrap();
    (m, d, beyond)
}

fn texts(r: &dxf::Dxf) -> Vec<&str> {
    r.entities.iter().filter_map(|e| if let Shape::Text { text, .. } = &e.shape { Some(text.as_str()) } else { None }).collect()
}

fn layers(n: &BTreeMap<&str, usize>, names: &[&str]) -> usize {
    names.iter().map(|l| n.get(l).copied().unwrap_or(0)).sum()
}

#[test]
fn a_sections_cut_walls_and_grids_go_out_on_aia_layers() {
    let (m, d, _) = view("Section 1");
    assert!(d.view.scale.is_finite() && d.view.scale > 0.0);
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert_eq!((r.version.as_str(), r.unit.as_str()), ("AC1032", "mm"));
    let n = per_layer(&r);
    let aia = |l: &str| l == "0" || (l.len() > 2 && ["A-", "S-", "P-", "E-", "M-", "G-"].iter().any(|p| l.starts_with(p)));
    assert!(n.keys().all(|l| aia(l)), "{n:?}");
    assert!(layers(&n, &["A-WALL", "A-WALL-PATT"]) >= 3, "{n:?}");
    let fills =
        r.entities.iter().filter(|e| e.layer.starts_with("A-WALL") && matches!(e.shape, Shape::Polyline { closed: true, .. })).count();
    assert!(fills >= 1 || layers(&n, &["A-WALL-PATT"]) >= 1, "no poche: {n:?}");
    assert!(layers(&n, &["A-GRID", "A-GRID-IDEN"]) >= 3, "{n:?}");
}

#[test]
fn a_sections_level_names_go_out_as_text() {
    let (m, d, _) = view("Section 1");
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let t = texts(&r);
    for level in ["Level 1", "Level 2"] {
        assert!(t.contains(&level), "{level} is not lettered: {t:?}");
    }
}

#[test]
fn a_section_is_not_the_level_plan() {
    let (m, d, _) = view("Section 1");
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    let t = texts(&r);
    for room in ["Living", "Kitchen", "Bedroom", "Study"] {
        assert!(!t.iter().any(|s| s.eq_ignore_ascii_case(room)), "the section letters {room}: {t:?}");
    }
    let (pm, pd) = sample();
    let plan = dxf::read_text(&dxf(&pd, &pm)).unwrap();
    assert!(r.entities.len() < 2 * plan.entities.len(), "{} against the plan's {}", r.entities.len(), plan.entities.len());
}

#[test]
fn an_elevation_goes_out_as_cad_too() {
    let (m, d, _) = view("South");
    let r = dxf::read_text(&dxf(&d, &m)).unwrap();
    assert!(!r.entities.is_empty());
    let t = texts(&r);
    assert!(t.iter().any(|s| s.starts_with("Level ")), "no level lettered: {t:?}");
}

#[test]
fn what_lies_beyond_is_counted_and_never_traced() {
    for name in ["Section 1", "South"] {
        let (m, d, beyond) = view(name);
        assert_eq!(beyond, 1, "{name}: the hidden-line picture it is drawn over is one");
        // The picture is the drawing's ground, not a primitive of it; nothing of it comes out as ink.
        assert_eq!(pictures(&d), 0);
        let r = dxf::read_text(&dxf(&d, &m)).unwrap();
        assert!(r.entities.len() < 400, "{name}: {} entities", r.entities.len());
    }
}

/// What `save` and `render` answer for a section or an elevation as DXF: a file, no picture, and a note on the one it left out.
#[test]
fn a_section_saved_as_dxf_owns_up_to_the_picture_beyond_it() {
    use crate::engine::Session;
    use serde_json::json;
    let dir = std::env::temp_dir().join(format!("bimcraft-sections-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    for (how, view) in [("save", "Section 1"), ("save", "South"), ("render", "Section 1"), ("render", "South")] {
        let path = dir.join(format!("{how}-{}.dxf", view.replace(' ', "")));
        let r = s.execute(how, &json!({"path": path, "view": view})).unwrap();
        assert!(r.get("pngBase64").is_none(), "{how} {view}: {r}");
        assert!(r.get("format").is_none_or(|f| f == "dxf"), "{how} {view}: {r}");
        let notes = r["notes"].as_array().unwrap_or_else(|| panic!("{how} {view}: no notes in {r}"));
        assert!(notes.iter().any(|n| n.as_str().is_some_and(|n| n.starts_with("1 picture"))), "{how} {view}: {notes:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("ENTITIES") && dxf::read_text(&text).is_ok(), "{how} {view}");
    }
    // A name no view has errors as it did; a plan's reply says nothing of what lies beyond.
    assert!(s.execute("save", &json!({"path": dir.join("x.dxf"), "view": "Nowhere"})).is_err());
    let r = s.execute("save", &json!({"path": dir.join("plan.dxf"), "view": "Level 1"})).unwrap();
    assert!(!r.to_string().contains("beyond"), "{r}");
    std::fs::remove_dir_all(&dir).ok();
}
