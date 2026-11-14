//! A room's tag with a leader: the three lines of lettering stand at `tag`, away from the room, with one straight stroke from
//! the tag's nearest edge to the room's point.

use serde_json::{Value, json};

use crate::dxf::{self, Shape};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::Element;
use crate::render::{plan, room_tag_box, room_tag_height};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A 4000 x 3000 box of walls with a room in it, its tag at `tag` (none: inside).
fn boxed(tag: Option<[f64; 2]>) -> (Session, u64) {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let mut p = json!({"point": [2000, 1500], "name": "Closet", "number": "101"});
    if let Some(t) = tag {
        p["tag"] = json!(t);
    }
    let id = run(&mut s, "room", p)["id"].as_u64().unwrap();
    (s, id)
}

fn tag_of(s: &Session, id: u64) -> Option<P2> {
    match s.model.elements.get(id) {
        Some(Element::Room(r)) => r.tag,
        _ => panic!("no room {id}"),
    }
}

#[test]
fn a_room_keeps_its_tag_through_a_save() {
    let (mut s, id) = boxed(Some([9000.0, 1500.0]));
    assert_eq!(tag_of(&s, id), Some(P2(9000.0, 1500.0)));
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-room-tag.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(tag_of(&t, id), Some(P2(9000.0, 1500.0)));
    // A room with no tag writes no key.
    let (s, id) = boxed(None);
    assert_eq!(tag_of(&s, id), None);
    let e = run(&mut { s }, "elements", json!({"category": "room"}));
    assert!(!e.to_string().contains("\"tag\""), "{e}");
}

#[test]
fn a_tag_moves_turns_and_mirrors_with_its_room() {
    let (mut s, id) = boxed(Some([9000.0, 1500.0]));
    run(&mut s, "move", json!({"ids": [id], "by": [100, 200]}));
    assert_eq!(tag_of(&s, id), Some(P2(9100.0, 1700.0)));
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 90}));
    let Some(P2(x, y)) = tag_of(&s, id) else { panic!("no tag") };
    assert!((x + 1700.0).abs() < 1e-6 && (y - 9100.0).abs() < 1e-6, "{x} {y}");
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    let Some(P2(x, y)) = tag_of(&s, id) else { panic!("no tag") };
    assert!((x - 1700.0).abs() < 1e-6 && (y - 9100.0).abs() < 1e-6, "{x} {y}");
}

#[test]
fn a_hostile_tag_is_refused_with_what_to_fix() {
    for tag in [json!([1e308, 0]), json!([0, -1e308]), json!("far"), json!([1]), json!([1, 2, 3]), json!({"x": 1})] {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
        let e = s.execute("room", &json!({"point": [2000, 1500], "tag": tag})).unwrap_err();
        assert!(e.contains("tag"), "{tag}: {e}");
        assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Room(_))).count(), 0, "{tag}");
    }
    // One set in the model directly is caught by the whole-model check.
    let (mut s, id) = boxed(Some([9000.0, 1500.0]));
    if let Some(Element::Room(r)) = s.model_mut().elements.get_mut(id) {
        r.tag = Some(P2(f64::NAN, 0.0));
    }
    assert!(s.model.check().unwrap_err().contains("tag"));
}

fn drawn(s: &Session) -> dxf::Dxf {
    let m = crate::link::seen(&s.model);
    let level = m.levels()[0].0;
    let d = plan(&m, level, 1600, 1000);
    dxf::read_text(&crate::render::dxf::dxf(&d, &m)).unwrap()
}

fn on_iden(r: &dxf::Dxf) -> (Vec<(String, f64)>, Vec<Vec<P2>>, usize) {
    let (mut texts, mut strokes, mut other) = (Vec::new(), Vec::new(), 0);
    for e in r.entities.iter().filter(|e| e.layer == "A-AREA-IDEN") {
        match &e.shape {
            Shape::Text { text, height, .. } => texts.push((text.clone(), *height)),
            Shape::Polyline { pts, closed: false } => strokes.push(pts.clone()),
            _ => other += 1,
        }
    }
    (texts, strokes, other)
}

fn text_at(r: &dxf::Dxf, word: &str) -> P2 {
    r.entities
        .iter()
        .find_map(|e| match &e.shape {
            Shape::Text { text, at, .. } if text == word => Some(*at),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {word}"))
}

#[test]
fn a_tag_away_from_its_room_is_three_texts_and_one_leader_into_the_room() {
    let (s, _) = boxed(Some([9000.0, 1500.0]));
    let r = drawn(&s);
    let (texts, strokes, other) = on_iden(&r);
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert_eq!(strokes.len(), 1, "{strokes:?}");
    assert_eq!(other, 0, "nothing but texts and the one stroke");
    let h = texts.iter().map(|t| t.1).fold(0.0, f64::max);
    assert!((h - room_tag_height(100.0)).abs() < 0.5, "{h}");
    // The tag is where it was put: the number's anchor is at the tag's point.
    let at = text_at(&r, "101");
    assert!((at.0 - 9000.0).abs() < 1.0 && (at.1 - 1500.0).abs() < 1.0, "{at:?}");
    // The leader ends at the room's point and starts on the tag's box, on the side that faces the room.
    let lines: Vec<String> = texts.iter().map(|t| t.0.clone()).collect();
    let lines = [lines[0].clone(), lines[1].clone(), lines[2].clone()];
    let names = ["Closet", "101"];
    assert!(names.iter().all(|n| lines.iter().any(|l| l == n)));
    let area = lines.iter().find(|l| l.ends_with("m²")).unwrap().clone();
    let bx = room_tag_box(P2(9000.0, 1500.0), h, &["Closet".into(), "101".into(), area]);
    let st = &strokes[0];
    let (a, b) = (st[0], st[1]);
    let (start, end) = if (a.0 - 2000.0).abs() < 1.0 { (b, a) } else { (a, b) };
    assert!((end.0 - 2000.0).abs() < 1.0 && (end.1 - 1500.0).abs() < 1.0, "{end:?}");
    // On the box's west edge (the room is west of it), level with the room's point where the edge reaches that far.
    assert!((start.0 - bx[0]).abs() < 1.0 && (start.1 - 1500.0).abs() < 1.0 && bx[1] < 1500.0 && bx[3] > 1500.0, "{start:?} {bx:?}");
}

#[test]
fn a_leader_that_crosses_a_wall_is_still_one_stroke() {
    let (mut s, _) = boxed(Some([9000.0, 1500.0]));
    run(&mut s, "wall", json!({"points": [[3000, 0], [3000, 3000]]}));
    run(&mut s, "door", json!({"at": [3000, 1500], "mark": "D1"}));
    let (_, strokes, other) = on_iden(&drawn(&s));
    assert_eq!((strokes.len(), other), (1, 0));
}

#[test]
fn a_room_with_no_tag_draws_no_stroke() {
    let (s, _) = boxed(None);
    let (texts, strokes, other) = on_iden(&drawn(&s));
    assert_eq!((texts.len(), strokes.len(), other), (3, 0, 0));
}

#[test]
fn the_tags_box_is_the_texts_middle_and_wide_as_the_longest_line() {
    let b = room_tag_box(P2(0.0, 0.0), 260.0, &["Hall".into(), "104".into(), "16.62 m²".into()]);
    // 8 characters at 0.6 H, 4.5 H high, its middle 35 mm over the anchor at 1 : 100.
    assert!((b[2] - b[0] - 0.6 * 260.0 * 8.0).abs() < 1e-9);
    assert!((b[3] - b[1] - 4.5 * 260.0).abs() < 1e-9);
    assert!((((b[1] + b[3]) / 2.0) - 35.0).abs() < 1e-9);
    assert!(((b[0] + b[2]) / 2.0).abs() < 1e-9);
    assert_eq!(room_tag_height(100.0), 260.0);
    assert_eq!(room_tag_height(150.0), 390.0);
}
