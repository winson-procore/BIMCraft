//! The trace contract: `topology` with `expect`, driven through commands as a front end drives it.

use serde_json::{Value, json};

use crate::engine::Session;

/// An 8 m x 4 m box with a partition at x = 4000, rooms "A" (west) and "B" (east), then `extra`.
fn house(extra: &str) -> Session {
    let mut s = Session::new();
    let script = format!(
        "wall {{\"points\": [[0, 0], [0, 4000], [8000, 4000], [8000, 0]], \"closed\": true, \"type\": \"Exterior - 300mm\"}}\n\
         wall {{\"points\": [[4000, 0], [4000, 4000]], \"type\": \"Interior - 100mm\"}}\n\
         room {{\"point\": [2000, 2000], \"name\": \"West\", \"number\": \"A\"}}\n\
         room {{\"point\": [6000, 2000], \"name\": \"East\", \"number\": \"B\"}}\n{extra}\n"
    );
    s.script(&script).unwrap();
    s
}

/// A swing door between the rooms, and an exterior door into A.
fn doors() -> Session {
    house(
        "door {\"at\": [4000, 2000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}\n\
         door {\"at\": [0, 2000], \"family\": \"Single Door\", \"type\": \"900 x 2100\"}",
    )
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-expect-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn expected(links: Value, entry: &str) -> Value {
    json!({
        "building": "school", "review": "x", "source": "a trace",
        "rooms": [{"id": "A", "name": "West", "kind": "classroom"}, {"id": "B", "name": "East", "kind": "hall"}],
        "links": links,
        "entry": {"room": entry, "via": "fixed"},
    })
}

fn compare_with(s: &mut Session, name: &str, file: &Value) -> Value {
    let dir = scratch(name);
    let path = dir.join("e.json");
    std::fs::write(&path, file.to_string()).unwrap();
    let r = s.execute("topology", &json!({"expect": path})).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    r["comparison"].clone()
}

fn empty(c: &Value, k: &str) -> bool {
    c[k].as_array().is_some_and(Vec::is_empty)
}

fn swing_ab() -> Value {
    json!([{"from": "A", "to": "B", "via": "swing"}])
}

#[test]
fn expect_agrees_on_the_house() {
    let mut s = doors();
    let c = compare_with(&mut s, "agree", &expected(json!([{"from": "B", "to": "A", "via": "swing"}]), "A"));
    assert_eq!(c["agree"], true, "{c}");
    for k in ["missing_rooms", "extra_rooms", "missing_links", "extra_links", "diagnostics"] {
        assert!(empty(&c, k), "{k}: {c}");
    }
    assert!(c["entry"].is_null());
}

#[test]
fn a_link_left_out_of_the_expected_file_is_in_missing_links() {
    let mut s = doors();
    let c = compare_with(&mut s, "missing", &expected(json!([]), "A"));
    assert_eq!(c["agree"], false);
    assert_eq!(c["missing_links"], swing_ab(), "{c}");
    assert!(empty(&c, "extra_links") && empty(&c, "missing_rooms") && c["entry"].is_null());
}

#[test]
fn a_link_the_model_lacks_is_in_extra_links() {
    let mut s = doors();
    let links = json!([{"from": "A", "to": "B", "via": "swing"}, {"from": "A", "to": "B", "via": "open"}]);
    let c = compare_with(&mut s, "extra", &expected(links, "A"));
    assert_eq!(c["extra_links"], json!([{"from": "A", "to": "B", "via": "open"}]), "{c}");
    assert!(empty(&c, "missing_links"));
}

#[test]
fn sliding_vs_swing_is_a_mismatch() {
    let mut s = doors();
    let c = compare_with(&mut s, "via", &expected(json!([{"from": "A", "to": "B", "via": "sliding"}]), "A"));
    assert_eq!(c["missing_links"], swing_ab(), "{c}");
    assert_eq!(c["extra_links"], json!([{"from": "A", "to": "B", "via": "sliding"}]), "{c}");
}

#[test]
fn entry_names_both_rooms() {
    let mut s = doors();
    let c = compare_with(&mut s, "entry", &expected(swing_ab(), "B"));
    assert_eq!(c["entry"], json!({"expected": "B", "extracted": "A"}), "{c}");
    assert_eq!(c["agree"], false);
    assert!(empty(&c, "missing_links") && empty(&c, "extra_links"));
}

#[test]
fn kind_building_review_source_and_entry_via_are_not_compared() {
    // `expected` carries kinds no house has, an unknown building, review, source and an entry via.
    let mut s = doors();
    let c = compare_with(&mut s, "ignored", &expected(swing_ab(), "A"));
    assert_eq!(c["agree"], true, "{c}");
}

#[test]
fn a_room_the_model_lacks_is_extra_and_one_it_has_is_missing() {
    let mut s = doors();
    let mut e = expected(swing_ab(), "A");
    e["rooms"] = json!([{"id": "A", "kind": "k"}, {"id": "Z", "kind": "k"}]);
    e["links"] = json!([]);
    let c = compare_with(&mut s, "rooms", &e);
    assert_eq!(c["missing_rooms"], json!(["B"]), "{c}");
    assert_eq!(c["extra_rooms"], json!(["Z"]), "{c}");
}

#[test]
fn diagnostics_make_a_mismatch() {
    let mut s = doors();
    s.execute("room", &json!({"point": [-5000, -5000], "name": "Outside", "number": "X"})).unwrap();
    let c = compare_with(&mut s, "diag", &expected(swing_ab(), "A"));
    assert_eq!(c["agree"], false);
    assert_eq!(c["diagnostics"][0]["code"], "room_not_enclosed", "{c}");
}

#[test]
fn exterior_door_is_never_a_link() {
    // The model has an exterior door into A; the expected file lists no link for it and still agrees.
    let mut s = doors();
    let c = compare_with(&mut s, "exterior", &expected(swing_ab(), "A"));
    assert!(empty(&c, "missing_links") && empty(&c, "extra_links"), "{c}");
}

#[test]
fn path_is_written_even_on_a_mismatch() {
    let mut s = doors();
    let dir = scratch("path");
    let (plain, both, e) = (dir.join("plain.json"), dir.join("both.json"), dir.join("e.json"));
    std::fs::write(&e, expected(json!([]), "B").to_string()).unwrap();
    s.execute("topology", &json!({"path": plain})).unwrap();
    let r = s.execute("topology", &json!({"path": both, "expect": e})).unwrap();
    assert_eq!(r["comparison"]["agree"], false);
    let same = std::fs::read(&plain).unwrap() == std::fs::read(&both).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(same, "the graph written must not hold the comparison");
}

#[test]
fn the_expected_file_is_read_strictly() {
    let mut s = doors();
    let mut cases: Vec<(&str, Value, &str)> = Vec::new();
    let mut e = expected(swing_ab(), "A");
    e["links"][0]["via"] = json!("slide");
    cases.push(("via", e, "links[0].via"));
    let mut e = expected(swing_ab(), "A");
    e["rooms"][1]["id"] = json!("A");
    cases.push(("dup", e, "rooms[1].id"));
    let mut e = expected(swing_ab(), "A");
    e["links"][0]["to"] = json!("Q");
    cases.push(("unknown", e, "links[0].to"));
    let mut e = expected(swing_ab(), "A");
    e["links"][0]["to"] = json!("exterior");
    cases.push(("exterior", e, "links[0].to"));
    let mut e = expected(swing_ab(), "A");
    e["entry"]["room"] = json!("Q");
    cases.push(("entry", e, "entry.room"));
    let mut e = expected(swing_ab(), "A");
    e["link"] = json!([]);
    cases.push(("typo", e, "link"));
    let mut e = expected(swing_ab(), "A");
    e["rooms"][0]["kind"] = json!("");
    cases.push(("kind", e, "rooms[0].kind"));
    for (name, file, says) in cases {
        let dir = scratch(name);
        let path = dir.join("e.json");
        std::fs::write(&path, file.to_string()).unwrap();
        let err = s.execute("topology", &json!({"expect": path})).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(err.contains(says), "{name}: {err}");
    }
    let dir = scratch("raw");
    let path = dir.join("e.json");
    std::fs::write(&path, "not json").unwrap();
    assert!(s.execute("topology", &json!({"expect": path})).is_err());
    let big = dir.join("big.json");
    std::fs::write(&big, vec![b' '; 17 * 1024 * 1024]).unwrap();
    let err = s.execute("topology", &json!({"expect": big})).unwrap_err();
    assert!(err.contains("at most"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(s.execute("topology", &json!({"expect": "/nonexistent/e.json"})).is_err());
}
