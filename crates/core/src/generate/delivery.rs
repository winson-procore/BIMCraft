//! `generate --delivery on`: the plan a generator built, again to the delivery drawing rules. The layout is the generator's own and
//! untouched; only what is drawn changes. The script gains the `cad_profile` line, a room is numbered `101` upward in the order of
//! the graph's nodes, and what a delivery drawing may not hold (dimensions) is left out. The graph, the building and the
//! manifest are written again to match. Detail: `docs/areas/graph-generator.md`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;

use super::{Built, MANIFEST, Refusal, fnv1a64, pretty, refuse, shape_graph};

/// The most rooms a delivery plan numbers: `101` to `999`.
const MAX_NUMBERED: usize = 899;

fn text_of<'a>(built: &'a Built, name: &str) -> Result<&'a str, Refusal> {
    built
        .files
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| t.as_str())
        .ok_or_else(|| refuse("generation_failed", format!("the plan has no {name}")))
}

fn json_of(built: &Built, name: &str) -> Result<Value, Refusal> {
    serde_json::from_str(text_of(built, name)?).map_err(|e| refuse("generation_failed", format!("{name} is not JSON: {e}")))
}

/// The number a node is drawn with: `101` upward, in node order.
pub(crate) fn number_of(index: usize) -> String {
    (101 + index).to_string()
}

/// The delivery script of `script`: the profile after the drawing property, no dimensions, rooms numbered by their node.
fn rewrite_script(script: &str, node_index: &BTreeMap<String, usize>) -> Result<String, Refusal> {
    let mut out = String::with_capacity(script.len() + 64);
    let mut profile_done = false;
    for line in script.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let (id, params) = line.split_once(' ').unwrap_or((line, "{}"));
        match id {
            "dimension" => continue,
            "cad_profile" => return Err(refuse("generation_failed", "the plan's script already names a CAD profile")),
            "room" => {
                let mut p: Value = serde_json::from_str(params)
                    .map_err(|e| refuse("generation_failed", format!("a room line of the script is not JSON: {e}")))?;
                let old = p["number"].as_str().unwrap_or_default().to_string();
                let index = node_index.get(&old).copied().or_else(|| {
                    // The corridor generators number rooms 101 upward already, in node order.
                    old.parse::<usize>().ok().and_then(|n| n.checked_sub(101)).filter(|i| *i < node_index.len())
                });
                let index = index.ok_or_else(|| refuse("generation_failed", format!("room {old:?} is no node of the graph")))?;
                p["number"] = json!(number_of(index));
                out.push_str(&format!("room {p}\n"));
                continue;
            }
            _ => {}
        }
        out.push_str(line);
        out.push('\n');
        if id == "drawing_property" && !profile_done {
            out.push_str("cad_profile {\"name\":\"delivery\"}\n");
            profile_done = true;
        }
    }
    if !profile_done {
        return Err(refuse("generation_failed", "the plan's script has no `drawing_property` line to put the delivery profile after"));
    }
    Ok(out)
}

/// A corridor generator's graph names its lobby as the entry, while its drawing has the main door open into the corridor and a
/// service exit. The delivery graph names the room the saved model's entry is: what a receiver reading the drawing finds.
fn set_entry_as_drawn(topology: &mut Value, script: &str, ids: &[String]) -> Result<(), Refusal> {
    let mut scratch = Session::new();
    scratch.script(script).map_err(|e| refuse("generation_failed", format!("the delivery script does not run: {e}")))?;
    let extracted = crate::topology::extract(&scratch.model);
    let Some(number) = extracted["entry"]["room"].as_str() else { return Ok(()) };
    let Some(id) = number.parse::<usize>().ok().and_then(|n| n.checked_sub(101)).and_then(|i| ids.get(i)) else { return Ok(()) };
    match &mut topology["entry"] {
        Value::String(s) => *s = id.clone(),
        Value::Object(o) => {
            o.insert("room".into(), json!(id));
        }
        _ => {}
    }
    Ok(())
}

/// `built` to the delivery drawing rules: the same layout, a delivery script, graph and manifest.
pub(crate) fn apply(mut built: Built) -> Result<Built, Refusal> {
    let mut topology = json_of(&built, "topology-graph.json")?;
    let ids: Vec<String> = topology["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().map(|n| n["id"].as_str().unwrap_or_default().to_string()).collect())
        .unwrap_or_default();
    if ids.len() > MAX_NUMBERED {
        return Err(refuse(
            "generation_failed",
            format!("a delivery plan numbers its rooms 101 to 999, and this one has {}", ids.len()),
        ));
    }
    let node_index: BTreeMap<String, usize> = ids.iter().enumerate().map(|(i, id)| (id.clone(), i)).collect();
    let script = rewrite_script(text_of(&built, "script.bimcraft.txt")?, &node_index)?;
    if let Some(nodes) = topology["nodes"].as_array_mut() {
        for (i, n) in nodes.iter_mut().enumerate() {
            n["number"] = json!(number_of(i));
        }
    }
    set_entry_as_drawn(&mut topology, &script, &ids)?;
    let topology_text = pretty(&topology)?;
    let topology_hash = fnv1a64(&topology_text);

    let mut building = json_of(&built, "building-ir.json")?;
    building["source_2d"]["topology_hash"] = json!(topology_hash);
    let mut manifest = json_of(&built, MANIFEST)?;
    let lines: Vec<(String, Value)> = script
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (id, params) = l.split_once(' ').unwrap_or((l, "{}"));
            (id.to_string(), serde_json::from_str(params).unwrap_or(Value::Null))
        })
        .collect();
    manifest["delivery"] = json!(true);
    manifest["building"] = json!(shape_graph::building_id(built.archetype.name(), &lines));
    manifest["topology_hash"] = json!(topology_hash);

    let mut new_texts: BTreeMap<&'static str, String> = BTreeMap::new();
    new_texts.insert("topology-graph.json", topology_text);
    new_texts.insert("building-ir.json", pretty(&building)?);
    new_texts.insert("script.bimcraft.txt", script);
    for (name, text) in &mut built.files {
        if let Some(t) = new_texts.remove(name) {
            *text = t;
        }
    }
    // The manifest names every file by its hash.
    if let Some(files) = manifest["files"].as_array_mut() {
        for f in files {
            let path = f["path"].as_str().unwrap_or_default().to_string();
            if let Some((_, text)) = built.files.iter().find(|(n, _)| *n == path) {
                f["hash"] = json!(fnv1a64(text));
            }
        }
    }
    let manifest_text = pretty(&manifest)?;
    for (name, text) in &mut built.files {
        if *name == MANIFEST {
            *text = manifest_text.clone();
        }
    }
    built.topology_hash = topology_hash;
    Ok(built)
}
