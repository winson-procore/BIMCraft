//! The trace contract: a traced plan's expected file, read strictly, and compared with an extracted graph.
//!
//! The fields are named from the model's side: `missing_*` is in the extracted graph and not in the expected file, `extra_*`
//! is in the expected file and not in the graph. Kinds, `building`, `review`, `source` and `entry.via` are not compared.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::{MAX_TEXT, Operation};

/// The largest expected file, bytes.
pub const MAX_FILE: u64 = 16 * 1024 * 1024;
/// The most rooms, and the most links, in an expected file.
pub const MAX_ITEMS: usize = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Room {
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
    kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    from: String,
    to: String,
    via: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    room: String,
    #[serde(default)]
    #[allow(dead_code)]
    via: Option<String>,
}

/// A traced plan's expected topology.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    rooms: Vec<Room>,
    links: Vec<Link>,
    entry: Entry,
    #[serde(default)]
    #[allow(dead_code)]
    building: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    review: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<String>,
}

fn text_ok(what: &str, v: &str) -> Result<(), String> {
    if v.chars().count() > MAX_TEXT {
        return Err(format!("`{what}` is at most {MAX_TEXT} characters"));
    }
    Ok(())
}

/// Reads and checks an expected file; each refusal names the field to fix.
pub fn read_expected(path: &str) -> Result<Expected, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.len();
    if len > MAX_FILE {
        return Err(format!("{path}: an expected file is at most {MAX_FILE} bytes"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let x: Expected = serde_json::from_str(&text).map_err(|e| {
        let why: String = e.to_string().chars().take(200).collect();
        format!("{path}: not an expected topology file: {why}")
    })?;
    if x.rooms.len() > MAX_ITEMS || x.links.len() > MAX_ITEMS {
        return Err(format!("`rooms` and `links` hold at most {MAX_ITEMS} each"));
    }
    let mut ids = BTreeSet::new();
    for (i, r) in x.rooms.iter().enumerate() {
        text_ok(&format!("rooms[{i}].id"), &r.id)?;
        if r.id.trim().is_empty() {
            return Err(format!("`rooms[{i}].id` is empty: give the room's number"));
        }
        if !ids.insert(r.id.as_str()) {
            return Err(format!("`rooms[{i}].id` is `{}`, which an earlier room has: ids are unique", r.id));
        }
        if r.kind.trim().is_empty() {
            return Err(format!("`rooms[{i}].kind` is empty: give a kind, any text"));
        }
        text_ok(&format!("rooms[{i}].kind"), &r.kind)?;
        if let Some(n) = &r.name {
            text_ok(&format!("rooms[{i}].name"), n)?;
        }
    }
    let vias: Vec<&str> = Operation::ALL.iter().map(|o| o.name()).collect();
    for (i, l) in x.links.iter().enumerate() {
        for (k, v) in [("from", &l.from), ("to", &l.to)] {
            if !ids.contains(v.as_str()) {
                return Err(format!("`links[{i}].{k}` is `{}`, which is no room of `rooms`", v.chars().take(MAX_TEXT).collect::<String>()));
            }
        }
        if l.from == l.to {
            return Err(format!("`links[{i}]` joins room `{}` to itself", l.from));
        }
        if l.via != "open" && !vias.contains(&l.via.as_str()) {
            return Err(format!(
                "`links[{i}].via` is `{}`: use open, {}",
                l.via.chars().take(MAX_TEXT).collect::<String>(),
                vias.join(", ")
            ));
        }
    }
    if !ids.contains(x.entry.room.as_str()) {
        return Err(format!("`entry.room` is `{}`, which is no room of `rooms`", x.entry.room.chars().take(MAX_TEXT).collect::<String>()));
    }
    Ok(x)
}

fn pair(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

fn links_json(set: &BTreeSet<(String, String, String)>) -> Value {
    Value::Array(set.iter().map(|(a, b, via)| json!({"from": a, "to": b, "via": via})).collect())
}

/// The extracted graph against an expected file: the six fields and `agree`.
pub fn compare(graph: &Value, x: &Expected) -> Value {
    let nodes: BTreeSet<String> =
        graph["nodes"].as_array().map_or_else(BTreeSet::new, |n| n.iter().filter_map(|n| n["id"].as_str().map(String::from)).collect());
    let rooms: BTreeSet<String> = x.rooms.iter().map(|r| r.id.clone()).collect();
    let mut got = BTreeSet::new();
    for e in graph["edges"].as_array().map_or(&[][..], Vec::as_slice) {
        let kind = e["kind"].as_str().unwrap_or("");
        if !(kind == "door" || kind == "open") || e["exterior"].as_bool() == Some(true) {
            continue;
        }
        let via = if kind == "open" { "open" } else { e["operation"].as_str().unwrap_or("swing") };
        let (a, b) = pair(e["from"].as_str().unwrap_or(""), e["to"].as_str().unwrap_or(""));
        got.insert((a, b, via.to_string()));
    }
    let want: BTreeSet<(String, String, String)> = x
        .links
        .iter()
        .map(|l| {
            let (a, b) = pair(&l.from, &l.to);
            (a, b, l.via.clone())
        })
        .collect();
    let extracted = graph["entry"]["room"].as_str();
    let entry =
        if extracted == Some(x.entry.room.as_str()) { Value::Null } else { json!({"expected": x.entry.room, "extracted": extracted}) };
    let diagnostics = graph["diagnostics"].clone();
    let diagnostics = if diagnostics.is_array() { diagnostics } else { json!([]) };
    let missing_rooms: Vec<&String> = nodes.difference(&rooms).collect();
    let extra_rooms: Vec<&String> = rooms.difference(&nodes).collect();
    let missing_links: BTreeSet<_> = got.difference(&want).cloned().collect();
    let extra_links: BTreeSet<_> = want.difference(&got).cloned().collect();
    let agree = missing_rooms.is_empty()
        && extra_rooms.is_empty()
        && missing_links.is_empty()
        && extra_links.is_empty()
        && entry.is_null()
        && diagnostics.as_array().is_some_and(Vec::is_empty);
    json!({
        "agree": agree,
        "missing_rooms": missing_rooms, "extra_rooms": extra_rooms,
        "missing_links": links_json(&missing_links), "extra_links": links_json(&extra_links),
        "entry": entry, "diagnostics": diagnostics,
    })
}
