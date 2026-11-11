//! Where the program goes: one rectangle, a corridor across its middle, the rooms dealt into a band on each side of it. All
//! in whole millimetres, on wall centre lines, so the same brief gives the same plan on any machine.
//!
//! ```text
//!   +------------------------------+   y = depth
//!   |  north band: rooms, each doored to the corridor
//!   +------------------------------+   y = corridor_top
//!   |  corridor, the whole width   |
//!   +------------------------------+   y = corridor_bottom
//!   |  lobby | south band: rooms   |   (the entry is in the lobby's west wall)
//!   +------------------------------+   y = 0
//! ```

use super::Rng;
use super::program::{Archetype, Program, Spec, program};

pub const EXTERIOR: &str = "Exterior - 300mm";
pub const INTERIOR: &str = "Interior - 100mm";
/// The narrowest room, so a door and a window sit in it clear of its walls.
pub const MIN_CELL_WIDTH: i64 = 2400;
/// A plan's depth is the square root of its area over this, so it is about this many times as long as it is deep.
const LENGTH_TO_DEPTH: f64 = 3.0;
const MIN_DEPTH: i64 = 12_000;
const MAX_DEPTH: i64 = 24_000;
const MIN_WIDTH: i64 = 6_000;
const CORRIDOR_WIDTHS: [i64; 4] = [1800, 2000, 2200, 2400];
/// One window for each this much of outside wall.
const WINDOW_SPACING: i64 = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    North,
    South,
}

impl Band {
    pub fn name(self) -> &'static str {
        match self {
            Band::North => "north",
            Band::South => "south",
        }
    }
}

/// A room's rectangle, on wall centre lines.
#[derive(Clone, Debug)]
pub struct Cell {
    pub spec: Spec,
    pub band: Band,
    pub x0: i64,
    pub x1: i64,
    pub y0: i64,
    pub y1: i64,
}

impl Cell {
    pub fn middle(&self) -> [i64; 2] {
        [(self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2]
    }

    pub fn area_mm2(&self) -> i64 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
}

#[derive(Clone, Debug)]
pub struct Wall {
    pub kind: &'static str,
    pub points: Vec<[i64; 2]>,
    pub closed: bool,
}

#[derive(Clone, Debug)]
pub struct Opening {
    pub kind: &'static str,
    pub space: String,
    pub at: [i64; 2],
    /// Where the building is entered from outside.
    pub entry: bool,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub archetype: Archetype,
    pub seed: u64,
    pub requested_m2: f64,
    pub width: i64,
    pub depth: i64,
    pub corridor_bottom: i64,
    pub corridor_top: i64,
    pub program: Program,
    /// The lobby first, then the south band west to east, then the north band.
    pub cells: Vec<Cell>,
    pub walls: Vec<Wall>,
    pub openings: Vec<Opening>,
}

impl Plan {
    pub fn corridor_width(&self) -> i64 {
        self.corridor_top - self.corridor_bottom
    }

    pub fn gross_m2(&self) -> f64 {
        mm2_to_m2(self.width * self.depth)
    }

    pub fn corridor_area_mm2(&self) -> i64 {
        self.width * self.corridor_width()
    }

    /// The cells of one band, west to east.
    pub fn band(&self, band: Band) -> impl Iterator<Item = &Cell> {
        self.cells.iter().filter(move |c| c.band == band)
    }
}

pub fn mm2_to_m2(mm2: i64) -> f64 {
    ((mm2 as f64) / 10_000.0).round() / 100.0
}

fn round_to_100(x: f64) -> i64 {
    ((x / 100.0).round() as i64) * 100
}

/// A plan for the brief, or why none fits.
pub fn plan(archetype: Archetype, area_m2: f64, seed: u64) -> Result<Plan, String> {
    let area_mm2 = area_m2 * 1.0e6;
    let depth = round_to_100((area_mm2 / LENGTH_TO_DEPTH).sqrt()).clamp(MIN_DEPTH, MAX_DEPTH);
    let width = round_to_100(area_mm2 / depth as f64);
    if width < MIN_WIDTH {
        return Err(format!("a plan {depth} mm deep and {width} mm long is narrower than the {MIN_WIDTH} mm a corridor and a room need"));
    }
    let program = program(archetype, area_m2, seed);
    let mut rng = Rng::new(seed, 0x1A7_0001);
    let corridor_width = CORRIDOR_WIDTHS[rng.below(CORRIDOR_WIDTHS.len() as u64) as usize];
    let corridor_bottom = (depth - corridor_width) / 2 / 100 * 100;
    let corridor_top = corridor_bottom + corridor_width;

    // The rooms in an order of the seed's, each to the band with less of the program in it so far; the lobby is the south band's first.
    let mut rooms = program.cells.clone();
    rng.shuffle(&mut rooms);
    let (mut south, mut north) = (vec![program.lobby.clone()], Vec::new());
    let (mut south_target, mut north_target) = (i64::from(program.lobby.target_m2), 0_i64);
    for room in rooms {
        let target = i64::from(room.target_m2);
        if north_target <= south_target {
            north_target += target;
            north.push(room);
        } else {
            south_target += target;
            south.push(room);
        }
    }

    let mut cells = Vec::new();
    for (band, specs, depth_of_band, y0, y1) in
        [(Band::South, south, corridor_bottom, 0, corridor_bottom), (Band::North, north, depth - corridor_top, corridor_top, depth)]
    {
        cells.extend(deal(band, specs, depth_of_band, width, y0, y1)?);
    }
    let walls = walls(width, depth, corridor_bottom, corridor_top, &cells);
    let openings = openings(&cells, corridor_bottom, corridor_top, depth);
    Ok(Plan { archetype, seed, requested_m2: area_m2, width, depth, corridor_bottom, corridor_top, program, cells, walls, openings })
}

/// A band's rooms along its length: each as wide as its least area asks, the rest shared out by what they would like.
fn deal(band: Band, specs: Vec<Spec>, band_depth: i64, length: i64, y0: i64, y1: i64) -> Result<Vec<Cell>, String> {
    let least: Vec<i64> = specs
        .iter()
        .map(|s| {
            let for_area = (i64::from(s.min_m2) * 1_000_000 + band_depth - 1) / band_depth.max(1);
            MIN_CELL_WIDTH.max((for_area + 99) / 100 * 100)
        })
        .collect();
    let needed: i64 = least.iter().sum();
    if needed > length {
        return Err(format!("the {} band needs {needed} mm of rooms for its minimum program and has {length} mm", band.name()));
    }
    let (spare, wanted) = (length - needed, specs.iter().map(|s| i64::from(s.target_m2)).sum::<i64>().max(1));
    let mut widths: Vec<i64> = specs.iter().zip(&least).map(|(s, l)| l + spare * i64::from(s.target_m2) / wanted / 100 * 100).collect();
    let left = length - widths.iter().sum::<i64>();
    let widest = specs.iter().enumerate().fold(0, |best, (i, s)| if s.target_m2 > specs[best].target_m2 { i } else { best });
    if let Some(w) = widths.get_mut(widest) {
        *w += left;
    }
    let mut x = 0;
    Ok(specs
        .into_iter()
        .zip(widths)
        .map(|(spec, w)| {
            let cell = Cell { spec, band, x0: x, x1: x + w, y0, y1 };
            x += w;
            cell
        })
        .collect())
}

fn walls(width: i64, depth: i64, bottom: i64, top: i64, cells: &[Cell]) -> Vec<Wall> {
    let mut walls = vec![
        Wall { kind: "exterior", points: vec![[0, 0], [0, depth], [width, depth], [width, 0]], closed: true },
        Wall { kind: "corridor", points: vec![[0, bottom], [width, bottom]], closed: false },
        Wall { kind: "corridor", points: vec![[0, top], [width, top]], closed: false },
    ];
    for c in cells.iter().filter(|c| c.x1 < width) {
        walls.push(Wall { kind: "partition", points: vec![[c.x1, c.y0], [c.x1, c.y1]], closed: false });
    }
    walls
}

/// The way in, a door from every room to the corridor, and windows for the rooms that want them.
fn openings(cells: &[Cell], bottom: i64, top: i64, depth: i64) -> Vec<Opening> {
    let mut out = Vec::new();
    if let Some(lobby) = cells.first() {
        out.push(Opening { kind: "door", space: lobby.spec.id.clone(), at: [0, bottom / 2], entry: true });
    }
    for c in cells {
        let (inside, outside) = if c.band == Band::North { (top, depth) } else { (bottom, 0) };
        out.push(Opening { kind: "door", space: c.spec.id.clone(), at: [c.middle()[0], inside], entry: false });
        if c.spec.window {
            let width = c.x1 - c.x0;
            let n = (width / WINDOW_SPACING).max(1);
            for i in 0..n {
                out.push(Opening {
                    kind: "window",
                    space: c.spec.id.clone(),
                    at: [c.x0 + width * (2 * i + 1) / (2 * n), outside],
                    entry: false,
                });
            }
        }
    }
    out
}
