//! The extractor's swing fields: `opens_into` and `hinge` on swing doors, and only on them.

use serde_json::Value;

use super::extract;
use crate::engine::Session;

fn script(reverse: bool, sliding: bool, flips: &str) -> String {
    let wall = if reverse { "[[6000,0],[0,0]]" } else { "[[0,0],[6000,0]]" };
    let (fam, typ) = if sliding { ("Sliding Door", "1500 x 2100") } else { ("Single Door", "800 x 2100") };
    let mut s = String::new();
    s += "new {\"name\":\"Two rooms\"}\nroom_area {\"at\":\"finish\"}\nlevel {\"elevation\":3600,\"name\":\"Roof\"}\n";
    for (a, b) in [((0, -3000), (6000, -3000)), ((6000, -3000), (6000, 3000)), ((6000, 3000), (0, 3000)), ((0, 3000), (0, -3000))] {
        s += &format!("wall {{\"points\":[[{},{}],[{},{}]],\"type\":\"Exterior - 300mm\"}}\n", a.0, a.1, b.0, b.1);
    }
    s += &format!("wall {{\"points\":{wall},\"type\":\"Interior - 100mm\"}}\n");
    s += "family {\"name\":\"Single Door\",\"types\":{\"800 x 2100\":{\"Height\":2100,\"Width\":800}}}\n";
    s += "family {\"name\":\"Sliding Door\",\"types\":{\"1500 x 2100\":{\"Height\":2100,\"Width\":1500}}}\n";
    for (mark, at) in [("D1", "[3000,0]"), ("D2", "[1500,-3000]")] {
        s += &format!("door {{\"at\":{at},\"family\":\"{fam}\",\"type\":\"{typ}\",\"mark\":\"{mark}\"{flips}}}\n");
    }
    s += "room {\"point\":[3000,1500],\"name\":\"North\",\"number\":\"north\"}\n";
    s += "room {\"point\":[3000,-1500],\"name\":\"South\",\"number\":\"south\"}\n";
    s
}

fn graph(reverse: bool, sliding: bool, flips: &str) -> Value {
    let mut s = Session::new();
    s.script(&script(reverse, sliding, flips)).expect("script");
    extract(&s.model)
}

fn pair(e: &Value) -> (Option<&str>, Option<&str>) {
    (e["opens_into"].as_str(), e["hinge"].as_str())
}

fn door<'a>(list: &'a Value, mark: &str) -> &'a Value {
    list.as_array().and_then(|l| l.iter().find(|e| e["mark"] == mark)).expect("door")
}

fn check(g: &Value, d1: (&str, &str), d2: (&str, &str)) {
    for (mark, want) in [("D1", d1), ("D2", d2)] {
        assert_eq!(pair(door(&g["edges"], mark)), (Some(want.0), Some(want.1)), "{mark} edge");
    }
    assert_eq!(pair(door(&g["exterior_doors"], "D2")), (Some(d2.0), Some(d2.1)), "D2 exterior_doors entry");
}

#[test]
fn base_opens_into_the_left_room_hinge_start() {
    check(&graph(false, false, ""), ("north", "start"), ("south", "start"));
}

#[test]
fn flip_facing_opens_into_the_right_room_or_the_exterior() {
    check(&graph(false, false, ",\"flip_facing\":true"), ("south", "start"), ("exterior", "start"));
}

#[test]
fn flip_hand_moves_the_hinge() {
    check(&graph(false, false, ",\"flip_hand\":true"), ("north", "end"), ("south", "end"));
}

#[test]
fn a_reversed_wall_swaps_the_sides() {
    check(&graph(true, false, ""), ("south", "start"), ("south", "start"));
}

#[test]
fn sliding_doors_carry_neither_field() {
    let g = graph(false, true, "");
    let mut n = 0;
    for e in g["edges"].as_array().unwrap().iter().chain(g["exterior_doors"].as_array().unwrap()) {
        if e["kind"] == "door" {
            n += 1;
            assert!(e.get("opens_into").is_none() && e.get("hinge").is_none(), "{e}");
        }
    }
    assert!(n >= 2);
}
