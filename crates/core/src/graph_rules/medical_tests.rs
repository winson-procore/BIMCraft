//! The medical-day pack: the A112 graph (44 rooms, 49 links, kinds applied) passes MD-001 to MD-011, five hand-broken copies each
//! fail the rule they break, and each rule's edge cases are judged as the done check's own evaluator judges them.

use serde_json::{Value, json};

use super::*;

fn a112() -> Value {
    serde_json::from_str(include_str!("medical_a112.json")).unwrap()
}

fn run(g: &Value) -> Vec<Check> {
    check(Pack::embedded("medical-day").unwrap(), &Graph::from_value(g).unwrap()).unwrap()
}

fn failing(g: &Value) -> Vec<String> {
    run(g).into_iter().filter(|c| c.status == Status::Fail).map(|c| c.id).collect()
}

fn kind_of(g: &Value, k: &str) -> Vec<String> {
    let mut v: Vec<String> =
        g["nodes"].as_array().unwrap().iter().filter(|n| n["kind"] == k).map(|n| n["id"].as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

fn edges(g: &mut Value) -> &mut Vec<Value> {
    g["edges"].as_array_mut().unwrap()
}

fn touches(e: &Value, id: &str) -> bool {
    e["from"] == id || e["to"] == id
}

/// A small legal plan: a bay open to the patient corridor, a treatment room, a staff corridor.
fn tiny() -> Value {
    json!({
        "entry": null,
        "nodes": [
            {"id": "c1", "kind": "corridor"}, {"id": "c2", "kind": "corridor"},
            {"id": "bay", "kind": "treatment_bay"}, {"id": "tr", "kind": "treatment_room"},
            {"id": "wc", "kind": "washroom"}, {"id": "med", "kind": "medication"},
            {"id": "sw", "kind": "staff_work"}, {"id": "of", "kind": "office"},
        ],
        "edges": [
            {"kind": "open", "from": "bay", "to": "c1", "id": "o1"},
            {"kind": "door", "from": "c1", "to": "tr", "operation": "swing", "id": "d1"},
            {"kind": "door", "from": "c1", "to": "wc", "operation": "swing", "id": "d2"},
            {"kind": "door", "from": "c2", "to": "med", "operation": "swing", "id": "d3"},
            {"kind": "open", "from": "c1", "to": "sw", "id": "o2"},
            {"kind": "door", "from": "c2", "to": "of", "operation": "swing", "id": "d4"},
            {"kind": "open", "from": "c1", "to": "c2", "id": "o3"},
        ],
    })
}

#[test]
fn a112_passes_md_001_to_011() {
    let g = a112();
    assert_eq!(g["nodes"].as_array().unwrap().len(), 44);
    assert_eq!(g["edges"].as_array().unwrap().len(), 49);
    let checks = run(&g);
    let ids: Vec<&str> = checks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, (1..=11).map(|i| format!("MD-{i:03}")).collect::<Vec<_>>());
    for c in &checks {
        assert_eq!(c.status, Status::Pass, "{}: {}", c.id, c.detail);
    }
}

#[test]
fn the_small_plan_passes() {
    assert_eq!(failing(&tiny()), Vec::<String>::new());
}

#[test]
fn a_bay_given_a_door_fails_md_001() {
    let mut g = a112();
    let bay = kind_of(&g, "treatment_bay")[0].clone();
    let e = edges(&mut g).iter_mut().find(|e| touches(e, &bay) && e["kind"] == "open").unwrap();
    e["kind"] = json!("door");
    e["operation"] = json!("swing");
    assert!(failing(&g).contains(&"MD-001".to_string()));
}

#[test]
fn clean_and_soiled_joined_fails_md_004() {
    let mut g = a112();
    let (c, s) = (kind_of(&g, "clean_utility")[0].clone(), kind_of(&g, "soiled_utility")[0].clone());
    edges(&mut g).push(json!({"kind": "door", "from": c, "to": s, "operation": "swing", "id": "x"}));
    assert!(failing(&g).contains(&"MD-004".to_string()));
}

#[test]
fn isolation_without_its_anteroom_fails_md_003() {
    let mut g = a112();
    let gone = kind_of(&g, "anteroom");
    g["nodes"].as_array_mut().unwrap().retain(|n| !gone.contains(&n["id"].as_str().unwrap().to_string()));
    edges(&mut g).retain(|e| !gone.iter().any(|a| touches(e, a)));
    assert!(failing(&g).contains(&"MD-003".to_string()));
}

#[test]
fn an_office_open_to_a_corridor_fails_md_007() {
    let mut g = a112();
    let office = kind_of(&g, "office")[0].clone();
    let corridors = kind_of(&g, "corridor");
    let e = edges(&mut g)
        .iter_mut()
        .find(|e| touches(e, &office) && e["kind"] == "door" && corridors.iter().any(|c| touches(e, c)))
        .unwrap();
    e["kind"] = json!("open");
    assert!(failing(&g).contains(&"MD-007".to_string()));
}

#[test]
fn a_link_with_a_room_on_one_side_fails_md_011() {
    let mut g = a112();
    let c = kind_of(&g, "corridor")[0].clone();
    edges(&mut g).push(json!({"kind": "door", "from": c, "to": "exterior", "exterior": true, "operation": "swing", "id": "x"}));
    assert!(failing(&g).contains(&"MD-011".to_string()));
}

#[test]
fn an_edge_to_a_node_not_in_the_graph_fails_md_011() {
    let mut g = tiny();
    edges(&mut g).push(json!({"kind": "door", "from": "c1", "to": "nowhere", "operation": "swing", "id": "x"}));
    assert_eq!(failing(&g), vec!["MD-011".to_string()]);
}

#[test]
fn exterior_doors_listed_fail_md_011() {
    let mut g = tiny();
    g["exterior_doors"] = json!([{"room": "c1"}]);
    assert_eq!(failing(&g), vec!["MD-011".to_string()]);
}

#[test]
fn multi_edges_count_for_medication_and_washrooms() {
    let mut g = tiny();
    edges(&mut g).push(json!({"kind": "door", "from": "c2", "to": "med", "operation": "swing", "id": "x"}));
    assert_eq!(failing(&g), vec!["MD-005".to_string()]);
    let mut g = tiny();
    edges(&mut g).push(json!({"kind": "door", "from": "c1", "to": "wc", "operation": "swing", "id": "x"}));
    assert_eq!(failing(&g), vec!["MD-006".to_string()]);
}

#[test]
fn a_washroom_with_an_open_link_fails_md_006() {
    let mut g = tiny();
    edges(&mut g).push(json!({"kind": "open", "from": "tr", "to": "wc", "id": "x"}));
    assert!(failing(&g).contains(&"MD-006".to_string()));
}

#[test]
fn a_staff_work_area_with_a_door_fails_md_008() {
    let mut g = tiny();
    edges(&mut g).push(json!({"kind": "door", "from": "sw", "to": "tr", "operation": "swing", "id": "x"}));
    assert!(failing(&g).contains(&"MD-008".to_string()));
}

#[test]
fn corridors_apart_fail_md_009() {
    let mut g = tiny();
    edges(&mut g).retain(|e| e["id"] != "o3");
    assert!(failing(&g).contains(&"MD-009".to_string()));
}

#[test]
fn md_009_follows_only_corridors_from_the_first() {
    // c1 and c2 joined only through a room is not a connected set of corridors.
    let mut g = tiny();
    edges(&mut g).retain(|e| e["id"] != "o3");
    edges(&mut g).push(json!({"kind": "door", "from": "c1", "to": "of", "operation": "swing", "id": "x"}));
    assert!(failing(&g).contains(&"MD-009".to_string()));
}

#[test]
fn a_room_cut_off_fails_md_010_but_a_shaft_may_stand_alone() {
    let mut g = tiny();
    g["nodes"].as_array_mut().unwrap().push(json!({"id": "st", "kind": "storage"}));
    assert_eq!(failing(&g), vec!["MD-010".to_string()]);
    let mut g = tiny();
    g["nodes"].as_array_mut().unwrap().push(json!({"id": "sh", "kind": "shaft"}));
    assert_eq!(failing(&g), Vec::<String>::new());
}

#[test]
fn a_graph_with_no_corridor_fails_md_009() {
    let g = json!({"nodes": [{"id": "a", "kind": "storage"}], "edges": []});
    assert!(failing(&g).contains(&"MD-009".to_string()));
}

#[test]
fn a_room_of_no_kind_or_a_kind_outside_the_eighteen_is_refused() {
    let mut g = tiny();
    g["nodes"].as_array_mut().unwrap().push(json!({"id": "x"}));
    assert!(check(Pack::embedded("medical-day").unwrap(), &Graph::from_value(&g).unwrap()).is_err());
    let mut g = tiny();
    g["nodes"].as_array_mut().unwrap().push(json!({"id": "x", "kind": "ballroom"}));
    assert!(check(Pack::embedded("medical-day").unwrap(), &Graph::from_value(&g).unwrap()).is_err());
}

#[test]
fn no_entry_and_no_role_are_needed() {
    let mut g = tiny();
    g.as_object_mut().unwrap().remove("entry");
    assert_eq!(failing(&g), Vec::<String>::new());
}
