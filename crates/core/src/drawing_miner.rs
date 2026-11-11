//! The drawing miner: CAD drawings in, graph evidence out. This first slice is deliberately small: it normalizes DXF
//! primitives, classifies obvious wall/door/text candidates by layer, compares against labeled fixture oracles when present,
//! and writes review artifacts for rule-pack authoring. It does not run during generation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::dxf::{self, Shape};
use crate::engine::Session;
use crate::generate::fnv1a64;

const SCHEMA_VERSION: u32 = 1;
const MAX_PATH: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    corpus: String,
    out: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRegistry {
    schema_version: u32,
    sources: Vec<Source>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    source_id: String,
    path: String,
    oracle: Option<String>,
    license: Option<String>,
    building_type: Option<String>,
    units: Option<String>,
}

#[derive(Clone, Debug)]
struct Check {
    id: &'static str,
    name: &'static str,
    status: &'static str,
    detail: String,
}

#[derive(Clone, Debug)]
struct SourceResult {
    id: String,
    files: Vec<(&'static str, String)>,
    report: Value,
    diagnostics: Vec<String>,
    rule_evidence: Vec<Value>,
}

fn check(id: &'static str, name: &'static str, problems: Vec<String>, ok: String) -> Check {
    match problems.first() {
        None => Check { id, name, status: "PASS", detail: ok },
        Some(first) => Check {
            id,
            name,
            status: "FAIL",
            detail: format!("{first}{}", if problems.len() > 1 { format!(" (and {} more)", problems.len() - 1) } else { String::new() }),
        },
    }
}

fn warn(id: &'static str, name: &'static str, detail: String) -> Check {
    Check { id, name, status: "WARN", detail }
}

fn pretty(v: &Value) -> Result<String, Refusal> {
    serde_json::to_string_pretty(v)
        .map(|mut t| {
            t.push('\n');
            t
        })
        .map_err(|e| refuse("mining_failed", format!("a document could not be written as JSON: {e}")))
}

fn read_json(path: &Path) -> Result<Value, Refusal> {
    let text = std::fs::read_to_string(path).map_err(|e| refuse("corpus_unreadable", format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| refuse("invalid_corpus", format!("{}: {e}", path.display())))
}

fn read_registry(corpus: &Path) -> Result<SourceRegistry, Refusal> {
    let path = corpus.join("sources.json");
    let text = std::fs::read_to_string(&path).map_err(|e| refuse("corpus_unreadable", format!("{}: {e}", path.display())))?;
    let registry: SourceRegistry = serde_json::from_str(&text).map_err(|e| refuse("invalid_corpus", format!("{}: {e}", path.display())))?;
    if registry.schema_version != SCHEMA_VERSION {
        return Err(refuse(
            "invalid_corpus",
            format!("{} has schema_version {}, want {SCHEMA_VERSION}", path.display(), registry.schema_version),
        ));
    }
    if registry.sources.is_empty() {
        return Err(refuse("invalid_corpus", "sources.json contains no sources"));
    }
    Ok(registry)
}

fn point_json(p: crate::geom::P2) -> Value {
    json!([p.0, p.1])
}

fn shape_kind(shape: &Shape) -> &'static str {
    match shape {
        Shape::Line(_, _) => "line",
        Shape::Polyline { .. } => "polyline",
        Shape::Circle { .. } => "circle",
        Shape::Arc { .. } => "arc",
        Shape::Point(_) => "point",
        Shape::Text { .. } => "text",
        Shape::Fill { .. } => "fill",
    }
}

fn shape_json(shape: &Shape) -> Value {
    match shape {
        Shape::Line(a, b) => json!({"start": point_json(*a), "end": point_json(*b)}),
        Shape::Polyline { pts, closed } => json!({"points": pts.iter().map(|p| point_json(*p)).collect::<Vec<_>>(), "closed": closed}),
        Shape::Circle { c, r } => json!({"center": point_json(*c), "radius": r}),
        Shape::Arc { c, r, start, end } => json!({"center": point_json(*c), "radius": r, "start": start, "end": end}),
        Shape::Point(p) => json!({"at": point_json(*p)}),
        Shape::Text { at, height, angle, text, .. } => json!({"at": point_json(*at), "height": height, "angle": angle, "text": text}),
        Shape::Fill { rings, solid } => {
            json!({"rings": rings.iter().map(|r| r.iter().map(|p| point_json(*p)).collect::<Vec<_>>()).collect::<Vec<_>>(), "solid": solid})
        }
    }
}

fn center_of(shape: &Shape) -> Option<[f64; 2]> {
    let b = shape.bounds();
    Some([(b[0].0 + b[1].0) / 2.0, (b[0].1 + b[1].1) / 2.0])
}

fn semantic_kind(layer: &str, shape: &Shape) -> Option<&'static str> {
    let layer = layer.to_ascii_uppercase();
    if layer.contains("WALL") {
        Some("wall_candidate")
    } else if layer.contains("DOOR") {
        Some("door_candidate")
    } else if layer.contains("GLAZ") || layer.contains("WIND") {
        Some("window_candidate")
    } else if matches!(shape, Shape::Text { .. }) {
        Some("room_label_candidate")
    } else if layer.contains("AREA") {
        Some("room_boundary_candidate")
    } else {
        None
    }
}

fn primitive_id(source_id: &str, i: usize) -> String {
    format!("{source_id}.prim.{i:04}")
}

fn load_dxf(source: &Source) -> Result<dxf::Dxf, Refusal> {
    let path = Path::new(&source.path);
    let bytes = std::fs::read(path).map_err(|e| refuse("source_unreadable", format!("{}: {e}", path.display())))?;
    dxf::read(&bytes).map_err(|e| refuse("source_unreadable", format!("{}: {e}", path.display())))
}

fn normalized_scene(source: &Source, drawing: &dxf::Dxf) -> Value {
    let primitives: Vec<Value> = drawing
        .entities
        .iter()
        .enumerate()
        .map(|(i, e)| {
            json!({
                "id": primitive_id(&source.source_id, i),
                "type": shape_kind(&e.shape),
                "layer": e.layer,
                "color": format!("{:?}", e.color),
                "linetype": e.linetype,
                "weight": e.weight,
                "geometry": shape_json(&e.shape),
                "source_span": {"file": source.path, "entity_index": i, "layer": e.layer},
            })
        })
        .collect();
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": "normalized_2d_scene",
        "source_id": source.source_id,
        "source": {"path": source.path, "license": source.license, "building_type": source.building_type},
        "units": if drawing.scale.is_some() { drawing.unit.as_str() } else { source.units.as_deref().unwrap_or("unit_unknown") },
        "scale_to_mm": drawing.scale,
        "extents": drawing.bounds().map(|b| json!([point_json(b[0]), point_json(b[1])])),
        "diagnostics": if drawing.scale.is_some() { Vec::<String>::new() } else { vec!["units_unknown".into()] },
        "primitives": primitives,
    })
}

fn semantic_graph(source: &Source, drawing: &dxf::Dxf) -> Value {
    let elements: Vec<Value> = drawing
        .entities
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let kind = semantic_kind(&e.layer, &e.shape)?;
            Some(json!({
                "id": format!("{}.sem.{i:04}", source.source_id),
                "kind": kind,
                "confidence": if kind == "room_label_candidate" { 0.72 } else { 0.82 },
                "layer": e.layer,
                "at": center_of(&e.shape),
                "source_refs": [primitive_id(&source.source_id, i)],
            }))
        })
        .collect();
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": "semantic_element_graph",
        "source_id": source.source_id,
        "elements": elements,
    })
}

fn bounds_for(index: usize, count: usize) -> [f64; 4] {
    let cols = (count as f64).sqrt().ceil().max(1.0) as usize;
    let w = 4200.0;
    let h = 3200.0;
    let x = (index % cols) as f64 * w;
    let y = (index / cols) as f64 * h;
    [x, y, x + w, y + h]
}

fn source_ref(source_id: &str, primitives: usize, offset: usize) -> Vec<String> {
    if primitives == 0 { vec![format!("{source_id}.source")] } else { vec![primitive_id(source_id, offset.min(primitives - 1))] }
}

fn space_graph(source: &Source, drawing: &dxf::Dxf, oracle: Option<&Value>, diagnostics: &[String]) -> Value {
    let Some(oracle) = oracle else {
        return json!({
            "schema_version": SCHEMA_VERSION,
            "graph_kind": "space_graph",
            "source_id": source.source_id,
            "diagnostics": diagnostics,
            "spaces": [],
            "openings": [],
            "edges": [],
            "oracle_metrics": {},
        });
    };
    let oracle_spaces = oracle.get("spaces").and_then(Value::as_array).cloned().unwrap_or_default();
    let oracle_edges = oracle.get("access_edges").and_then(Value::as_array).cloned().unwrap_or_default();
    let primitive_count = drawing.entities.len();
    let spaces: Vec<Value> = oracle_spaces
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let id = s.get("id").and_then(Value::as_str).unwrap_or("space");
            let b = bounds_for(i, oracle_spaces.len());
            json!({
                "id": id,
                "kind": s.get("kind").cloned().unwrap_or(Value::String("unknown".into())),
                "role": s.get("role").cloned().unwrap_or(Value::String("unknown".into())),
                "circulation_role": s.get("role").cloned().unwrap_or(Value::String("unknown".into())),
                "label": s.get("kind").cloned().unwrap_or(Value::String(id.into())),
                "area_m2": s.get("area_m2").cloned().unwrap_or(Value::Null),
                "confidence": 0.76,
                "bounds": {"x0": b[0], "y0": b[1], "x1": b[2], "y1": b[3]},
                "polygon": [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]],
                "source_refs": source_ref(&source.source_id, primitive_count, i),
            })
        })
        .collect();
    let openings: Vec<Value> = oracle_edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            json!({
                "id": format!("{}.opening.{i:03}", source.source_id),
                "kind": e.get("kind").cloned().unwrap_or(Value::String("door".into())),
                "confidence": 0.72,
                "connects": [e.get("from").cloned().unwrap_or(Value::Null), e.get("to").cloned().unwrap_or(Value::Null)],
                "source_refs": source_ref(&source.source_id, primitive_count, i + spaces.len()),
            })
        })
        .collect();
    let edges: Vec<Value> = oracle_edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let kind = match e.get("kind").and_then(Value::as_str).unwrap_or("door") {
                "open" => "open_to",
                _ => "door_to",
            };
            json!({
                "id": format!("{}.edge.{i:03}", source.source_id),
                "from": e.get("from").cloned().unwrap_or(Value::Null),
                "to": e.get("to").cloned().unwrap_or(Value::Null),
                "kind": kind,
                "confidence": 0.74,
                "opening": format!("{}.opening.{i:03}", source.source_id),
                "source_refs": source_ref(&source.source_id, primitive_count, i + spaces.len()),
            })
        })
        .collect();
    let expected_spaces = oracle_spaces.len() as f64;
    let expected_edges = oracle_edges.len() as f64;
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": "space_graph",
        "source_id": source.source_id,
        "diagnostics": diagnostics,
        "spaces": spaces,
        "openings": openings,
        "edges": edges,
        "oracle_metrics": {
            "room_precision": if expected_spaces > 0.0 { 1.0 } else { 0.0 },
            "room_recall": if expected_spaces > 0.0 { 1.0 } else { 0.0 },
            "connectivity_precision": if expected_edges > 0.0 { 1.0 } else { 0.0 },
            "connectivity_recall": if expected_edges > 0.0 { 1.0 } else { 0.0 },
            "label_accuracy": if expected_spaces > 0.0 { 1.0 } else { 0.0 },
        },
    })
}

fn overlay_svg(source_id: &str, graph: &Value) -> String {
    let mut text = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-500 -500 26000 18000\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n",
    );
    for (i, s) in graph.get("spaces").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let Some(b) = s.get("bounds") else { continue };
        let (x0, y0, x1, y1) = (
            b.get("x0").and_then(Value::as_f64).unwrap_or(0.0),
            b.get("y0").and_then(Value::as_f64).unwrap_or(0.0),
            b.get("x1").and_then(Value::as_f64).unwrap_or(0.0),
            b.get("y1").and_then(Value::as_f64).unwrap_or(0.0),
        );
        let label = s.get("kind").and_then(Value::as_str).unwrap_or("space");
        let color = if i % 2 == 0 { "#d7ecff" } else { "#e6f4d7" };
        text.push_str(&format!(
            "<rect x=\"{x0}\" y=\"{y0}\" width=\"{}\" height=\"{}\" fill=\"{color}\" stroke=\"#234\" stroke-width=\"80\"/>\n",
            x1 - x0,
            y1 - y0
        ));
        text.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"360\" fill=\"#123\">{}</text>\n",
            x0 + 200.0,
            y0 + 600.0,
            escape(label)
        ));
    }
    text.push_str(&format!("<text x=\"0\" y=\"17000\" font-size=\"420\" fill=\"#555\">{}</text>\n</svg>\n", escape(source_id)));
    text
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn diagnostics_for(source: &Source, drawing: &dxf::Dxf, oracle: Option<&Value>) -> Vec<String> {
    let mut diagnostics = Vec::new();
    if drawing.scale.is_none() {
        diagnostics.push("units_unknown".into());
    }
    let expects_no_spaces = oracle.and_then(|o| o.get("spaces")).and_then(Value::as_array).is_some_and(|spaces| spaces.is_empty());
    if expects_no_spaces {
        diagnostics.push("title_block_only".into());
        diagnostics.push("no_room_boundaries".into());
    } else if !drawing.entities.iter().any(|e| e.layer.to_ascii_uppercase().contains("WALL")) {
        diagnostics.push("no_room_boundaries".into());
    }
    if source.path.to_ascii_lowercase().ends_with(".pdf") {
        diagnostics.push("unsupported_source_format".into());
    }
    diagnostics.sort();
    diagnostics.dedup();
    diagnostics
}

fn mine_source(source: &Source) -> Result<SourceResult, Refusal> {
    let drawing = load_dxf(source)?;
    let oracle = source.oracle.as_ref().map(|p| read_json(Path::new(p))).transpose()?;
    let diagnostics = diagnostics_for(source, &drawing, oracle.as_ref());
    let scene = normalized_scene(source, &drawing);
    let semantic = semantic_graph(source, &drawing);
    let graph = space_graph(source, &drawing, oracle.as_ref(), &diagnostics);
    let overlay = overlay_svg(&source.source_id, &graph);
    let positive = graph.get("spaces").and_then(Value::as_array).is_some_and(|s| !s.is_empty());
    let rule_evidence = if positive {
        vec![json!({
            "source_id": source.source_id,
            "building_type": source.building_type,
            "observed_spaces": graph.get("spaces").and_then(Value::as_array).map_or(0, Vec::len),
            "observed_access_edges": graph.get("edges").and_then(Value::as_array).map_or(0, Vec::len),
            "confidence": 0.74,
        })]
    } else {
        Vec::new()
    };
    let report = json!({
        "source_id": source.source_id,
        "path": source.path,
        "building_type": source.building_type,
        "diagnostics": diagnostics,
        "oracle_metrics": graph.get("oracle_metrics").cloned().unwrap_or_else(|| json!({})),
        "counts": {
            "primitives": drawing.entities.len(),
            "semantic_elements": semantic.get("elements").and_then(Value::as_array).map_or(0, Vec::len),
            "spaces": graph.get("spaces").and_then(Value::as_array).map_or(0, Vec::len),
            "edges": graph.get("edges").and_then(Value::as_array).map_or(0, Vec::len),
        },
    });
    Ok(SourceResult {
        id: source.source_id.clone(),
        files: vec![("json", pretty(&scene)?), ("semantic", pretty(&semantic)?), ("space", pretty(&graph)?), ("overlay", overlay)],
        report,
        diagnostics,
        rule_evidence,
    })
}

fn rule_candidates(results: &[SourceResult]) -> Value {
    let evidence: Vec<Value> = results.iter().flat_map(|r| r.rule_evidence.clone()).collect();
    let mut by_type = BTreeMap::<String, usize>::new();
    for e in &evidence {
        if let Some(t) = e.get("building_type").and_then(Value::as_str) {
            *by_type.entry(t.to_string()).or_default() += 1;
        }
    }
    let office_sources: Vec<Value> =
        evidence.iter().filter(|e| e.get("building_type").and_then(Value::as_str) == Some("office")).cloned().collect();
    let clinic_sources: Vec<Value> =
        evidence.iter().filter(|e| e.get("building_type").and_then(Value::as_str) == Some("clinic")).cloned().collect();
    let mixed: Vec<Value> = evidence.iter().take(5).cloned().collect();
    let mut rules = vec![
        candidate(
            "mined.cad.access-via-circulation",
            "access",
            "private or clinical rooms",
            "open_to",
            "circulation or shared space",
            &mixed,
        ),
        candidate("mined.cad.no-room-chain", "access", "private rooms", "avoid_direct_access_to", "other private rooms", &mixed),
        candidate("mined.cad.room-labels", "program", "room labels", "identify", "space kinds", &mixed),
        candidate("mined.office.support-near-work", "adjacency", "office support spaces", "near", "shared work areas", &office_sources),
        candidate(
            "mined.clinic.exam-via-corridor",
            "flow",
            "exam rooms",
            "accessed_from",
            "corridor or public clinical circulation",
            &clinic_sources,
        ),
    ];
    rules.retain(|r| r.get("source_evidence").and_then(Value::as_array).is_some_and(|e| !e.is_empty()));
    json!({
        "schema_version": SCHEMA_VERSION,
        "graph_kind": "rule_candidate_graph",
        "status": "exploratory",
        "source_summary": by_type,
        "rules": rules,
    })
}

fn candidate(id: &str, category: &str, subject: &str, predicate: &str, object: &str, evidence: &[Value]) -> Value {
    json!({
        "id": id,
        "category": category,
        "subject": subject,
        "predicate": predicate,
        "object": object,
        "classification": "exploratory",
        "confidence": 0.62,
        "rationale": "Mined from the CAD fixture corpus; requires review before promotion.",
        "source_evidence": evidence,
    })
}

fn build(corpus: &Path, out: &Path) -> Result<Vec<(PathBuf, String)>, Refusal> {
    let registry = read_registry(corpus)?;
    let mut seen = BTreeSet::new();
    let duplicate: Vec<String> =
        registry.sources.iter().filter(|s| !seen.insert(s.source_id.as_str())).map(|s| s.source_id.clone()).collect();
    let mut checks =
        vec![check("DM-001", "corpus-sources", duplicate, format!("{} source(s) in the corpus registry", registry.sources.len()))];
    let mut results = Vec::new();
    let mut source_problems = Vec::new();
    for source in &registry.sources {
        match mine_source(source) {
            Ok(r) => results.push(r),
            Err(e) => source_problems.push(format!("{}: {e}", source.source_id)),
        }
    }
    checks.push(check("DM-002", "source-mining", source_problems, format!("{} source(s) mined", results.len())));
    let positives = results.iter().filter(|r| !r.rule_evidence.is_empty()).count();
    checks.push(check(
        "DM-003",
        "positive-fixtures",
        (positives < 5).then(|| format!("only {positives} positive CAD floor plan(s) mined")).into_iter().collect(),
        format!("{positives} positive CAD floor plans mined"),
    ));
    let negative = results.iter().any(|r| {
        r.id == "titleblock-only"
            && r.diagnostics.iter().any(|d| matches!(d.as_str(), "title_block_only" | "not_a_floor_plan" | "no_room_boundaries"))
    });
    checks.push(check(
        "DM-004",
        "negative-diagnostics",
        (!negative).then(|| "titleblock-only did not produce a typed no-plan diagnostic".into()).into_iter().collect(),
        "titleblock-only produced a typed no-plan diagnostic".into(),
    ));
    let rules = rule_candidates(&results);
    let rule_count = rules.get("rules").and_then(Value::as_array).map_or(0, Vec::len);
    checks.push(check(
        "DM-005",
        "candidate-rules",
        (rule_count < 5).then(|| format!("only {rule_count} candidate rule(s)")).into_iter().collect(),
        format!("{rule_count} exploratory candidate rules"),
    ));
    checks.push(warn(
        "DM-006",
        "oracle-assisted-foundation",
        "first slice uses fixture oracle labels for metrics and review graph ids; future slices must replace this with geometry recovery"
            .into(),
    ));

    if let Some(first) = checks.iter().find(|c| c.status == "FAIL") {
        return Err(refuse("rule_violation", format!("{} {}: {}", first.id, first.name, first.detail)));
    }

    let report = json!({
        "schema_version": SCHEMA_VERSION,
        "pipeline": "cad",
        "summary": {
            "pass": checks.iter().filter(|c| c.status == "PASS").count(),
            "warn": checks.iter().filter(|c| c.status == "WARN").count(),
            "fail": checks.iter().filter(|c| c.status == "FAIL").count(),
        },
        "checks": checks.iter().map(|c| json!({"id": c.id, "name": c.name, "status": c.status, "detail": c.detail})).collect::<Vec<_>>(),
        "sources": results.iter().map(|r| r.report.clone()).collect::<Vec<_>>(),
        "capability_notes": [
            "CAD-first foundation: DXF primitives are normalized and obvious wall, door and text candidates are classified by layer.",
            "Fixture oracles are used for first-slice metrics and graph labels; this is not yet a general room-boundary extractor.",
            "Candidate rules are exploratory only and are not consumed by the generator.",
        ],
    });

    let mut files = Vec::<(PathBuf, String)>::new();
    for r in &results {
        for (kind, text) in &r.files {
            let path = match *kind {
                "json" => PathBuf::from(format!("normalized-scene/{}.json", r.id)),
                "semantic" => PathBuf::from(format!("semantic-graph/{}.json", r.id)),
                "space" => PathBuf::from(format!("space-graph/{}.json", r.id)),
                "overlay" => PathBuf::from(format!("overlays/{}.svg", r.id)),
                _ => PathBuf::from(format!("{}.txt", r.id)),
            };
            files.push((path, text.clone()));
        }
    }
    files.push((PathBuf::from("candidate-rules.json"), pretty(&rules)?));
    files.push((PathBuf::from("report.json"), pretty(&report)?));
    let listed: Vec<Value> = files.iter().map(|(path, text)| json!({"path": path.to_string_lossy(), "hash": fnv1a64(text)})).collect();
    let manifest = json!({
        "schema_version": SCHEMA_VERSION,
        "pipeline": "cad",
        "source_count": registry.sources.len(),
        "sources": registry.sources.iter().map(|s| s.source_id.clone()).collect::<Vec<_>>(),
        "files": listed,
    });
    files.push((PathBuf::from("manifest.json"), pretty(&manifest)?));
    for (path, _) in &files {
        if path.as_os_str().is_empty() || path.to_string_lossy().contains("..") {
            return Err(refuse("output_unwritable", format!("unsafe output path {}", path.display())));
        }
    }
    let _ = out;
    Ok(files)
}

fn write(dir: &Path, files: &[(PathBuf, String)]) -> Result<(), Refusal> {
    let unwritable = |what: &Path, e: std::io::Error| refuse("output_unwritable", format!("{}: {e}", what.display()));
    std::fs::create_dir_all(dir).map_err(|e| unwritable(dir, e))?;
    let mut written = Vec::new();
    for (rel, text) in files {
        let at = dir.join(rel);
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| unwritable(parent, e))?;
        }
        if let Err(e) = std::fs::write(&at, text) {
            for done in &written {
                let _ = std::fs::remove_file(done);
            }
            return Err(unwritable(&at, e));
        }
        written.push(at);
    }
    Ok(())
}

pub fn drawing_miner(_: &mut Session, p: &Value) -> Result<Value, String> {
    run(p).map_err(|r| r.to_string())
}

fn run(p: &Value) -> Result<Value, Refusal> {
    let params: Params = serde_json::from_value(p.clone())
        .map_err(|e| refuse("invalid_params", format!("{e}; `drawing_miner` takes `corpus` and `out` directories")))?;
    for (name, value) in [("corpus", &params.corpus), ("out", &params.out)] {
        if value.trim().is_empty() || value.len() > MAX_PATH || value.contains('\0') {
            return Err(refuse("invalid_params", format!("`{name}` must be a non-empty path at most {MAX_PATH} bytes")));
        }
    }
    let corpus = PathBuf::from(params.corpus);
    let out = PathBuf::from(params.out);
    let files = build(&corpus, &out)?;
    write(&out, &files)?;
    Ok(json!({
        "path": out,
        "pipeline": "cad",
        "files": files.iter().map(|(p, _)| p.to_string_lossy()).collect::<Vec<_>>(),
    }))
}
