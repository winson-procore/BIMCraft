//! A plan's doors, windows and components as INSERTs of blocks: read back from the DXF text as a plain group-code walk.

use std::collections::BTreeMap;

use super::dims_tests::{Pairs, group, number, records};
use super::tests::sample;
use super::*;
use crate::geom::P2;
use crate::model::Element;

const STROKES: [&str; 10] = ["LWPOLYLINE", "POLYLINE", "LINE", "ARC", "CIRCLE", "HATCH", "SOLID", "SPLINE", "ELLIPSE", "POINT"];

/// The INSERTs of the ENTITIES, each with the ATTRIBs that follow it.
fn inserts(text: &str) -> Vec<(Pairs, Vec<Pairs>)> {
    let mut out: Vec<(Pairs, Vec<Pairs>)> = Vec::new();
    let mut open = false;
    for (kind, pairs) in records(text, "ENTITIES") {
        match kind.as_str() {
            "INSERT" => {
                out.push((pairs, Vec::new()));
                open = true;
            }
            "ATTRIB" if open => out.last_mut().into_iter().for_each(|(_, a)| a.push(pairs.clone())),
            _ => open = false,
        }
    }
    out
}

/// The BLOCKS' definitions by name: the entities each holds (its ATTDEFs among them) as kind and pairs.
fn blocks(text: &str) -> BTreeMap<String, Vec<(String, Pairs)>> {
    let mut out = BTreeMap::new();
    let mut name: Option<String> = None;
    for (kind, pairs) in records(text, "BLOCKS") {
        match kind.as_str() {
            "BLOCK" => {
                let n = group(&pairs, 2).unwrap_or_default().to_string();
                out.insert(n.clone(), Vec::new());
                name = Some(n);
            }
            "ENDBLK" => name = None,
            _ => {
                if let Some(list) = name.as_ref().and_then(|n| out.get_mut(n)) {
                    list.push((kind, pairs));
                }
            }
        }
    }
    out
}

fn named(text: &str) -> Vec<(Pairs, Vec<Pairs>)> {
    inserts(text).into_iter().filter(|(p, _)| !group(p, 2).unwrap_or("*").starts_with('*')).collect()
}

/// What the sample's doors, windows and furniture are: where each stands and how it is turned, degrees.
fn instances(m: &Model) -> Vec<(P2, Option<f64>)> {
    m.elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Component(c) => Some((c.at, Some(c.angle))),
            Element::Door(_) | Element::Window(_) => m.anchor(id).map(|p| (p, None)),
            _ => None,
        })
        .collect()
}

#[test]
fn each_door_window_and_component_is_an_insert_of_a_block_that_exists() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let (ins, defined) = (named(&text), blocks(&text));
    assert!(ins.len() >= instances(&m).len(), "{} INSERTs for {} instances", ins.len(), instances(&m).len());
    for (p, _) in &ins {
        let name = group(p, 2).unwrap_or_default();
        assert!(!name.is_empty() && !name.starts_with('*'), "{name:?}");
        assert!(defined.get(name).is_some_and(|b| b.iter().any(|(k, _)| STROKES.contains(&k.as_str()))), "{name} holds no geometry");
    }
}

#[test]
fn the_five_chairs_share_one_block_and_each_type_has_its_own() {
    let (m, d) = sample();
    let ins = named(&dxf(&d, &m));
    let mut used: BTreeMap<&str, usize> = BTreeMap::new();
    for (p, _) in &ins {
        *used.entry(group(p, 2).unwrap_or_default()).or_insert(0) += 1;
    }
    // 14 family types, some of which a flipped hand or another wall's thickness draws differently.
    assert!((14..=19).contains(&used.len()), "{} blocks for {} INSERTs: {used:?}", used.len(), ins.len());
    assert!(used.values().any(|n| *n >= 5), "{used:?}");
}

#[test]
fn an_insert_stands_where_its_instance_stands_and_is_turned_by_its_angle() {
    let (m, d) = sample();
    let ins = named(&dxf(&d, &m));
    let mut free: Vec<&Pairs> = ins.iter().map(|(p, _)| p).collect();
    for (at, angle) in instances(&m) {
        let near = |p: &Pairs| P2(number(p, 10).unwrap_or(f64::NAN), number(p, 20).unwrap_or(f64::NAN)).dist(at);
        let (k, p) = free.iter().enumerate().find(|(_, p)| near(p) < 1.0).unwrap_or_else(|| panic!("no INSERT at {at:?}"));
        for code in [41, 42, 43] {
            assert_eq!(number(p, code), Some(1.0), "scale {code}");
        }
        if let Some(angle) = angle {
            let turn = (number(p, 50).unwrap_or(0.0) - angle).rem_euclid(360.0);
            assert!(turn < 1e-3 || 360.0 - turn < 1e-3, "{at:?} is turned {turn} off its angle");
        }
        free.remove(k);
    }
}

#[test]
fn a_doors_block_holds_its_jambs_leaf_and_swing() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let defined = blocks(&text);
    let doors: Vec<_> = named(&text).into_iter().filter(|(p, _)| group(p, 8) == Some("A-DOOR")).collect();
    assert!(doors.len() >= 4);
    for (p, _) in &doors {
        let body = &defined[group(p, 2).unwrap_or_default()];
        let shapes: Vec<_> = body.iter().filter(|(k, _)| STROKES.contains(&k.as_str())).collect();
        assert!(shapes.len() >= 2, "{} has {} entities", group(p, 2).unwrap_or_default(), shapes.len());
        let swing = shapes.iter().any(|(k, pairs)| {
            k == "ARC"
                && number(pairs, 40).is_some_and(|r| r > 100.0)
                && number(pairs, 50).zip(number(pairs, 51)).is_some_and(|(s, e)| {
                    let sweep = (e - s).rem_euclid(360.0);
                    (80.0..190.0).contains(&sweep)
                })
        });
        assert!(swing, "no swing arc in {}", group(p, 2).unwrap_or_default());
    }
}

#[test]
fn the_block_layers_carry_no_strokes_once_their_instances_are_inserts() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let watched = ["A-DOOR", "A-GLAZ", "A-FURN", "A-FLOR-CASE"];
    for (kind, pairs) in records(&text, "ENTITIES") {
        let layer = group(&pairs, 8).unwrap_or_default();
        let on = watched.iter().any(|w| layer == *w || layer.strip_prefix(w).is_some_and(|s| s == "-PATT" || s == "-IDEN"));
        assert!(!(on && STROKES.contains(&kind.as_str())), "{kind} left on {layer}");
    }
}

#[test]
fn every_insert_carries_an_invisible_mark_and_type() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let defined = blocks(&text);
    let ins = named(&text);
    assert!(!ins.is_empty());
    let mut types = Vec::new();
    for (p, attribs) in &ins {
        let tags = |list: Vec<&Pairs>| {
            let mut t: Vec<String> = list.iter().filter_map(|a| group(a, 2)).map(str::to_uppercase).collect();
            t.sort();
            t
        };
        let name = group(p, 2).unwrap_or_default();
        let defs = defined[name].iter().filter(|(k, _)| k == "ATTDEF").map(|(_, a)| a).collect();
        assert_eq!(tags(defs), ["MARK", "TYPE"], "{name}'s ATTDEFs");
        assert_eq!(tags(attribs.iter().collect()), ["MARK", "TYPE"], "{name}'s ATTRIBs");
        assert_eq!(group(p, 66), Some("1"));
        for a in attribs {
            // A visible one would be lettering to a reader that draws them.
            assert_eq!(number(a, 70).map(|f| f as i32 & 1), Some(1), "{a:?}");
            if group(a, 2) == Some("TYPE") {
                types.push(group(a, 1).unwrap_or_default().to_string());
            }
        }
    }
    assert!(types.iter().all(|t| !t.is_empty()), "{types:?}");
    assert!(types.iter().any(|t| t.contains("Chair")) && types.iter().any(|t| t.contains("Casement Window")), "{types:?}");
}

#[test]
fn an_instance_that_cannot_be_placed_keeps_its_ink() {
    let (m, d) = sample();
    let before = named(&dxf(&d, &m)).len();
    let mut broken = (*m).clone();
    let id = broken.elements.iter().find(|(_, e)| matches!(e, Element::Component(_))).map(|(id, _)| *id).unwrap();
    if let Some(Element::Component(c)) = broken.elements.get_mut(id) {
        c.angle = f64::NAN;
    }
    let text = dxf(&d, &broken);
    assert_eq!(named(&text).len(), before - 1);
    // Its strokes are still on the plan, as they were before this slice.
    let strokes =
        records(&text, "ENTITIES").into_iter().filter(|(k, p)| STROKES.contains(&k.as_str()) && group(p, 8) == Some("A-FURN")).count();
    assert!(strokes > 0);
}

#[test]
fn a_plan_with_inserts_reads_back_whole_through_the_core() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let r = crate::dxf::read_text(&text).unwrap();
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
    // The furniture is drawn where the plan draws it, from the blocks, and no attribute is lettering.
    let furn = r.entities.iter().filter(|e| e.layer == "A-FURN").count();
    assert!(furn > 100, "{furn}");
    let on_the_blocks_layers = r
        .entities
        .iter()
        .filter(|e| ["A-DOOR", "A-GLAZ", "A-FURN", "A-FLOR-CASE"].contains(&e.layer.as_str()))
        .filter(|e| matches!(e.shape, crate::dxf::Shape::Text { .. }))
        .count();
    assert_eq!(on_the_blocks_layers, 0, "an attribute was drawn as lettering");
}

#[test]
fn an_insert_stands_where_its_first_stroke_stood_so_the_grid_still_draws_over_a_window() {
    let (m, d) = sample();
    let text = dxf(&d, &m);
    let order: Vec<(String, String)> =
        records(&text, "ENTITIES").into_iter().map(|(k, p)| (k, group(&p, 8).unwrap_or("").to_string())).collect();
    let window = order.iter().position(|(k, l)| k == "INSERT" && l == "A-GLAZ");
    let grid = order.iter().rposition(|(_, l)| l == "A-GRID");
    let (Some(window), Some(grid)) = (window, grid) else { panic!("no window insert or grid line") };
    assert!(window < grid, "the window ({window}) was written after the grid ({grid})");
}
