//! A sheet as paper space. The paper holds the title block (an `INSERT` of a block of the border and the title box's rules, with its
//! project, name, scale and number lettered in it), what is drawn on the paper itself, and a `VIEWPORT` for each view
//! placed on it, at the viewport's place and size on the paper and the view's scale. Each view is drawn once in model space, as its
//! own view draws it, in a place of its own (a plan, a section and a schedule share no coordinates, so no two windows overlap);
//! the viewport's window on it is the view's frame.

use super::*;
use crate::sheet::{Export, Field, Placed};

/// Model mm kept clear between two views' windows.
const GAP: f64 = 200.0;

/// The most viewports written.
const MAX_VIEWPORTS: usize = 1_000;

/// The title block's block: its rules in paper mm and its lettering, in the block itself and not as attributes. libredwg files the
/// `ATTRIB`s acadrust writes under model space (see `crates/dwg/CLAUDE.md`), and an attribute definition with no attribute drawn is
/// invisible in a CAD program, so each sheet has a block of its own with its words in it.
pub(super) struct TitleBlock {
    name: String,
    record: u64,
    begin: u64,
    end: u64,
    body: String,
    texts: Vec<u64>,
    fields: Vec<(&'static str, String, P2, f64)>,
}

impl TitleBlock {
    /// The block `e`'s title block is: handles and the name come from `sh`; its rules go on layers of `sh`.
    fn of(e: &Export, m: &Model, sh: &mut Shared) -> TitleBlock {
        let name = safe("BIM-TITLE-BLOCK", "BIM-TITLE-BLOCK", &sh.names.blocks);
        sh.names.blocks.insert(name.to_uppercase());
        let record = sh.w.handle();
        let (begin, end) = (sh.w.handle(), sh.w.handle());
        let texts = e.fields.iter().map(|_| sh.w.handle()).collect();
        let mut body = String::new();
        let rules = &e.title;
        for (prim, owner, part) in rules.prims.iter().zip(&rules.owners).zip(&rules.parts).map(|((p, o), part)| (p, o, part)) {
            let Some(ink) = ink(rules, prim, *part, layer(m, *owner)) else { continue };
            sh.layers.insert(ink.layer.clone());
            put(&mut body, sh.w.handle(), record, &ink);
        }
        sh.layers.insert(TITLE_LAYER.to_string());
        let fields = e.fields.iter().map(|f: &Field| (f.tag, f.text.clone(), f.at, f.height)).collect();
        TitleBlock { name, record, begin, end, body, texts, fields }
    }

    pub fn block_record(&self, s: &mut String, owner: u64) {
        pair(s, 0, "BLOCK_RECORD");
        record(s, 5, self.record, owner, "AcDbBlockTableRecord", &self.name);
    }

    pub fn block_section(&self, s: &mut String) {
        pair(s, 0, "BLOCK");
        hex(s, 5, self.begin);
        hex(s, 330, self.record);
        pair(s, 100, "AcDbEntity");
        pair(s, 8, "0");
        pair(s, 100, "AcDbBlockBegin");
        pair(s, 2, &self.name);
        pair(s, 70, 0);
        point3(s, 10, P2(0.0, 0.0));
        pair(s, 3, &self.name);
        pair(s, 1, "");
        s.push_str(&self.body);
        for ((_, text, at, height), h) in self.fields.iter().zip(&self.texts) {
            pair(s, 0, "TEXT");
            hex(s, 5, *h);
            hex(s, 330, self.record);
            pair(s, 100, "AcDbEntity");
            pair(s, 8, TITLE_LAYER);
            pair(s, 100, "AcDbText");
            point3(s, 10, *at);
            pair(s, 40, num(*height));
            pair(s, 1, value(text));
            pair(s, 100, "AcDbText");
        }
        pair(s, 0, "ENDBLK");
        hex(s, 5, self.end);
        hex(s, 330, self.record);
        pair(s, 100, "AcDbEntity");
        pair(s, 8, "0");
        pair(s, 100, "AcDbBlockEnd");
    }

    /// Its `INSERT` in paper space, at the paper's origin.
    fn insert(&self, s: &mut String, w: &mut Out) {
        pair(s, 0, "INSERT");
        owned(s, w.handle(), PAPER_SPACE);
        pair(s, 8, TITLE_LAYER);
        pair(s, 100, "AcDbBlockReference");
        pair(s, 2, &self.name);
        point3(s, 10, P2(0.0, 0.0));
        for code in [41, 42, 43] {
            pair(s, code, 1);
        }
        pair(s, 50, 0);
    }
}

/// An attribute's text: ASCII-safe, and short enough for a reader.
fn value(text: &str) -> String {
    escaped(text, false).chars().take(200).collect()
}

/// The viewport `n` (numbered from 1), through which paper point `at` sees what model point `window` is the middle of: `size` mm of
/// paper, a view `size × scale` mm tall.
fn viewport(s: &mut String, h: u64, n: usize, at: P2, size: P2, window: P2, scale: f64) {
    pair(s, 0, "VIEWPORT");
    owned(s, h, PAPER_SPACE);
    pair(s, 8, VIEWPORT_LAYER);
    pair(s, 100, "AcDbViewport");
    point3(s, 10, at);
    pair(s, 40, num(size.0));
    pair(s, 41, num(size.1));
    pair(s, 68, 1);
    pair(s, 69, n + 1);
    point2(s, 12, window);
    point3(s, 16, P2(0.0, 0.0));
    pair(s, 36, 1);
    point3(s, 17, P2(0.0, 0.0));
    pair(s, 42, 50);
    pair(s, 45, num(size.1 * scale));
    pair(s, 90, 32800);
}

/// What a view's model-space window is, mm: its middle and size, after the drawing's moved.
fn window(d: &Drawing) -> Option<(P2, P2)> {
    let v = d.view;
    let ok = v.scale.is_finite() && v.scale > 0.0 && v.cx.is_finite() && v.cy.is_finite();
    ok.then(|| (P2(v.cx, v.cy), P2(f64::from(v.width) / v.scale, f64::from(v.height) / v.scale)))
}

/// Sheet `e` of `m` as a DXF file's text: paper space as it says, the views behind it in model space.
pub(super) fn write(mut e: Export, m: &Model) -> String {
    let mut doc = Doc::default();
    let title = TitleBlock::of(&e, m, &mut doc.shared);
    title.insert(&mut doc.ents, &mut doc.shared.w);
    doc.title = Some(title);
    let paper = e.paper;
    let mut ext = grow(None, &[P2(0.0, 0.0), paper]);
    doc.add(&e.page, m, PAPER_SPACE, P2(0.0, 0.0));
    let mut right: Option<f64> = None;
    let mut viewports = String::new();
    for (n, placed) in e.views.iter_mut().take(MAX_VIEWPORTS).enumerate() {
        let Placed { drawing, at, size, scale, .. } = placed;
        let (Some((middle, extent)), true) = (
            window(drawing),
            at.0.is_finite() && at.1.is_finite() && size.0 > 0.0 && size.1 > 0.0 && size.0.is_finite() && size.1.is_finite(),
        ) else {
            continue;
        };
        // Beside the last, `GAP` clear, and level with where it was drawn.
        let shift = right.map_or(0.0, |r| r + GAP - (middle.0 - extent.0 / 2.0));
        let by = P2(shift, 0.0);
        translate(drawing, by);
        doc.add(drawing, m, MODEL_SPACE, by);
        right = Some(middle.0 + shift + extent.0 / 2.0);
        let h = doc.shared.w.handle();
        viewport(&mut viewports, h, n, *at, *size, P2(middle.0 + shift, middle.1), *scale);
        ext = grow(ext, &[P2(at.0 - size.0 / 2.0, at.1 - size.1 / 2.0), P2(at.0 + size.0 / 2.0, at.1 + size.1 / 2.0)]);
        doc.shared.layers.insert(VIEWPORT_LAYER.to_string());
    }
    doc.ents.push_str(&viewports);
    doc.paper_ext = crate::dxf::union(doc.paper_ext.into_iter().chain(ext));
    doc.finish(m)
}
