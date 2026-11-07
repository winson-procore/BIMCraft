//! A plan as ASCII DXF (AutoCAD 2018, AC1032, in millimetres), from the public DXF reference: each primitive of a
//! [`Drawing`] an entity on the AIA layer of its element's category, a line a `LWPOLYLINE` (its weight, true colour and
//! dashes) and a fill a solid `HATCH`. Lettering stays the strokes the plan draws it in. Pictures are left out, which
//! [`pictures`] counts for a reply to say.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{Drawing, Part, Prim, Rgb};
use crate::family::Category;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::sheet;

/// The line weights a DXF may give, hundredths of a mm.
const WEIGHTS: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];

/// The layer of what belongs to no element, or to one no layer is kept for.
const MISC: &str = "A-ANNO-MISC";

/// The plan `d` of `m` as a DXF file's text.
pub fn dxf(d: &Drawing, m: &Model) -> String {
    let mut w = Out::default();
    let mut ents = String::new();
    let mut layers: BTreeSet<String> = BTreeSet::from(["0".to_string()]);
    let mut ltypes: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut ext: Option<[P2; 2]> = None;
    let model_space = w.handle();
    let paper_space = w.handle();
    let mm = |p: &(f32, f32)| (p.0.is_finite() && p.1.is_finite()).then(|| d.view.mm(*p));
    for (i, prim) in d.prims.iter().enumerate() {
        let owner = d.owners.get(i).copied().flatten();
        let part = d.parts.get(i).copied().unwrap_or_default();
        let base = layer(m, owner);
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
                entity(&mut ents, h, model_space, &name, ltype.as_deref(), Some(weight(d, *width)), *color);
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
                entity(&mut ents, h, model_space, &name, None, None, *color);
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
    let mut tables = String::new();
    table(&mut tables, &mut w, "VPORT", |_, _, _| 0);
    table(&mut tables, &mut w, "LTYPE", |s, w, owner| {
        for name in ["ByBlock", "ByLayer", "Continuous"] {
            ltype(s, w.handle(), owner, name, &[]);
        }
        for (name, dashes) in &ltypes {
            ltype(s, w.handle(), owner, name, dashes);
        }
        3 + ltypes.len()
    });
    table(&mut tables, &mut w, "LAYER", |s, w, owner| {
        for name in &layers {
            pair(s, 0, "LAYER");
            record(s, 5, w.handle(), owner, "AcDbLayerTableRecord", name);
            pair(s, 62, 7);
            pair(s, 6, "Continuous");
            pair(s, 370, -3);
        }
        layers.len()
    });
    table(&mut tables, &mut w, "STYLE", |s, w, owner| {
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
    table(&mut tables, &mut w, "VIEW", |_, _, _| 0);
    table(&mut tables, &mut w, "UCS", |_, _, _| 0);
    table(&mut tables, &mut w, "APPID", |s, w, owner| {
        pair(s, 0, "APPID");
        record(s, 5, w.handle(), owner, "AcDbRegAppTableRecord", "ACAD");
        1
    });
    table(&mut tables, &mut w, "DIMSTYLE", |s, w, owner| {
        pair(s, 0, "DIMSTYLE");
        record(s, 105, w.handle(), owner, "AcDbDimStyleTableRecord", "Standard");
        1
    });
    table(&mut tables, &mut w, "BLOCK_RECORD", |s, _, owner| {
        for (h, name) in [(model_space, "*Model_Space"), (paper_space, "*Paper_Space")] {
            pair(s, 0, "BLOCK_RECORD");
            record(s, 5, h, owner, "AcDbBlockTableRecord", name);
        }
        2
    });
    let mut rest = String::new();
    section(&mut rest, "TABLES", |s| s.push_str(&tables));
    section(&mut rest, "BLOCKS", |s| {
        for (owner, name) in [(model_space, "*Model_Space"), (paper_space, "*Paper_Space")] {
            pair(s, 0, "BLOCK");
            hex(s, 5, w.handle());
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
            hex(s, 5, w.handle());
            hex(s, 330, owner);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockEnd");
        }
    });
    section(&mut rest, "ENTITIES", |s| s.push_str(&ents));
    section(&mut rest, "OBJECTS", |s| {
        let (root, groups) = (w.handle(), w.handle());
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

/// Handles as they're given out, from 1.
struct Out {
    next: u64,
}

impl Default for Out {
    fn default() -> Out {
        Out { next: 1 }
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
fn table(s: &mut String, w: &mut Out, name: &str, records: impl FnOnce(&mut String, &mut Out, u64) -> usize) {
    let h = w.handle();
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

#[cfg(test)]
mod tests;
