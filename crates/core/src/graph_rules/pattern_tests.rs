//! The office-pattern pack: a graph of the pattern passes every rule, each hand-broken copy fails a pack rule that the base
//! passed, a legal edit passes.

use serde_json::{Value, json};

use super::*;

const SPACES: [(&str, &str, f64); 8] = [
    ("lobby", "lobby", 28.0),
    ("corridor", "hall", 30.0),
    ("open_office", "open_office", 90.0),
    ("private_office_1", "private_office", 15.0),
    ("conference_1", "conference", 20.0),
    ("restroom_1", "restroom", 10.0),
    ("pantry_1", "pantry", 12.0),
    ("focus_room_1", "focus_room", 9.0),
];

fn door(a: &str, b: &str, op: &str) -> Value {
    json!({"kind": "door", "from": a, "to": b, "operation": op, "width_mm": 900.0, "id": format!("door:{a}:{b}")})
}

fn base() -> Value {
    let mut edges = vec![
        door("lobby", "corridor", "double-swing"),
        door("corridor", "open_office", "double-swing"),
        json!({"kind": "door", "from": "lobby", "to": "exterior", "exterior": true, "operation": "double-swing", "id": "door:lobby:exterior"}),
    ];
    for room in ["private_office_1", "conference_1", "restroom_1", "pantry_1", "focus_room_1"] {
        edges.push(door("corridor", room, "swing"));
    }
    json!({
        "entry": {"room": "lobby"},
        "nodes": SPACES.iter().map(|(id, kind, area)| json!({"id": id, "kind": kind, "area_m2": area})).collect::<Vec<_>>(),
        "edges": edges,
    })
}

fn run(g: &Value) -> Vec<Check> {
    check(Pack::embedded("office-pattern").unwrap(), &Graph::from_value(g).unwrap()).unwrap()
}

fn failing(g: &Value) -> Vec<String> {
    run(g).into_iter().filter(|c| c.status == Status::Fail).map(|c| c.id).collect()
}

fn edges(g: &mut Value) -> &mut Vec<Value> {
    g["edges"].as_array_mut().unwrap()
}

/// Moves the one door `a`-`b` so its end `old` becomes `new`.
fn rehome(g: &mut Value, a: &str, b: &str, old: &str, new: &str) {
    let mut moved = 0;
    for e in edges(g).iter_mut() {
        if e["kind"] == "door" && e["exterior"] != true && [&e["from"], &e["to"]].iter().all(|x| *x == a || *x == b) {
            for k in ["from", "to"] {
                if e[k] == old {
                    e[k] = json!(new);
                }
            }
            moved += 1;
        }
    }
    assert_eq!(moved, 1);
}

#[test]
fn a_graph_of_the_pattern_passes_every_rule() {
    let checks = run(&base());
    assert!(checks.len() >= 5);
    assert!(checks.iter().all(|c| c.status == Status::Pass), "{checks:?}");
    assert_eq!(Pack::embedded("office-pattern").unwrap().rules.len(), 12);
}

#[test]
fn a_second_way_in_fails_the_one_exterior_door_rule() {
    let mut g = base();
    g["edges"].as_array_mut().unwrap().push(json!({"kind": "door", "from": "open_office", "to": "exterior", "exterior": true, "operation": "swing", "id": "door:open_office:exterior-x"}));
    assert!(failing(&g).contains(&"OP-006".to_string()));
}

#[test]
fn a_way_in_off_another_room_fails() {
    let mut g = base();
    for e in edges(&mut g) {
        if e["exterior"] == true {
            e["from"] = json!("corridor");
        }
    }
    assert!(failing(&g).contains(&"OP-006".to_string()));
}

#[test]
fn a_private_office_with_no_door_to_the_corridor_fails() {
    let mut g = base();
    rehome(&mut g, "corridor", "private_office_1", "corridor", "lobby");
    let failed = failing(&g);
    for rule in ["OP-005", "OP-009", "OP-010"] {
        assert!(failed.contains(&rule.to_string()), "{rule} in {failed:?}");
    }
}

#[test]
fn an_open_office_off_the_lobby_fails() {
    let mut g = base();
    rehome(&mut g, "corridor", "open_office", "corridor", "lobby");
    let failed = failing(&g);
    assert!(failed.contains(&"OP-011".to_string()), "{failed:?}");
}

#[test]
fn a_side_room_off_the_open_office_fails() {
    let mut g = base();
    rehome(&mut g, "corridor", "pantry_1", "corridor", "open_office");
    assert!(failing(&g).contains(&"OP-011".to_string()));
}

#[test]
fn a_plan_with_no_restroom_fails_the_counts() {
    let mut g = base();
    g["nodes"].as_array_mut().unwrap().retain(|n| n["id"] != "restroom_1");
    edges(&mut g).retain(|e| e["to"] != "restroom_1");
    assert_eq!(failing(&g), ["OP-003"]);
}

#[test]
fn a_legal_edit_still_passes() {
    // No focus room is as good a plan.
    let mut g = base();
    g["nodes"].as_array_mut().unwrap().retain(|n| n["id"] != "focus_room_1");
    edges(&mut g).retain(|e| e["to"] != "focus_room_1");
    assert_eq!(failing(&g), Vec::<String>::new());
    // So is a lobby entry that is the entry by name, or a conference room with a double door.
    let mut g = base();
    for e in edges(&mut g) {
        if e["to"] == "conference_1" {
            e["operation"] = json!("double-swing");
        }
    }
    assert_eq!(failing(&g), Vec::<String>::new());
}

#[test]
fn an_unreached_room_and_a_wrong_entry_fail() {
    let mut g = base();
    edges(&mut g).retain(|e| e["to"] != "pantry_1");
    assert!(failing(&g).contains(&"OP-012".to_string()));
    let mut g = base();
    g["entry"] = json!({"room": "open_office"});
    assert!(failing(&g).contains(&"OP-007".to_string()));
}
