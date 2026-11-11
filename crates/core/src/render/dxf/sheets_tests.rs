//! A sheet written as DXF, read back with a plain group-code reader that finds paper space for itself: the sheet's own ink in
//! `*Paper_Space` at paper millimetres, a `VIEWPORT` for each view placed on it, the views' drawings behind them in model space,
//! and the title block an `INSERT` of a block that letters its number and name.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

/// One record of the file: its section, kind and group codes in order.
struct Rec {
    section: String,
    kind: String,
    codes: Vec<(String, String)>,
}

impl Rec {
    fn one(&self, code: &str) -> Option<&str> {
        self.codes.iter().find(|(k, _)| k == code).map(|(_, v)| v.as_str())
    }
    fn nums(&self, code: &str) -> Vec<f64> {
        self.codes.iter().filter(|(k, _)| k == code).filter_map(|(_, v)| v.parse().ok()).collect()
    }
    fn num(&self, code: &str) -> Option<f64> {
        self.nums(code).first().copied()
    }
}

fn records(text: &str) -> Vec<Rec> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
    let (mut out, mut section, mut i) = (Vec::new(), String::new(), 0);
    while i < pairs.len() {
        let (code, value) = pairs[i];
        if code == "0" && value == "SECTION" {
            section = pairs.get(i + 1).map(|p| p.1.to_string()).unwrap_or_default();
            i += 2;
        } else if code == "0" && value == "ENDSEC" {
            section.clear();
            i += 1;
        } else if code == "0" {
            let mut j = i + 1;
            let mut codes = Vec::new();
            while j < pairs.len() && pairs[j].0 != "0" {
                codes.push((pairs[j].0.to_string(), pairs[j].1.to_string()));
                j += 1;
            }
            out.push(Rec { section: section.clone(), kind: value.to_string(), codes });
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// What a file holds, by space.
struct Sheet {
    paper: Vec<Rec>,
    model: Vec<Rec>,
    /// Each block's name, with the strings its body letters and how many entities it holds.
    blocks: BTreeMap<String, (usize, Vec<String>)>,
    block_names: Vec<String>,
}

fn read(text: &str) -> Sheet {
    let all = records(text);
    let paper_handles: Vec<String> = all
        .iter()
        .filter(|r| r.section == "TABLES" && r.kind == "BLOCK_RECORD" && r.one("2").is_some_and(|n| n.eq_ignore_ascii_case("*Paper_Space")))
        .filter_map(|r| r.one("5").map(str::to_uppercase))
        .collect();
    let (mut paper, mut model, mut blocks, mut names, mut block) = (Vec::new(), Vec::new(), BTreeMap::new(), Vec::new(), None::<String>);
    for r in all {
        match (r.section.as_str(), r.kind.as_str()) {
            ("BLOCKS", "BLOCK") => {
                let name = r.one("2").unwrap_or_default().to_string();
                names.push(name.clone());
                blocks.entry(name.clone()).or_insert((0, Vec::new()));
                block = Some(name);
            }
            ("BLOCKS", "ENDBLK") => block = None,
            ("BLOCKS", _) => {
                if let Some(entry) = block.as_ref().and_then(|b| blocks.get_mut(b)) {
                    entry.0 += 1;
                    if let Some(t) = r.one("1").filter(|t| !t.is_empty()) {
                        entry.1.push(t.to_string());
                    }
                }
            }
            ("ENTITIES", "SEQEND") => {}
            ("ENTITIES", _) => {
                let in_paper = r.one("67") == Some("1") || r.one("330").is_some_and(|h| paper_handles.contains(&h.to_uppercase()));
                if in_paper { paper.push(r) } else { model.push(r) }
            }
            _ => {}
        }
    }
    Sheet { paper, model, blocks, block_names: names }
}

impl Sheet {
    fn kinds(recs: &[Rec]) -> BTreeMap<&str, usize> {
        let mut n = BTreeMap::new();
        for r in recs {
            *n.entry(r.kind.as_str()).or_insert(0) += 1;
        }
        n
    }
    fn viewports(&self) -> Vec<&Rec> {
        self.paper.iter().filter(|r| r.kind == "VIEWPORT").collect()
    }
    /// The box round what paper space holds, a viewport's window included: x0, y0, x1, y1.
    fn extent(recs: &[Rec]) -> Option<[f64; 4]> {
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for r in recs {
            xs.extend(r.nums("10"));
            ys.extend(r.nums("20"));
            if r.kind == "VIEWPORT" {
                let (w, h) = (r.num("40").unwrap_or(0.0), r.num("41").unwrap_or(0.0));
                let (x, y) = (r.num("10").unwrap_or(0.0), r.num("20").unwrap_or(0.0));
                xs.extend([x - w / 2.0, x + w / 2.0]);
                ys.extend([y - h / 2.0, y + h / 2.0]);
            }
        }
        let fold = |v: &[f64], f: fn(f64, f64) -> f64, init: f64| v.iter().copied().fold(init, f);
        (!xs.is_empty()).then(|| {
            [fold(&xs, f64::min, f64::MAX), fold(&ys, f64::min, f64::MAX), fold(&xs, f64::max, f64::MIN), fold(&ys, f64::max, f64::MIN)]
        })
    }
    /// Every string the file letters, loose or in a block's body or an insert's attributes.
    fn lettered(&self) -> Vec<String> {
        let loose = self.paper.iter().chain(&self.model).filter(|r| matches!(r.kind.as_str(), "TEXT" | "MTEXT" | "ATTRIB"));
        let mut all: Vec<String> = loose.filter_map(|r| r.one("1").map(str::to_string)).collect();
        all.extend(self.blocks.values().flat_map(|(_, t)| t.clone()));
        all
    }
    fn loose_paper_text(&self) -> Vec<String> {
        let texts = self.paper.iter().filter(|r| matches!(r.kind.as_str(), "TEXT" | "MTEXT"));
        texts.filter_map(|r| r.one("1").map(str::to_string)).collect()
    }
}

fn session() -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s
}

/// `view` saved as a DXF in `dir`: the reply and the file's text.
fn saved(s: &mut Session, dir: &std::path::Path, how: &str, view: &str) -> (Value, String) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{how}-{view}.dxf"));
    let reply = s.execute(how, &json!({"path": path, "view": view})).unwrap_or_else(|e| panic!("{how} {view}: {e:?}"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{how} {view} wrote no file: {e}"));
    (reply, text)
}

fn scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-sheets-{name}-{}", std::process::id()))
}

/// The viewports of the sheet numbered `number`: where each is on the paper, and its scale.
fn placed(s: &Session, number: &str) -> Vec<([f64; 2], f64)> {
    let guid = s.model.elements.iter().find_map(|(id, e)| match e {
        Element::Sheet(sh) if sh.number == number => Some(s.model.guid(*id)),
        _ => None,
    });
    let guid = guid.unwrap();
    let mut on: Vec<([f64; 2], f64)> = s
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Viewport(v) if v.sheet == guid => Some(([v.at.0, v.at.1], v.scale)),
            _ => None,
        })
        .collect();
    on.sort_by(|a, b| a.0[0].total_cmp(&b.0[0]));
    on
}

#[test]
fn a_sheet_writes_paper_space_at_the_sheets_millimetres() {
    let mut s = session();
    let dir = scratch("paper");
    let (_, text) = saved(&mut s, &dir, "save", "A102");
    let f = read(&text);
    assert!(!f.paper.is_empty(), "paper space holds nothing: model space {:?}", Sheet::kinds(&f.model));
    assert!(f.paper.iter().all(|r| r.one("330").is_some_and(|h| h.eq_ignore_ascii_case("1B"))), "{:?}", Sheet::kinds(&f.paper));
    // A3, landscape: 420 x 297.
    let [x0, y0, x1, y1] = Sheet::extent(&f.paper).unwrap();
    assert!(x0 >= -15.0 && y0 >= -15.0 && x1 <= 435.0 && y1 <= 312.0, "[{x0}, {x1}] x [{y0}, {y1}]");
    assert!(x1 - x0 >= 210.0 && y1 - y0 >= 148.5, "[{x0}, {x1}] x [{y0}, {y1}]");
    assert!(crate::dxf::read_text(&text).is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn each_view_placed_on_the_sheet_is_a_viewport_at_its_scale() {
    let mut s = session();
    let dir = scratch("viewports");
    let (_, text) = saved(&mut s, &dir, "save", "A102");
    let f = read(&text);
    let want = placed(&s, "A102");
    assert_eq!(want.len(), 3);
    let mut vps = f.viewports();
    assert_eq!(vps.len(), 3, "{:?}", Sheet::kinds(&f.paper));
    vps.sort_by(|a, b| a.num("10").unwrap_or(0.0).total_cmp(&b.num("10").unwrap_or(0.0)));
    for (([x, y], scale), vp) in want.iter().zip(vps) {
        let (at, size) = ((vp.num("10").unwrap(), vp.num("20").unwrap()), (vp.num("40").unwrap(), vp.num("41").unwrap()));
        assert!((at.0 - x).abs() <= 10.0 && (at.1 - y).abs() <= 10.0, "at {at:?}, the model says {x},{y}");
        assert!(size.0 > 0.0 && size.1 > 0.0, "{size:?}");
        let shown = vp.num("45").unwrap();
        assert!(((shown / size.1) / scale - 1.0).abs() < 0.01, "shows {shown} mm over {} mm of paper, placed at 1:{scale}", size.1);
        assert_eq!(vp.one("100"), Some("AcDbEntity"));
        assert_eq!(vp.one("67"), Some("1"));
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_views_are_drawn_in_model_space_behind_their_viewports_each_in_a_place_of_its_own() {
    let mut s = session();
    let dir = scratch("model");
    let (_, text) = saved(&mut s, &dir, "save", "A102");
    let f = read(&text);
    let [x0, y0, x1, y1] = Sheet::extent(&f.model).unwrap();
    assert!((x1 - x0).max(y1 - y0) >= 2000.0, "model space spans [{x0}, {x1}] x [{y0}, {y1}]");
    assert!(f.model.len() > 20, "{:?}", Sheet::kinds(&f.model));
    // Each viewport looks at a window of its own: no two windows overlap, so none shows another's drawing.
    let windows: Vec<[f64; 4]> = f
        .viewports()
        .iter()
        .map(|v| {
            let (cx, cy) = (v.num("12").unwrap(), v.num("22").unwrap());
            let (h, w) = (v.num("45").unwrap(), v.num("45").unwrap() * v.num("40").unwrap() / v.num("41").unwrap());
            [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
        })
        .collect();
    for (i, a) in windows.iter().enumerate() {
        for b in &windows[i + 1..] {
            assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1], "{a:?} overlaps {b:?}");
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_title_block_is_an_insert_of_a_block_that_letters_the_number_and_name() {
    let mut s = session();
    let dir = scratch("title");
    let (_, text) = saved(&mut s, &dir, "save", "A102");
    let f = read(&text);
    let inserts: Vec<&Rec> = f.paper.iter().filter(|r| r.kind == "INSERT").collect();
    assert_eq!(inserts.len(), 1, "{:?}", Sheet::kinds(&f.paper));
    let name = inserts[0].one("2").unwrap();
    let (count, _) = f.blocks.get(name).unwrap_or_else(|| panic!("block {name} is not defined"));
    assert!(*count >= 4, "{name} holds {count} entities");
    let (_, words) = &f.blocks[name];
    for want in ["A102", "Section and Elevations"] {
        assert!(words.iter().any(|w| w == want), "{want} is not lettered in the block: {words:?}");
    }
    // No attribute definitions to draw nothing, and no attributes for libredwg to file in the wrong space.
    assert!(f.paper.iter().all(|r| r.kind != "ATTRIB"), "{:?}", Sheet::kinds(&f.paper));
    // And not lettered twice, as loose text.
    assert!(f.loose_paper_text().iter().all(|t| !t.contains("A102")), "{:?}", f.loose_paper_text());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_sheet_is_not_the_level_plan() {
    let mut s = session();
    let dir = scratch("plan");
    let (reply, plan) = saved(&mut s, &dir, "save", "Level 1");
    let (p, sheet) = (read(&plan), read(&saved(&mut s, &dir, "save", "A102").1));
    assert!(p.paper.is_empty(), "{:?}", Sheet::kinds(&p.paper));
    assert!(p.lettered().iter().all(|t| !t.contains("A102")));
    assert!(sheet.lettered().iter().any(|t| t == "A102"));
    assert!(reply["kind"].as_str().is_none_or(|k| !k.contains("sheet")), "{reply}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn another_sheet_has_its_own_viewports_and_its_own_title_block() {
    let mut s = session();
    let dir = scratch("second");
    let (_, text) = saved(&mut s, &dir, "save", "A101");
    let f = read(&text);
    // The plan and the room schedule.
    assert_eq!(f.viewports().len(), 2, "{:?}", Sheet::kinds(&f.paper));
    assert_eq!(placed(&s, "A101").len(), 2);
    let values = f.lettered();
    assert!(values.iter().any(|t| t == "A101") && values.iter().any(|t| t == "Ground Floor"), "{values:?}");
    assert!(f.lettered().iter().all(|t| !t.contains("A102")), "{:?}", f.lettered());
    // The table is lettered in model space, behind its viewport, and the plan's rooms with it.
    let words = f.lettered();
    assert!(words.iter().any(|t| t == "Living"), "{words:?}");
    let [_, _, x1, _] = Sheet::extent(&f.paper).unwrap();
    assert!(x1 <= 435.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn what_the_views_share_is_named_once_in_the_file() {
    let mut s = session();
    let dir = scratch("names");
    for sheet in ["A101", "A102"] {
        let f = read(&saved(&mut s, &dir, "save", sheet).1);
        let mut seen: Vec<String> = f.block_names.iter().map(|n| n.to_uppercase()).collect();
        seen.sort();
        let all = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), all, "{sheet}: a block is defined twice: {:?}", f.block_names);
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// What `save` and `render` answer for a sheet as DXF: a file, no picture, the pictures it left out owned up to.
#[test]
fn a_sheet_saved_as_dxf_says_what_it_is_and_owns_up_to_the_pictures_it_left_out() {
    let mut s = session();
    let dir = scratch("reply");
    for how in ["save", "render"] {
        let (reply, text) = saved(&mut s, &dir, how, "A102");
        assert!(reply.get("pngBase64").is_none(), "{how}: {reply}");
        assert!(reply.get("format").is_none_or(|f| f == "dxf"), "{how}: {reply}");
        assert_eq!(reply["view"], "A102", "{how}: {reply}");
        assert!(reply["kind"].as_str().is_some_and(|k| k.contains("sheet")), "{how}: {reply}");
        assert!(reply["width"].as_u64().unwrap_or(0) > 0 && reply["height"].as_u64().unwrap_or(0) > 0, "{how}: {reply}");
        let notes = reply["notes"].as_array().unwrap_or_else(|| panic!("{how}: no notes in {reply}"));
        assert!(notes.iter().any(|n| n.as_str().is_some_and(|n| n.contains("picture"))), "{how}: {notes:?}");
        assert!(text.contains("VIEWPORT"));
    }
    // A sheet by its name, and one that does not exist.
    let (reply, _) = saved(&mut s, &dir, "save", "Ground Floor");
    assert_eq!(reply["view"], "A101");
    assert!(s.execute("save", &json!({"path": dir.join("x.dxf"), "view": "A999"})).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// What the PNG of a sheet shows is not changed by the CAD path, and a sheet is still no DWG without a writer.
#[test]
fn a_sheet_still_makes_its_png_and_needs_a_writer_for_a_dwg() {
    let mut s = session();
    let dir = scratch("png");
    std::fs::create_dir_all(&dir).unwrap();
    let r = s.execute("render", &json!({"view": "A102", "path": dir.join("a.png")})).unwrap();
    assert_eq!(r["kind"], "sheet");
    assert!(std::fs::read(dir.join("a.png")).unwrap().starts_with(b"\x89PNG"));
    let e = s.execute("save", &json!({"path": dir.join("a.dwg"), "view": "A102"}));
    assert!(e.is_err_and(|e| format!("{e:?}").contains("DWG")));
    std::fs::remove_dir_all(&dir).ok();
}
