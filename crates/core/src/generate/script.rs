//! The placed building as a BIMCraft script: one command to a line, in the order the commands need (levels and grids, then
//! walls, the floor, the doors and windows in them, the rooms between them, then what is drawn on the plan and the sheet).
//! It holds no file path, so it runs from any directory.

use serde_json::{Value, json};

use super::layout::Plan;
use super::lift::Building;

/// A command of the script, and the space it makes a room for, when it is one.
#[derive(Clone, Debug)]
pub struct Line {
    pub id: &'static str,
    pub params: Value,
    pub room_of: Option<String>,
    /// The id of the building's wall this command makes.
    pub wall_of: Option<String>,
    /// The id of the building's door or window this command makes.
    pub opening_of: Option<String>,
}

pub struct Script {
    pub lines: Vec<Line>,
    pub text: String,
}

/// The commands a script of a generated plan holds at least once.
pub const REQUIRED: [&str; 12] = ["new", "level", "grid", "wall", "door", "floor", "room", "text", "dimension", "tag", "sheet", "place"];

/// The sheet's size in millimetres, landscape.
fn sheet_size(name: &str) -> (i64, i64) {
    match name {
        "A0" => (1189, 841),
        "A1" => (841, 594),
        "A2" => (594, 420),
        _ => (420, 297),
    }
}

/// The smallest sheet and the largest scale the plan and its dimensions fit on, with room left for the title block.
fn sheet_for(plan: &Plan) -> (&'static str, i64) {
    for size in ["A3", "A2", "A1", "A0"] {
        let (w, h) = sheet_size(size);
        for scale in [50, 100, 200, 250, 500] {
            if (plan.width + 4000) / scale * 100 <= w * 70 && (plan.depth + 6000) / scale * 100 <= h * 70 {
                return (size, scale);
            }
        }
    }
    ("A0", 500)
}

fn horizontal_span(points: &[[i64; 2]], y: i64) -> Option<(i64, i64)> {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .filter(|(a, b)| a[1] == y && b[1] == y)
        .map(|(a, b)| (a[0].min(b[0]), a[0].max(b[0])))
        .max_by_key(|(x0, x1)| x1 - x0)
}

fn line(id: &'static str, params: Value) -> Line {
    Line { id, params, room_of: None, wall_of: None, opening_of: None }
}

/// The script of a building: its walls and openings as the lifted graph has them, the rest as the plan has it.
pub fn compile(plan: &Plan, building: &Building) -> Script {
    let (w, d) = (plan.width, plan.depth);
    let title = format!("{} {} m2, seed {}", plan.archetype.name(), plan.gross_m2().round(), plan.seed);
    let mut lines = vec![
        line("new", json!({"name": title})),
        line("room_area", json!({"at": "finish"})),
        line("level", json!({"elevation": 3600, "name": "Roof"})),
    ];
    for (start, end, name) in [
        ([0, -1500], [0, d + 1500], None),
        ([w, -1500], [w, d + 1500], None),
        ([-1500, 0], [w + 1500, 0], Some("A")),
        ([-1500, d], [w + 1500, d], None),
    ] {
        let mut params = json!({"start": start, "end": end});
        if let Some(name) = name {
            params["name"] = json!(name);
        }
        lines.push(line("grid", params));
    }
    for wall in &building.walls {
        let params = json!({"points": [wall.start, wall.end], "closed": false, "type": wall.wall_type});
        lines.push(Line { wall_of: Some(wall.id.clone()), ..line("wall", params) });
    }
    lines.push(line("floor", json!({"boundary": building.boundary})));
    for id in ["door", "window"] {
        lines.extend(
            building
                .openings
                .iter()
                .filter(|o| o.kind == id)
                .map(|o| Line { opening_of: Some(o.id.clone()), ..line(id, json!({"at": o.at})) }),
        );
    }
    let corridor = &plan.program.corridor;
    let mut rooms = vec![(corridor.id.clone(), corridor.label.clone(), [w / 2, (plan.corridor_bottom + plan.corridor_top) / 2])];
    rooms.extend(plan.cells.iter().map(|c| (c.spec.id.clone(), c.spec.label.clone(), c.middle())));
    for (i, (space, name, point)) in rooms.into_iter().enumerate() {
        let number = (101 + i).to_string();
        lines.push(Line { room_of: Some(space), ..line("room", json!({"point": point, "name": name, "number": number})) });
    }
    let bottom_span = horizontal_span(&building.boundary, 0).unwrap_or((0, w));
    let top_span = horizontal_span(&building.boundary, d).unwrap_or((0, w));
    let overlap = (bottom_span.0.max(top_span.0), bottom_span.1.min(top_span.1));
    let depth_x = if overlap.0 < overlap.1 { (overlap.0 + overlap.1) / 2 } else { w / 2 };
    let corridor_y = (plan.corridor_bottom + plan.corridor_top) / 2;
    lines.push(line("dimension", json!({"refs": [[0, corridor_y], [w, corridor_y]], "at": [w / 2, -1300]})));
    lines.push(line("dimension", json!({"refs": [[depth_x, 0], [depth_x, d]], "at": [w + 900, d / 2]})));
    lines.push(line(
        "dimension",
        json!({"refs": [[top_span.0, d - 500], [top_span.1, d - 500]], "at": [(top_span.0 + top_span.1) / 2, d + 1000]}),
    ));
    lines.push(line("tag", json!({})));
    let note = format!("{} of {} m2 gross, seed {}", capital(plan.archetype.name()), plan.gross_m2().round(), plan.seed);
    lines.push(line("text", json!({"text": note, "at": [0, -2400]})));
    lines.push(line(
        "text",
        json!({"text": "Room areas are to wall finish faces. Made by the BIMCraft graph generator.", "at": [0, -3200]}),
    ));
    let (size, scale) = sheet_for(plan);
    let (sw, sh) = sheet_size(size);
    lines.push(line("sheet", json!({"number": "A101", "name": format!("{} Floor Plan", capital(plan.archetype.name())), "size": size})));
    lines.push(line("place", json!({"sheet": "A101", "view": "Level 1", "scale": scale, "at": [sw / 2, sh / 2 + 10]})));

    let mut text =
        format!("# Generated by the BIMCraft graph generator: {}, seed {}. One command to a line.\n", plan.archetype.name(), plan.seed);
    for l in &lines {
        text.push_str(l.id);
        text.push(' ');
        text.push_str(&l.params.to_string());
        text.push('\n');
    }
    Script { lines, text }
}

fn capital(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
