//! The studio-office pack: the traced studio passes every rule, each hand-broken copy fails a pack rule, a legal edit passes.

use serde_json::{Value, json};

use super::*;

const SPACES: [(&str, &str, f64); 9] = [
    ("reception", "reception", 51.7),
    ("corridor", "hall", 25.342),
    ("store", "storage", 12.231),
    ("meeting", "meeting", 25.043),
    ("wc_1", "bath", 3.509),
    ("wc_2", "bath", 3.572),
    ("wc_3", "bath", 3.557),
    ("passage", "hall", 14.405),
    ("studio", "office", 182.865),
];

fn door(a: &str, b: &str, op: &str) -> Value {
    json!({"kind": "door", "from": a, "to": b, "operation": op, "width_mm": 800.0})
}

fn adjacent(a: &str, b: &str) -> Value {
    json!({"kind": "adjacent", "from": a, "to": b, "length_mm": 1500.0})
}

/// The traced studio as the extractor writes it, with the approved kinds put on its nodes.
fn traced() -> Value {
    json!({
        "entry": {"room": "reception"},
        "nodes": SPACES.iter().map(|(id, kind, area)| json!({"id": id, "kind": kind, "area_m2": area})).collect::<Vec<_>>(),
        "edges": [
            door("reception", "corridor", "double-swing"), door("corridor", "meeting", "double-swing"),
            door("corridor", "store", "swing"), door("corridor", "studio", "swing"), door("reception", "passage", "swing"),
            door("passage", "wc_1", "swing"), door("passage", "wc_2", "swing"), door("passage", "wc_3", "swing"),
            {"kind": "open", "from": "passage", "to": "studio", "length_mm": 2400.0},
            {"kind": "door", "from": "reception", "to": "exterior", "exterior": true, "operation": "double-swing"},
            {"kind": "door", "from": "studio", "to": "exterior", "exterior": true, "operation": "double-swing"},
            adjacent("corridor", "reception"), adjacent("corridor", "meeting"), adjacent("corridor", "store"),
            adjacent("corridor", "studio"), adjacent("passage", "reception"), adjacent("passage", "studio"),
            adjacent("passage", "wc_1"), adjacent("passage", "wc_2"), adjacent("passage", "wc_3"),
        ],
    })
}

fn run(g: &Value) -> Vec<Check> {
    check(Pack::embedded("studio-office").unwrap(), &Graph::from_value(g).unwrap()).unwrap()
}

fn failing(g: &Value) -> Vec<String> {
    run(g).into_iter().filter(|c| c.status == Status::Fail).map(|c| c.id).collect()
}

fn edges(g: &mut Value) -> &mut Vec<Value> {
    g["edges"].as_array_mut().unwrap()
}

/// Moves the one door `a`-`b` so its end `old` becomes `new`, and the adjacency with it.
fn rehome(g: &mut Value, a: &str, b: &str, old: &str, new: &str) {
    for e in edges(g).iter_mut() {
        if e["kind"] == "door" && e["exterior"] != true && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b) {
            for k in ["from", "to"] {
                if e[k] == old {
                    e[k] = json!(new);
                }
            }
        }
    }
    edges(g).retain(|e| !(e["kind"] == "adjacent" && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b)));
}

#[test]
fn the_pack_has_the_six_kinds_and_twelve_rules_with_their_own_ids() {
    let pack = Pack::embedded("studio-office").unwrap();
    let mut kinds: Vec<&str> = pack.kinds.keys().map(String::as_str).collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["bath", "hall", "meeting", "office", "reception", "storage"]);
    let ids: Vec<&str> = pack.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), 12);
    assert!(ids.iter().all(|i| i.starts_with("SO-")));
    assert!(pack.rules.iter().all(|r| r.classification == "exploratory"));
    assert!(names().contains("studio-office"));
}

#[test]
fn the_traced_studio_passes_every_rule() {
    let checks = run(&traced());
    assert_eq!(checks.len(), 12);
    assert_eq!(failing(&traced()), Vec::<String>::new(), "{checks:?}");
}

#[test]
fn an_entry_that_is_not_the_reception_fails() {
    let mut g = traced();
    g["entry"] = json!({"room": "studio"});
    assert!(failing(&g).contains(&"SO-006".to_string()));
}

#[test]
fn a_meeting_room_only_through_the_studio_fails() {
    let mut g = traced();
    rehome(&mut g, "corridor", "meeting", "corridor", "studio");
    let f = failing(&g);
    assert!(f.contains(&"SO-007".to_string()) && f.contains(&"SO-010".to_string()), "{f:?}");
}

#[test]
fn a_wc_off_the_studio_fails() {
    let mut g = traced();
    rehome(&mut g, "passage", "wc_2", "passage", "studio");
    let f = failing(&g);
    assert!(f.contains(&"SO-007".to_string()) && f.contains(&"SO-005".to_string()) && f.contains(&"SO-008".to_string()), "{f:?}");
}

#[test]
fn a_door_for_the_open_passage_fails() {
    let mut g = traced();
    for e in edges(&mut g) {
        if e["kind"] == "open" {
            *e = door("passage", "studio", "swing");
        }
    }
    assert!(failing(&g).contains(&"SO-009".to_string()));
}

#[test]
fn a_store_off_the_studio_fails() {
    let mut g = traced();
    rehome(&mut g, "corridor", "store", "corridor", "studio");
    let f = failing(&g);
    assert!(f.contains(&"SO-010".to_string()) && f.contains(&"SO-005".to_string()), "{f:?}");
}

#[test]
fn a_fourth_wc_off_the_passage_is_legal() {
    let mut g = traced();
    g["nodes"].as_array_mut().unwrap().push(json!({"id": "wc_4", "kind": "bath", "area_m2": 3.5}));
    edges(&mut g).push(door("passage", "wc_4", "swing"));
    edges(&mut g).push(adjacent("passage", "wc_4"));
    assert_eq!(failing(&g), Vec::<String>::new());
}

#[test]
fn the_means_are_kept_apart() {
    for (a, b, op) in [("corridor", "store", "double-swing"), ("passage", "wc_1", "sliding"), ("reception", "passage", "folding")] {
        let mut g = traced();
        for e in edges(&mut g) {
            if e["kind"] == "door" && e["exterior"] != true && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b) {
                e["operation"] = json!(op);
            }
        }
        assert!(failing(&g).contains(&"SO-005".to_string()), "{a}-{b} {op}");
    }
}

#[test]
fn a_studio_smaller_than_the_reception_fails() {
    let mut g = traced();
    g["nodes"][8]["area_m2"] = json!(40.0);
    assert!(failing(&g).contains(&"SO-012".to_string()));
}

#[test]
fn a_reception_that_joins_no_passage_fails() {
    let mut g = traced();
    rehome(&mut g, "reception", "passage", "reception", "studio");
    assert!(failing(&g).contains(&"SO-011".to_string()));
}
