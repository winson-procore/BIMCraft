//! The graphs a plan is made from and the building as data: the program (what the building must hold), the topology (what
//! can be walked from where) and the placed building, each as JSON a person or another program can read.
//!
//! Only `door` and `open` edges are access. `adjacent` and `near` are softer relations that no rule reads.

use serde_json::{Value, json};

use super::layout::{Band, Plan, mm2_to_m2};
use super::program::Role;

pub const SCHEMA_VERSION: u32 = 1;
/// What the topology file says it is, and what the building file says it is.
pub const TOPOLOGY_KIND: &str = "2d_topology";
pub const BUILDING_KIND: &str = "3d_bim";

/// The kinds of edge that mean one can walk from one space to the other.
pub const ACCESS: [&str; 2] = ["door", "open"];

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub role: Role,
    pub area_m2: f64,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
}

impl Edge {
    /// What a 3D object that came of this edge names it by: stable, and made of what the edge is.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.kind, self.from, self.to)
    }
}

#[derive(Clone, Debug)]
pub struct Topology {
    /// The space the building is entered from outside through.
    pub entry: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Topology {
    pub fn of(plan: &Plan) -> Topology {
        let mut nodes = vec![Node {
            id: plan.program.corridor.id.clone(),
            kind: plan.program.corridor.kind.to_string(),
            role: plan.program.corridor.role,
            area_m2: mm2_to_m2(plan.corridor_area_mm2()),
        }];
        nodes.extend(plan.cells.iter().map(|c| Node {
            id: c.spec.id.clone(),
            kind: c.spec.kind.to_string(),
            role: c.spec.role,
            area_m2: mm2_to_m2(c.area_mm2()),
        }));
        let corridor = plan.program.corridor.id.clone();
        let mut edges: Vec<Edge> =
            plan.cells.iter().map(|c| Edge { from: c.spec.id.clone(), to: corridor.clone(), kind: "door" }).collect();
        for band in [Band::South, Band::North] {
            let cells: Vec<_> = plan.band(band).collect();
            edges.extend(cells.windows(2).filter_map(|pair| match pair {
                [a, b] => Some(Edge { from: a.spec.id.clone(), to: b.spec.id.clone(), kind: "adjacent" }),
                _ => None,
            }));
        }
        Topology { entry: plan.program.lobby.id.clone(), nodes, edges }
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn access(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(|e| ACCESS.contains(&e.kind))
    }

    pub fn json(&self, plan: &Plan) -> Value {
        json!({
            "schema_version": SCHEMA_VERSION,
            "graph_kind": TOPOLOGY_KIND,
            "archetype": plan.archetype.name(),
            "seed": plan.seed,
            "entry": self.entry,
            "nodes": self.nodes.iter().map(|n| json!({
                "id": n.id, "kind": n.kind, "role": n.role.name(), "area_m2": n.area_m2, "level": "Level 1",
            })).collect::<Vec<_>>(),
            "edges": self.edges.iter().map(|e| json!({"id": e.id(), "from": e.from, "to": e.to, "kind": e.kind})).collect::<Vec<_>>(),
        })
    }
}

/// What the building must hold, before any of it is placed.
pub fn program_json(plan: &Plan) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "archetype": plan.archetype.name(),
        "seed": plan.seed,
        "requested_area_m2": plan.requested_m2,
        "spaces": plan.program.all().map(|s| json!({
            "id": s.id, "kind": s.kind, "label": s.label, "role": s.role.name(),
            "min_area_m2": s.min_m2, "target_area_m2": s.target_m2, "optional": false,
        })).collect::<Vec<_>>(),
        "edges": plan.cells.iter().map(|c| json!({"from": c.spec.id, "to": plan.program.corridor.id, "kind": "access_required"})).collect::<Vec<_>>(),
    })
}
