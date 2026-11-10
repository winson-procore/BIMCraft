//! A plan as ASCII DXF (AutoCAD 2018, AC1032, in millimetres), from the public DXF reference: each primitive of a
//! [`Drawing`] an entity on the AIA layer of its element's category, a line a `LWPOLYLINE` (its weight, true colour and
//! dashes) and a fill a solid `HATCH`. Lettering is the text itself, a `TEXT` (an `MTEXT` for several lines) of style
//! `Standard` on its element's annotation layer, from the drawing's [`super::Lettering`] records; the strokes the plan draws it
//! in stay in the picture and are left out here. Pictures are left out, which [`pictures`] counts for a reply to say.
//!
//! A fill of the paper's colour is a raster trick (what is drawn before it is hidden), and a CAD viewer draws a hatch over
//! the lines under it. So it is never an entity: [`clipped`] bakes the paint order into the geometry, cutting what came
//! before such a fill out of it.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{Drawing, PAPER, Part, Prim, Rgb, boxes_meet};
use crate::family::Category;
use crate::geom::{self, P2};
use crate::model::{Element, Id, Model};
use crate::sheet;
use crate::walls;

/// The line weights a DXF may give, hundredths of a mm.
const WEIGHTS: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

/// The layer of what belongs to no element, or to one no layer is kept for.
const MISC: &str = "A-ANNO-MISC";

/// The most pieces paper may cut a line or a ring into; one it would cut into more stays whole.
const MAX_PIECES: usize = 64;

/// The most corners a paper fill may have unless it's convex: a concave one is cut into triangles, which costs a cube of that.
const MAX_MASK: usize = 128;

// The handles AutoCAD gives a drawing's tables, dictionaries and two spaces, which readers expect there; the rest are given
// from `FIRST`.
const BLOCK_RECORDS: u64 = 0x1;
const LAYERS: u64 = 0x2;
const STYLES: u64 = 0x3;
const LTYPES: u64 = 0x5;
const VIEWS: u64 = 0x6;
const UCSS: u64 = 0x7;
const VPORTS: u64 = 0x8;
const APPIDS: u64 = 0x9;
const DIMSTYLES: u64 = 0xA;
const ROOT: u64 = 0xC;
const GROUPS: u64 = 0xD;
const PAPER_SPACE: u64 = 0x1B;
const MODEL_SPACE: u64 = 0x1F;
const FIRST: u64 = 0x30;

/// The plan `d` of `m` as a DXF file's text.
pub fn dxf(d: &Drawing, m: &Model) -> String {
    let mut w = Out::default();
    let mut ents = String::new();
    let mut layers: BTreeSet<String> = BTreeSet::from(["0".to_string()]);
    let mut ltypes: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut ext: Option<[P2; 2]> = None;
    let mm = |p: &(f32, f32)| (p.0.is_finite() && p.1.is_finite()).then(|| d.view.mm(*p));
    // The lettering goes out as text; a view that cannot place text keeps the strokes it is drawn in.
    let texts = d.view.scale.is_finite() && d.view.scale > 0.0;
    for (i, prim) in &clipped(d) {
        let (i, prim) = (*i, prim.as_ref());
        let owner = d.owners.get(i).copied().flatten();
        let part = d.parts.get(i).copied().unwrap_or_default();
        let base = layer(m, owner);
        if texts && part == Part::Text {
            // Not an entity, but still what the drawing's extents are the box of.
            let rings: Vec<&[(f32, f32)]> = match prim {
                Prim::Line { pts, .. } => vec![pts],
                Prim::Fill { rings, .. } => rings.iter().map(Vec::as_slice).collect(),
                Prim::Image { .. } => Vec::new(),
            };
            for ring in rings {
                let ring: Option<Vec<P2>> = ring.iter().map(mm).collect();
                ext = grow(ext, &ring.unwrap_or_default());
            }
            continue;
        }
        match prim {
            Prim::Line { pts, width, color, dash, closed } => {
                let pts: Option<Vec<P2>> = pts.iter().map(mm).collect();
                let Some(pts) = pts.filter(|p| p.len() >= 2) else { continue };
                let name = on(base, part, false);
                let ltype = dash.as_deref().and_then(|dash| linetype(dash, d.view.scale)).map(|(name, dashes)| {
                    ltypes.insert(name.clone(), dashes);
                    name
                });
                let h = w.handle();
                pair(&mut ents, 0, "LWPOLYLINE");
                entity(&mut ents, h, MODEL_SPACE, &name, ltype.as_deref(), Some(weight(d, *width)), *color);
                pair(&mut ents, 100, "AcDbPolyline");
                pair(&mut ents, 90, pts.len());
                pair(&mut ents, 70, i32::from(*closed));
                for p in &pts {
                    point2(&mut ents, 10, *p);
                }
                ext = grow(ext, &pts);
                layers.insert(name);
            }
            Prim::Fill { rings, color } => {
                let rings: Vec<Vec<P2>> =
                    rings.iter().filter_map(|r| r.iter().map(mm).collect::<Option<Vec<P2>>>()).filter(|r| r.len() >= 3).collect();
                if rings.is_empty() {
                    continue;
                }
                let name = on(base, part, true);
                let h = w.handle();
                pair(&mut ents, 0, "HATCH");
                entity(&mut ents, h, MODEL_SPACE, &name, None, None, *color);
                pair(&mut ents, 100, "AcDbHatch");
                point3(&mut ents, 10, P2(0.0, 0.0));
                pair(&mut ents, 210, 0);
                pair(&mut ents, 220, 0);
                pair(&mut ents, 230, 1);
                pair(&mut ents, 2, "SOLID");
                pair(&mut ents, 70, 1);
                pair(&mut ents, 71, 0);
                pair(&mut ents, 91, rings.len());
                for (k, ring) in rings.iter().enumerate() {
                    // A polyline path; the first the outermost, the rest inside it by odd parity.
                    pair(&mut ents, 92, if k == 0 { 2 | 1 } else { 2 });
                    pair(&mut ents, 72, 0);
                    pair(&mut ents, 73, 1);
                    pair(&mut ents, 93, ring.len());
                    for p in ring {
                        point2(&mut ents, 10, *p);
                    }
                    pair(&mut ents, 97, 0);
                    ext = grow(ext, ring);
                }
                pair(&mut ents, 75, 0);
                pair(&mut ents, 76, 1);
                pair(&mut ents, 98, 0);
                layers.insert(name);
            }
            Prim::Image { .. } => {}
        }
    }
    for l in d.lettering.iter().filter(|_| texts) {
        let (at, height, angle) = (mm(&l.at), f64::from(l.height) / d.view.scale, -f64::from(l.angle).to_degrees());
        let Some(at) = at.filter(|_| height.is_finite() && height > 0.0 && angle.is_finite() && !l.text.trim().is_empty()) else {
            continue;
        };
        let name = on(layer(m, l.owner), Part::Text, false);
        lettering(&mut ents, w.handle(), &name, l, at, height, angle.rem_euclid(360.0));
        layers.insert(name);
    }
    let mut tables = String::new();
    table(&mut tables, &mut w, "VPORT", VPORTS, |_, _, _| 0);
    table(&mut tables, &mut w, "LTYPE", LTYPES, |s, w, owner| {
        for name in ["ByBlock", "ByLayer", "Continuous"] {
            ltype(s, w.handle(), owner, name, &[]);
        }
        for (name, dashes) in &ltypes {
            ltype(s, w.handle(), owner, name, dashes);
        }
        3 + ltypes.len()
    });
    table(&mut tables, &mut w, "LAYER", LAYERS, |s, w, owner| {
        for name in &layers {
            pair(s, 0, "LAYER");
            record(s, 5, w.handle(), owner, "AcDbLayerTableRecord", name);
            pair(s, 62, 7);
            pair(s, 6, "Continuous");
            pair(s, 370, -3);
        }
        layers.len()
    });
    table(&mut tables, &mut w, "STYLE", STYLES, |s, w, owner| {
        pair(s, 0, "STYLE");
        record(s, 5, w.handle(), owner, "AcDbTextStyleTableRecord", "Standard");
        pair(s, 40, 0);
        pair(s, 41, 1);
        pair(s, 50, 0);
        pair(s, 71, 0);
        pair(s, 42, 2.5);
        pair(s, 3, "txt");
        pair(s, 4, "");
        1
    });
    table(&mut tables, &mut w, "VIEW", VIEWS, |_, _, _| 0);
    table(&mut tables, &mut w, "UCS", UCSS, |_, _, _| 0);
    table(&mut tables, &mut w, "APPID", APPIDS, |s, w, owner| {
        pair(s, 0, "APPID");
        record(s, 5, w.handle(), owner, "AcDbRegAppTableRecord", "ACAD");
        1
    });
    table(&mut tables, &mut w, "DIMSTYLE", DIMSTYLES, |s, w, owner| {
        pair(s, 0, "DIMSTYLE");
        record(s, 105, w.handle(), owner, "AcDbDimStyleTableRecord", "Standard");
        1
    });
    table(&mut tables, &mut w, "BLOCK_RECORD", BLOCK_RECORDS, |s, _, owner| {
        for (h, name) in [(MODEL_SPACE, "*Model_Space"), (PAPER_SPACE, "*Paper_Space")] {
            pair(s, 0, "BLOCK_RECORD");
            record(s, 5, h, owner, "AcDbBlockTableRecord", name);
        }
        2
    });
    let mut rest = String::new();
    section(&mut rest, "TABLES", |s| s.push_str(&tables));
    section(&mut rest, "BLOCKS", |s| {
        for (owner, name) in [(MODEL_SPACE, "*Model_Space"), (PAPER_SPACE, "*Paper_Space")] {
            pair(s, 0, "BLOCK");
            hex(s, 5, owner + 1);
            hex(s, 330, owner);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockBegin");
            pair(s, 2, name);
            pair(s, 70, 0);
            point3(s, 10, P2(0.0, 0.0));
            pair(s, 3, name);
            pair(s, 1, "");
            pair(s, 0, "ENDBLK");
            hex(s, 5, owner + 2);
            hex(s, 330, owner);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockEnd");
        }
    });
    section(&mut rest, "ENTITIES", |s| s.push_str(&ents));
    section(&mut rest, "OBJECTS", |s| {
        let (root, groups) = (ROOT, GROUPS);
        pair(s, 0, "DICTIONARY");
        hex(s, 5, root);
        hex(s, 330, 0);
        pair(s, 100, "AcDbDictionary");
        pair(s, 281, 1);
        pair(s, 3, "ACAD_GROUP");
        hex(s, 350, groups);
        pair(s, 0, "DICTIONARY");
        hex(s, 5, groups);
        hex(s, 330, root);
        pair(s, 100, "AcDbDictionary");
        pair(s, 281, 1);
    });
    pair(&mut rest, 0, "EOF");
    let [lo, hi] = ext.unwrap_or_default();
    let mut out = String::with_capacity(rest.len() + 1024);
    section(&mut out, "HEADER", |s| {
        pair(s, 9, "$ACADVER");
        pair(s, 1, "AC1032");
        pair(s, 9, "$HANDSEED");
        hex(s, 5, w.next);
        pair(s, 9, "$DWGCODEPAGE");
        pair(s, 3, "ANSI_1252");
        pair(s, 9, "$INSUNITS");
        pair(s, 70, 4);
        pair(s, 9, "$MEASUREMENT");
        pair(s, 70, 1);
        pair(s, 9, "$LUNITS");
        pair(s, 70, 2);
        pair(s, 9, "$EXTMIN");
        point3(s, 10, lo);
        pair(s, 9, "$EXTMAX");
        point3(s, 10, hi);
        pair(s, 9, "$LTSCALE");
        pair(s, 40, 1);
        pair(s, 9, "$LWDISPLAY");
        pair(s, 290, 1);
    });
    section(&mut out, "CLASSES", |_| {});
    out.push_str(&rest);
    out
}

/// A fill of the paper's colour, whose convex pieces (px) hide what is drawn before the primitive `at`.
struct Mask {
    at: usize,
    pieces: Vec<Vec<P2>>,
}

fn is_paper(p: &Prim) -> bool {
    matches!(p, Prim::Fill { color, .. } if *color == PAPER)
}

fn px(pts: &[(f32, f32)]) -> Vec<P2> {
    pts.iter().map(|p| P2(f64::from(p.0), f64::from(p.1))).collect()
}

fn back(pts: &[P2]) -> Vec<(f32, f32)> {
    pts.iter().map(|p| (p.0 as f32, p.1 as f32)).collect()
}

fn finite(pts: &[P2]) -> bool {
    pts.iter().all(|p| p.0.is_finite() && p.1.is_finite())
}

/// What the paper fills hide, as pieces: none for a fill that has holes, isn't finite, has no area or is a big concave one.
fn masks(d: &Drawing) -> Vec<Mask> {
    let mut out = Vec::new();
    for (at, prim) in d.prims.iter().enumerate() {
        let Prim::Fill { rings, color } = prim else { continue };
        if *color != PAPER {
            continue;
        }
        let rings: Vec<Vec<P2>> = rings.iter().map(|r| px(r)).filter(|r| r.len() >= 3).collect();
        let turns = |r: &Vec<P2>| geom::area(r) > 0.0;
        if rings.is_empty() || !rings.iter().all(|r| finite(r)) || rings.iter().any(|r| turns(r) != turns(&rings[0])) {
            continue;
        }
        let mut pieces = Vec::new();
        for r in &rings {
            if r.len() <= MAX_MASK || walls::convex_pieces(r).len() <= 1 {
                pieces.extend(walls::convex_pieces(r).into_iter().filter(|p| geom::area(p).abs() > 0.5));
            }
        }
        if !pieces.is_empty() {
            out.push(Mask { at, pieces });
        }
    }
    out
}

/// `line` less the paper `masks` cover; none when they leave it whole.
fn cut_line(line: &[P2], masks: &[&Mask]) -> Option<Vec<Vec<P2>>> {
    let mut parts = vec![line.to_vec()];
    for piece in masks.iter().flat_map(|k| &k.pieces) {
        parts =
            parts.into_iter().flat_map(|l| if boxes_meet(&l, piece) { geom::line_outside_convex(&l, piece) } else { vec![l] }).collect();
        if parts.len() > MAX_PIECES {
            return None;
        }
    }
    (parts.len() != 1 || parts[0] != line).then_some(parts)
}

/// `ring` less the paper `masks` cover, as convex pieces; none when they leave it whole.
fn cut_ring(ring: &[P2], masks: &[&Mask]) -> Option<Vec<Vec<P2>>> {
    let (mut parts, mut cut) = (vec![ring.to_vec()], false);
    for piece in masks.iter().flat_map(|k| &k.pieces) {
        let mut next = Vec::new();
        for part in parts {
            if !boxes_meet(&part, piece) {
                next.push(part);
                continue;
            }
            let (mut rest, mut hit) = (Vec::new(), false);
            for convex in walls::convex_pieces(&part) {
                let (inside, outside) = walls::split(convex.clone(), piece);
                if inside.is_some() {
                    hit = true;
                    rest.extend(outside);
                } else {
                    rest.push(convex);
                }
            }
            if hit {
                cut = true;
                next.extend(rest);
            } else {
                next.push(part);
            }
        }
        parts = next;
        if parts.len() > MAX_PIECES {
            return None;
        }
    }
    cut.then_some(parts)
}

/// The primitives of `d` as a CAD drawing has them (with the index of the one each is of): the paper fills gone, and what each
/// hid of what came before it cut out of that, so a line a mask crosses is the pieces outside it and a fill is convex pieces,
/// one hatch apiece.
fn clipped(d: &Drawing) -> Vec<(usize, Cow<'_, Prim>)> {
    let masks = masks(d);
    let mut out = Vec::with_capacity(d.prims.len());
    for (i, prim) in d.prims.iter().enumerate() {
        if is_paper(prim) {
            continue;
        }
        let later: Vec<&Mask> = masks[masks.partition_point(|k| k.at <= i)..].iter().collect();
        if later.is_empty() {
            out.push((i, Cow::Borrowed(prim)));
            continue;
        }
        match prim {
            Prim::Line { pts, width, color, dash, closed } => {
                let mut line = px(pts);
                if *closed {
                    line.extend(line.first().copied());
                }
                let pieces = (line.len() >= 2 && finite(&line)).then(|| cut_line(&line, &later)).flatten();
                let Some(pieces) = pieces else {
                    out.push((i, Cow::Borrowed(prim)));
                    continue;
                };
                for piece in pieces.iter().filter(|p| p.len() >= 2) {
                    let (width, color, dash) = (*width, *color, dash.clone());
                    out.push((i, Cow::Owned(Prim::Line { pts: back(piece), width, color, dash, closed: false })));
                }
            }
            Prim::Fill { rings, color } => {
                let px_rings: Vec<Vec<P2>> = rings.iter().map(|r| px(r)).collect();
                let turns = |r: &Vec<P2>| geom::area(r) > 0.0;
                // Holes (rings wound the other way) are left as they are: only a fill of pieces is cut.
                let plain = px_rings.iter().all(|r| finite(r)) && px_rings.iter().all(|r| turns(r) == turns(&px_rings[0]));
                let cuts: Vec<Option<Vec<Vec<P2>>>> =
                    if plain { px_rings.iter().map(|r| cut_ring(r, &later)).collect() } else { Vec::new() };
                if cuts.iter().all(Option::is_none) {
                    out.push((i, Cow::Borrowed(prim)));
                    continue;
                }
                let whole: Vec<Vec<(f32, f32)>> = rings.iter().zip(&cuts).filter(|(_, c)| c.is_none()).map(|(r, _)| r.clone()).collect();
                if !whole.is_empty() {
                    out.push((i, Cow::Owned(Prim::Fill { rings: whole, color: *color })));
                }
                for piece in cuts.iter().flatten().flatten() {
                    out.push((i, Cow::Owned(Prim::Fill { rings: vec![back(piece)], color: *color })));
                }
            }
            Prim::Image { .. } => out.push((i, Cow::Borrowed(prim))),
        }
    }
    out
}

/// How many pictures `d` holds, which [`dxf`] leaves out.
pub fn pictures(d: &Drawing) -> usize {
    d.prims.iter().filter(|p| matches!(p, Prim::Image { .. })).count()
}

/// The AIA layer of what `owner` draws, by its category: one table, so a change stays here.
fn layer(m: &Model, owner: Option<Id>) -> &'static str {
    let Some(e) = owner.and_then(|id| m.elements.get(id)) else { return MISC };
    match e {
        Element::Wall(_) => "A-WALL",
        Element::Door(_) => "A-DOOR",
        Element::Window(_) => "A-GLAZ",
        Element::Floor(_) => "A-FLOR",
        Element::Ceiling(_) => "A-CLNG",
        Element::Roof(_) => "A-ROOF",
        Element::Shaft(_) => "A-FLOR-OPEN",
        Element::Column(_) => "A-COLS",
        Element::StructuralColumn(_) => "S-COLS",
        Element::Beam(_) => "S-BEAM",
        Element::Footing(_) | Element::StripFooting(_) => "S-FNDN",
        Element::Rebar(_) => "S-REIN",
        Element::Stair(_) | Element::Ramp(_) => "A-STRS",
        Element::Railing(_) => "A-FLOR-HRAL",
        Element::Grid(_) => "A-GRID",
        Element::Room(_) => "A-AREA-IDEN",
        Element::RoomSeparator(_) => "A-AREA-BNDY",
        Element::Dimension(_) => "A-ANNO-DIMS",
        Element::Text(_) => "A-ANNO-TEXT",
        Element::Tag(_) | Element::Spot(_) | Element::Section(_) | Element::Elevation(_) => "A-ANNO-SYMB",
        Element::DetailLine(_) | Element::FilledRegion(_) | Element::DetailGroup(_) | Element::RepeatingDetail(_) => "A-DETL",
        Element::Mass(_) => "A-MASS",
        Element::Pipe(_) => "P-PIPE",
        Element::Duct(_) => "M-DUCT",
        Element::Conduit(_) => "E-POWR-COND",
        Element::CableTray(_) => "E-POWR-TRAY",
        Element::Wire(_) | Element::Cable(_) => "E-POWR-WIRE",
        Element::DownConductor(_) | Element::GroundConductor(_) => "E-GRND",
        Element::Component(c) => match m.families.get(&c.family).map(|f| f.category) {
            Some(Category::Furniture) => "A-FURN",
            Some(Category::Casework) => "A-FLOR-CASE",
            Some(Category::Plumbing) => "P-FIXT",
            Some(Category::Drain) => "P-SANR",
            Some(Category::PipeFitting | Category::PipeAccessory) => "P-PIPE",
            Some(Category::Equipment) => "E-POWR-EQPM",
            Some(Category::Outlet) => "E-POWR",
            Some(Category::Switch) => "E-LITE-SWCH",
            Some(Category::Lighting) => "E-LITE",
            Some(Category::AirTerminal | Category::Earthing | Category::InspectionBox) => "E-GRND",
            Some(Category::DataOutlet | Category::VoiceOutlet | Category::PatchPanel | Category::Rack) => "E-COMM",
            Some(Category::Door) => "A-DOOR",
            Some(Category::Window) => "A-GLAZ",
            Some(Category::Generic) | None => "A-GENM",
        },
        _ => MISC,
    }
}

/// The layer a primitive of `part` goes on, its element's being `base`: lettering on its annotation layer (`-IDEN`, for
/// a model element), a model element's fills and patterns on `-PATT`; annotation keeps all of it on its own.
fn on(base: &str, part: Part, fill: bool) -> String {
    let annotation = base.starts_with("A-ANNO") || base.ends_with("-IDEN");
    match part {
        Part::Text if !annotation => format!("{base}-IDEN"),
        Part::Pattern | Part::LinePattern if !annotation => format!("{base}-PATT"),
        _ if fill && !annotation => format!("{base}-PATT"),
        _ => base.to_string(),
    }
}

/// A dash pattern of `dash` px, drawn `scale` px to the mm: its name and its dashes and gaps, mm (a gap negative). None
/// for one that isn't a pattern of finite lengths, which is drawn solid.
fn linetype(dash: &[f32], scale: f64) -> Option<(String, Vec<f64>)> {
    if dash.is_empty() || dash.len() > 32 || !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    // An odd list repeats to make its dashes and gaps alternate.
    let twice = if dash.len() % 2 == 1 { 2 } else { 1 };
    let mut dashes = Vec::with_capacity(dash.len() * twice);
    for (k, v) in dash.iter().cycle().take(dash.len() * twice).enumerate() {
        let v = f64::from(*v);
        if !(v.is_finite() && v >= 0.0) {
            return None;
        }
        let mm = (v / scale * 10.0).round() / 10.0;
        dashes.push(if k % 2 == 0 { mm } else { -mm });
    }
    if dashes.iter().all(|v| *v == 0.0) {
        return None;
    }
    let name = dashes.iter().map(|v| num(v.abs())).collect::<Vec<_>>().join("_");
    Some((format!("BIM-DASH-{name}"), dashes))
}

/// The DXF line weight nearest a line `width` px wide in `d`, hundredths of a mm on paper.
fn weight(d: &Drawing, width: f32) -> i32 {
    let per_mm = sheet::DENSITY * if d.paper.is_finite() && d.paper > 0.0 { f64::from(d.paper) } else { 1.0 };
    let mm = f64::from(width) / per_mm;
    if !mm.is_finite() || mm <= 0.0 {
        return 0;
    }
    let want = mm * 100.0;
    WEIGHTS.iter().copied().min_by(|a, b| (f64::from(*a) - want).abs().total_cmp(&(f64::from(*b) - want).abs())).unwrap_or(0)
}

/// The box round `ext` and `pts`.
fn grow(ext: Option<[P2; 2]>, pts: &[P2]) -> Option<[P2; 2]> {
    crate::dxf::union(ext.into_iter().chain(pts.iter().map(|p| [*p, *p])))
}

/// Handles as they're given out, from [`FIRST`].
struct Out {
    next: u64,
}

impl Default for Out {
    fn default() -> Out {
        Out { next: FIRST }
    }
}

impl Out {
    fn handle(&mut self) -> u64 {
        let h = self.next;
        self.next += 1;
        h
    }
}

fn pair(s: &mut String, code: i32, value: impl std::fmt::Display) {
    let _ = write!(s, "{code:>3}\n{value}\n");
}

fn hex(s: &mut String, code: i32, h: u64) {
    let _ = write!(s, "{code:>3}\n{h:X}\n");
}

/// A number in mm, to a ten-thousandth, without trailing zeros.
fn num(x: f64) -> String {
    let t = format!("{x:.4}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" { "0".into() } else { t.into() }
}

fn point2(s: &mut String, code: i32, p: P2) {
    pair(s, code, num(p.0));
    pair(s, code + 10, num(p.1));
}

fn point3(s: &mut String, code: i32, p: P2) {
    point2(s, code, p);
    pair(s, code + 20, 0);
}

fn section(s: &mut String, name: &str, body: impl FnOnce(&mut String)) {
    pair(s, 0, "SECTION");
    pair(s, 2, name);
    body(s);
    pair(s, 0, "ENDSEC");
}

/// A symbol table, its records written by `records` (given the table's handle, which owns them), which says how many.
fn table(s: &mut String, w: &mut Out, name: &str, h: u64, records: impl FnOnce(&mut String, &mut Out, u64) -> usize) {
    let mut body = String::new();
    let n = records(&mut body, w, h);
    pair(s, 0, "TABLE");
    pair(s, 2, name);
    hex(s, 5, h);
    hex(s, 330, 0);
    pair(s, 100, "AcDbSymbolTable");
    pair(s, 70, n);
    if name == "DIMSTYLE" {
        pair(s, 100, "AcDbDimStyleTable");
    }
    s.push_str(&body);
    pair(s, 0, "ENDTAB");
}

/// The head of a table's record: its handle (under `code`), owner, subclass and name.
fn record(s: &mut String, code: i32, h: u64, owner: u64, class: &str, name: &str) {
    hex(s, code, h);
    hex(s, 330, owner);
    pair(s, 100, "AcDbSymbolTableRecord");
    pair(s, 100, class);
    pair(s, 2, name);
    pair(s, 70, 0);
}

fn ltype(s: &mut String, h: u64, owner: u64, name: &str, dashes: &[f64]) {
    pair(s, 0, "LTYPE");
    record(s, 5, h, owner, "AcDbLinetypeTableRecord", name);
    pair(s, 3, if dashes.is_empty() { "Solid line" } else { "" });
    pair(s, 72, 65);
    pair(s, 73, dashes.len());
    pair(s, 40, num(dashes.iter().map(|v| v.abs()).sum()));
    for v in dashes {
        pair(s, 49, num(*v));
        pair(s, 74, 0);
    }
}

/// The common head of an entity: handle, owner, layer, linetype, weight and true colour.
fn entity(s: &mut String, h: u64, owner: u64, layer: &str, ltype: Option<&str>, weight: Option<i32>, color: Rgb) {
    hex(s, 5, h);
    hex(s, 330, owner);
    pair(s, 100, "AcDbEntity");
    pair(s, 8, layer);
    if let Some(lt) = ltype {
        pair(s, 6, lt);
    }
    if let Some(w) = weight {
        pair(s, 370, w);
    }
    pair(s, 420, (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2));
}

/// The text style every text is in, which the STYLE table defines.
const STYLE: &str = "Standard";

/// The longest string a TEXT holds, or a group of an MTEXT's: the DXF reference's 255 less room for what a reader adds.
const CHUNK: usize = 250;

/// `l` as a TEXT, or as an MTEXT when it has lines or something a TEXT reads as formatting, at `at` mm and turned `angle`
/// degrees counter-clockwise. Its characters outside ASCII go out as they are: the file is AC1032, whose DXF is UTF-8.
fn lettering(s: &mut String, h: u64, layer: &str, l: &super::Lettering, at: P2, height: f64, angle: f64) {
    use crate::dxf::{Horizontal, Vertical};
    let (right, down) = (
        match l.align.h {
            Horizontal::Left => 0,
            Horizontal::Center => 1,
            Horizontal::Right => 2,
        },
        match l.align.v {
            Vertical::Baseline => 0,
            Vertical::Bottom => 1,
            Vertical::Middle => 2,
            Vertical::Top => 3,
        },
    );
    let plain = escaped(&l.text, false);
    // Lettering a plan laid out in a typeface goes as an MTEXT with that typeface's run, which is how a reader that drew it
    // the same finds the face; a TEXT has no way to say it.
    let face: String =
        l.font.iter().flat_map(|f| f.chars()).filter(|c| !c.is_control() && !"|;\\{}".contains(*c)).collect::<String>().trim().into();
    if face.is_empty() && !l.text.contains(['\n', '\\', '{', '}']) && plain.len() <= CHUNK {
        pair(s, 0, "TEXT");
        entity(s, h, MODEL_SPACE, layer, None, None, l.color);
        pair(s, 100, "AcDbText");
        point3(s, 10, at);
        pair(s, 40, num(height));
        pair(s, 1, plain);
        pair(s, 50, num(angle));
        pair(s, 7, STYLE);
        if right != 0 || down != 0 {
            pair(s, 72, right);
            point3(s, 11, at);
        }
        pair(s, 100, "AcDbText");
        pair(s, 73, down);
        return;
    }
    // A text's top is DXF's top attachment, its baseline the bottom's: an MTEXT's attachment point is 1 to 9, row by row.
    let attach =
        1 + match down {
            3 => 0,
            2 => 3,
            _ => 6,
        } + right;
    let mut body = escaped(&l.text, true);
    if !face.is_empty() {
        body = format!("{{\\f{face}|b0|i0|c0|p34;{body}}}");
    }
    pair(s, 0, "MTEXT");
    entity(s, h, MODEL_SPACE, layer, None, None, l.color);
    pair(s, 100, "AcDbMText");
    point3(s, 10, at);
    pair(s, 40, num(height));
    pair(s, 41, 0);
    pair(s, 71, attach);
    pair(s, 72, 1);
    let mut rest = body.as_str();
    while rest.len() > CHUNK {
        let cut = (1..=CHUNK).rev().find(|&i| rest.is_char_boundary(i)).unwrap_or(CHUNK);
        let (head, tail) = rest.split_at(cut);
        pair(s, 3, head);
        rest = tail;
    }
    pair(s, 1, rest);
    pair(s, 7, STYLE);
    let (sin, cos) = angle.to_radians().sin_cos();
    pair(s, 11, num(cos));
    pair(s, 21, num(sin));
    pair(s, 31, 0);
    pair(s, 73, 1);
    // Our lines are 1.6 capitals apart; an MTEXT's are 5/3 of them, times this.
    pair(s, 44, 0.96);
}

/// `text` as a DXF string, ASCII: for `multi` an MTEXT's, whose backslashes and braces are escaped and lines parted by `\P`, else a
/// TEXT's, which has no lines. A control character is a space.
fn escaped(text: &str, multi: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' if multi => out.push_str("\\P"),
            '\\' | '{' | '}' if multi => {
                out.push('\\');
                out.push(c);
            }
            '\r' if multi => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
