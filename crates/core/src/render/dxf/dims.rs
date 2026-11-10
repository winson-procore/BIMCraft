//! A plan's aligned and linear dimensions as `DIMENSION` entities, from the drawing's [`DimensionRecord`]s: one for each segment,
//! of a `DIMSTYLE` named for its dimension type. What a dimension draws (its dimension line, witness lines and ticks) is the
//! entity's anonymous block, `*D1`, `*D2`, ..., held by the first of its segments, as a CAD program keeps it, so the drawing
//! carries it once and a reader that does not rebuild a dimension still draws it. The number is the `TEXT` beside it, so the
//! entity's own is blank.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::render::DimensionRecord;

/// The most segments written; a drawing with more keeps the rest stroked.
const MAX_DIMENSIONS: usize = 20_000;

/// The characters a table name may not hold in a DXF.
const ILLEGAL: [char; 13] = ['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'];

/// The longest style name written.
const MAX_NAME: usize = 100;

/// What one dimension element draws, held under a block of its own.
pub(super) struct Block {
    pub name: String,
    pub record: u64,
    begin: u64,
    end: u64,
    /// The entities of its strokes.
    pub body: String,
}

/// A `DIMSTYLE`: the name its dimensions give and the sizes it holds, model mm.
struct Style {
    name: String,
    record: DimensionRecord,
}

/// The dimensions a drawing writes.
#[derive(Default)]
pub(super) struct Dims {
    /// The `DIMENSION` entities.
    pub entities: String,
    /// The block of each dimension element written, by element.
    pub blocks: BTreeMap<Id, Block>,
    styles: Vec<Style>,
}

/// A name a DXF table may hold, which no other of `taken` (compared as a CAD program does, without case) is, and which is not
/// the `Standard` style every file has.
fn safe(raw: &str, taken: &BTreeSet<String>) -> String {
    let cleaned: String = raw.chars().map(|c| if c.is_control() || ILLEGAL.contains(&c) { '_' } else { c }).take(MAX_NAME).collect();
    let base = cleaned.trim();
    let base = if base.is_empty() { "BIM-DIM" } else { base };
    let mut name = base.to_string();
    let mut n = 1;
    while name.eq_ignore_ascii_case("Standard") || taken.contains(&name.to_uppercase()) {
        n += 1;
        name = format!("{base}_{n}");
    }
    name
}

fn placeable(r: &DimensionRecord) -> bool {
    let points = [r.from, r.to, r.line, r.text];
    let numbers = [r.measure, r.text_height, r.tick_size, r.witness_gap, r.witness_extension, r.text_offset];
    r.owner.is_some()
        && finite(&points)
        && r.linear.is_none_or(f64::is_finite)
        && numbers.iter().all(|v| v.is_finite())
        && r.measure > 0.0
        && r.text_height > 0.0
}

impl Dims {
    /// The dimension entities `d` holds that can be written, with a block for each element and a style for each type; empty for a
    /// view that cannot place them. Handles come from `w`; what they cover grows `ext` and what they are on goes in `layers`.
    pub fn of(d: &Drawing, m: &Model, texts: bool, w: &mut Out, ext: &mut Option<[P2; 2]>, layers: &mut BTreeSet<String>) -> Dims {
        let mut out = Dims::default();
        if !texts {
            return out;
        }
        let mut names: BTreeMap<&str, String> = BTreeMap::new();
        let mut taken = BTreeSet::new();
        for r in d.dimensions.iter().filter(|r| placeable(r)).take(MAX_DIMENSIONS) {
            let Some(owner) = r.owner else { continue };
            let name = match names.get(r.style.as_str()) {
                Some(name) => name.clone(),
                None => {
                    let name = safe(&r.style, &taken);
                    taken.insert(name.to_uppercase());
                    names.insert(r.style.as_str(), name.clone());
                    out.styles.push(Style { name: name.clone(), record: r.clone() });
                    name
                }
            };
            let first = !out.blocks.contains_key(&owner);
            if first {
                let n = out.blocks.len() + 1;
                let block = Block { name: format!("*D{n}"), record: w.handle(), begin: w.handle(), end: w.handle(), body: String::new() };
                out.blocks.insert(owner, block);
            }
            let block = out.blocks.get(&owner).filter(|_| first).map(|b| b.name.as_str());
            let layer = layer(m, Some(owner));
            let color = match m.elements.get(owner) {
                Some(Element::Dimension(dim)) => {
                    let c = m.dimension_type(dim).color;
                    Rgb(c[0], c[1], c[2])
                }
                _ => Rgb(0, 0, 0),
            };
            entity_of(&mut out.entities, w.handle(), layer, color, &name, block, r);
            *ext = grow(*ext, &[r.from, r.to, r.line, r.text]);
            layers.insert(layer.to_string());
        }
        out
    }

    /// Its blocks' records, for the `BLOCK_RECORD` table.
    pub fn block_records(&self, s: &mut String, owner: u64) {
        for b in self.blocks.values() {
            pair(s, 0, "BLOCK_RECORD");
            record(s, 5, b.record, owner, "AcDbBlockTableRecord", &b.name);
        }
    }

    /// Its blocks, for the `BLOCKS` section.
    pub fn block_section(&self, s: &mut String) {
        for b in self.blocks.values() {
            pair(s, 0, "BLOCK");
            hex(s, 5, b.begin);
            hex(s, 330, b.record);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockBegin");
            pair(s, 2, &b.name);
            // Anonymous.
            pair(s, 70, 1);
            point3(s, 10, P2(0.0, 0.0));
            pair(s, 3, &b.name);
            pair(s, 1, "");
            s.push_str(&b.body);
            pair(s, 0, "ENDBLK");
            hex(s, 5, b.end);
            hex(s, 330, b.record);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, "0");
            pair(s, 100, "AcDbBlockEnd");
        }
    }

    /// Its styles, for the `DIMSTYLE` table, after the one every file has; how many.
    pub fn style_records(&self, s: &mut String, w: &mut Out, owner: u64) -> usize {
        for st in &self.styles {
            let r = &st.record;
            pair(s, 0, "DIMSTYLE");
            record(s, 105, w.handle(), owner, "AcDbDimStyleTableRecord", &st.name);
            pair(s, 3, "");
            pair(s, 4, "");
            pair(s, 40, 1);
            for (code, v) in [(41, r.tick_size), (42, r.witness_gap), (44, r.witness_extension), (140, r.text_height), (147, r.text_offset)]
            {
                if v > 0.0 {
                    pair(s, code, num(v));
                }
            }
            if let Some(places) = r.decimals {
                pair(s, 271, places);
            }
            // The number above the line, along it.
            pair(s, 77, 1);
            pair(s, 73, 0);
            pair(s, 74, 0);
        }
        self.styles.len()
    }
}

/// One segment as a `DIMENSION`: aligned, or rotated for a linear one, of the style `style`, holding `block` if it is the first
/// of its element's.
fn entity_of(s: &mut String, h: u64, layer: &str, color: Rgb, style: &str, block: Option<&str>, r: &DimensionRecord) {
    pair(s, 0, "DIMENSION");
    entity(s, h, MODEL_SPACE, layer, None, None, color);
    pair(s, 100, "AcDbDimension");
    if let Some(name) = block {
        pair(s, 2, name);
    }
    point3(s, 10, r.line);
    point3(s, 11, r.text);
    // Aligned is 1, rotated 0; 32 says the block is this dimension's alone.
    pair(s, 70, i32::from(r.linear.is_none()) | if block.is_some() { 32 } else { 0 });
    // The number is centred on its point.
    pair(s, 71, 5);
    // A space is how a CAD file says "no text of its own": the number is the TEXT beside.
    pair(s, 1, " ");
    pair(s, 42, num(r.measure));
    pair(s, 3, style);
    pair(s, 100, "AcDbAlignedDimension");
    point3(s, 13, r.from);
    point3(s, 14, r.to);
    if let Some(angle) = r.linear {
        pair(s, 50, num(angle.to_degrees().rem_euclid(360.0)));
        pair(s, 100, "AcDbRotatedDimension");
    }
}
