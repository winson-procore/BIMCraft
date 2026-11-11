//! The lift from the 2D topology to the 3D/BIM graph: what the two files say about themselves, that every 2D space and wall
//! and opening can be followed from the building back to the plan, and that each rule on the lift fails on the fault it names.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::Built;
use super::graph::Topology;
use super::lift::Building;
use super::report::{self, Status};
use super::tests::{BRIEFS, archetype, built, doc};

fn lifted(name: &str, seed: u64, area: f64) -> (Built, Topology, Building) {
    let b = built(archetype(name), seed, area);
    let topology = Topology::of(&b.plan);
    let building = Building::lift(&b.plan, &topology);
    (b, topology, building)
}

fn ids(v: &Value, key: &str) -> BTreeSet<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|x| x["id"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn lift_status(b: &Building, t: &Topology) -> BTreeMap<&'static str, Status> {
    report::lift_checks(b, t).into_iter().map(|c| (c.id, c.status)).collect()
}

#[test]
fn the_two_graphs_say_which_is_2d_and_which_is_3d() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        assert_eq!(doc(&b, "topology-graph.json")["graph_kind"], "2d_topology", "{name} {seed}");
        assert_eq!(doc(&b, "building-ir.json")["graph_kind"], "3d_bim", "{name} {seed}");
        assert_eq!(doc(&b, "program-graph.json").get("graph_kind"), None, "the program graph is neither");
    }
}

#[test]
fn every_2d_space_is_lifted_onto_a_level() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (top, ir) = (doc(&b, "topology-graph.json"), doc(&b, "building-ir.json"));
        let label = format!("{name} {seed} {area}");
        let levels = ids(&ir, "levels");
        assert!(!levels.is_empty(), "{label}");
        for l in ir["levels"].as_array().unwrap() {
            assert!(l["elevation_mm"].is_i64(), "{label}: a level has its elevation in whole millimetres");
            assert!(l["name"].is_string(), "{label}");
        }
        let nodes = ids(&top, "nodes");
        let lifted: Vec<String> =
            ir["spaces"].as_array().unwrap().iter().filter_map(|s| s["source_2d"].as_str().map(str::to_string)).collect();
        assert_eq!(lifted.len(), nodes.len(), "{label}: one 3D space for each 2D one");
        assert_eq!(lifted.iter().cloned().collect::<BTreeSet<_>>(), nodes, "{label}: the corridor is lifted too, and nothing else is");
        for s in ir["spaces"].as_array().unwrap() {
            assert!(levels.contains(s["level"].as_str().unwrap_or_default()), "{label}: {s}");
            assert!(s["base_mm"].as_i64() < s["top_mm"].as_i64() && s["base_mm"].is_i64(), "{label}: {s}");
        }
        assert_eq!(ir["source_2d"]["topology_hash"], b.topology_hash.as_str(), "{label}: the building names the plan it came from");
    }
}

#[test]
fn every_3d_wall_names_the_2d_it_came_from() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (top, ir) = (doc(&b, "topology-graph.json"), doc(&b, "building-ir.json"));
        let label = format!("{name} {seed} {area}");
        let (nodes, levels, walls) = (ids(&top, "nodes"), ids(&ir, "levels"), ids(&ir, "walls"));
        assert_eq!(walls.len(), ir["walls"].as_array().unwrap().len(), "{label}: wall ids are unique");
        assert!(walls.len() >= 4 + 2, "{label}: four outside walls and two for the corridor, at least");
        for w in ir["walls"].as_array().unwrap() {
            let refs = strings(&w["source_2d_refs"]);
            assert!(!refs.is_empty() && refs.iter().all(|r| nodes.contains(r)), "{label}: {w}");
            assert!(levels.contains(w["level"].as_str().unwrap_or_default()), "{label}: {w}");
            assert!(w["base_mm"].as_i64() < w["top_mm"].as_i64() && w["base_mm"].is_i64(), "{label}: {w}");
            assert!(w["thickness_mm"].as_i64().unwrap_or(0) > 0 && w["type"].is_string(), "{label}: {w}");
        }
        let by_id: BTreeMap<&str, &Value> = ir["walls"].as_array().unwrap().iter().filter_map(|w| Some((w["id"].as_str()?, w))).collect();
        for id in ["wall_ext_west", "wall_ext_north", "wall_ext_east", "wall_ext_south", "wall_corr_south", "wall_corr_north"] {
            assert!(by_id.contains_key(id), "{label}: {id}");
        }
        for id in ["wall_corr_south", "wall_corr_north"] {
            assert!(
                strings(&by_id[id]["source_2d_refs"]).len() >= 2,
                "{label}: a corridor wall bounds the corridor and the rooms along it"
            );
        }
        // A partition stands between the two rooms it is named for.
        for (id, w) in &by_id {
            if let Some(room) = id.strip_prefix("wall_part_") {
                let refs = strings(&w["source_2d_refs"]);
                assert!(refs.iter().any(|r| r == room) && refs.len() == 2, "{label}: {id} {refs:?}");
            }
        }
    }
}

#[test]
fn wall_ids_come_from_where_a_wall_stands_and_not_from_the_seed() {
    let (_, _, a) = lifted("office", 1, 900.0);
    let (_, _, b) = lifted("office", 2, 900.0);
    let fixed =
        |b: &Building| -> BTreeSet<String> { b.walls.iter().map(|w| w.id.clone()).filter(|i| !i.starts_with("wall_part_")).collect() };
    assert_eq!(fixed(&a), fixed(&b));
    assert_eq!(fixed(&a).len(), 6);
}

#[test]
fn every_opening_names_its_host_wall_and_where_it_came_from() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let (top, ir) = (doc(&b, "topology-graph.json"), doc(&b, "building-ir.json"));
        let label = format!("{name} {seed} {area}");
        let (nodes, walls) = (ids(&top, "nodes"), ids(&ir, "walls"));
        let edges = ids(&top, "edges");
        let openings = ir["openings"].as_array().unwrap();
        assert!(!openings.is_empty(), "{label}");
        let mut seen = BTreeSet::new();
        for o in openings {
            assert!(seen.insert(o["id"].as_str().unwrap_or_default().to_string()), "{label}: ids are unique");
            assert!(walls.contains(o["host"].as_str().unwrap_or_default()), "{label}: {o}");
            assert!(nodes.contains(o["source_2d"].as_str().unwrap_or_default()), "{label}: {o}");
            match o["kind"].as_str() {
                // A room's door is the door edge of the topology it is the lift of.
                Some("door") if o["entry"] != true => {
                    assert!(edges.contains(o["source_topology_edge"].as_str().unwrap_or_default()), "{label}: {o}")
                }
                Some("door" | "window") => assert!(o["source_topology_edge"].is_null(), "{label}: {o}"),
                other => panic!("{label}: an opening of kind {other:?}"),
            }
        }
        let doors = openings.iter().filter(|o| o["kind"] == "door").count();
        assert_eq!(doors, top["nodes"].as_array().unwrap().len(), "{label}: a door for every room but the corridor, and the way in");
    }
}

#[test]
fn the_slab_is_on_a_level_and_nothing_is_added_to_pad_the_graph() {
    for (name, seed, area) in BRIEFS {
        let b = built(archetype(name), seed, area);
        let ir = doc(&b, "building-ir.json");
        let (levels, slabs) = (ids(&ir, "levels"), ir["slabs"].as_array().unwrap());
        assert_eq!(slabs.len(), 1, "{name} {seed}: the one floor the script makes");
        assert!(levels.contains(slabs[0]["level"].as_str().unwrap_or_default()));
        assert_eq!(slabs[0]["kind"], "floor");
        for none in ["vertical_circulation", "components", "systems"] {
            assert_eq!(ir[none].as_array().map(Vec::len), Some(0), "{name} {seed}: no {none} is made, and the file says so");
        }
    }
}

#[test]
fn each_lift_rule_fails_on_a_graph_that_breaks_it_and_only_that_one() {
    let (_, topology, good) = lifted("office", 1, 900.0);
    let rules = ["LIFT-001", "LIFT-002", "LIFT-003", "LIFT-004"];
    let all = lift_status(&good, &topology);
    assert_eq!(all.keys().copied().collect::<Vec<_>>(), rules);
    assert!(all.values().all(|s| *s == Status::Pass), "{all:?}");

    let with = |change: &dyn Fn(&mut Building)| {
        let mut b = good.clone();
        change(&mut b);
        b
    };
    let other_wall = good.walls.iter().find(|w| w.id == "wall_ext_south").map(|w| w.id.clone()).unwrap();
    let a_door = good.openings.iter().position(|o| o.kind == "door" && !o.entry).unwrap();
    let cases: Vec<(&str, Building)> = vec![
        ("LIFT-001", with(&|b| drop(b.spaces.pop()))),
        ("LIFT-001", with(&|b| b.spaces[0].source_2d = "nowhere".into())),
        ("LIFT-001", with(&|b| b.spaces[0].level = "level_99".into())),
        ("LIFT-001", with(&|b| b.spaces[1].top_mm = b.spaces[1].base_mm)),
        ("LIFT-002", with(&|b| b.walls[0].source_2d_refs.clear())),
        ("LIFT-002", with(&|b| b.walls[0].source_2d_refs.push("nowhere".into()))),
        ("LIFT-002", with(&|b| b.walls[0].level = "level_99".into())),
        ("LIFT-002", with(&|b| b.walls[0].thickness_mm = 0)),
        ("LIFT-002", with(&|b| b.walls[0].top_mm = b.walls[0].base_mm)),
        ("LIFT-002", with(&|b| b.walls.last_mut().into_iter().for_each(|w| w.id = "wall_ext_west".into()))),
        ("LIFT-003", with(&|b| b.openings[a_door].host = "wall_nowhere".into())),
        ("LIFT-003", with(&|b| b.openings[a_door].host = other_wall.clone())),
        ("LIFT-003", with(&|b| b.openings[a_door].source_2d = "nowhere".into())),
        ("LIFT-003", with(&|b| b.openings[a_door].source_edge = Some("door:nowhere:nowhere".into()))),
        ("LIFT-003", with(&|b| b.openings[a_door].source_edge = None)),
        ("LIFT-003", with(&|b| b.openings.clear())),
        ("LIFT-004", with(&|b| b.slabs[0].level = "level_99".into())),
        ("LIFT-004", with(&|b| b.slabs.clear())),
    ];
    for (broken, building) in &cases {
        let got = lift_status(building, &topology);
        for id in rules {
            let want = if id == *broken { Status::Fail } else { Status::Pass };
            assert_eq!(got[id], want, "{broken} broken, rule {id}: {got:?}");
        }
    }
    // A building with fewer than four walls is no building, whatever else it holds.
    let few = with(&|b| b.walls.truncate(3));
    let c = report::lift_checks(&few, &topology).into_iter().find(|c| c.id == "LIFT-002").unwrap();
    assert!(c.status == Status::Fail && c.detail.contains("fewer than 4"), "{}", c.detail);
    // The way in has no door edge, and needs none.
    let entry = good.openings.iter().position(|o| o.entry).unwrap();
    assert_eq!(good.openings[entry].source_edge, None);
}

#[test]
fn a_failed_lift_rule_says_what_to_fix() {
    let (_, topology, good) = lifted("clinic", 3, 650.0);
    let mut b = good.clone();
    b.walls[2].source_2d_refs.clear();
    let c = report::lift_checks(&b, &topology).into_iter().find(|c| c.id == "LIFT-002").unwrap();
    assert_eq!(c.status, Status::Fail);
    assert!(c.detail.contains(&b.walls[2].id), "the wall is named: {}", c.detail);
}
