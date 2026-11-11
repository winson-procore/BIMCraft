//! The bridge driven as the apps drive it: DWG files made in the tests (a DXF written as DWG by the same library, since
//! Autodesk's own files are never committed) read back as DXF the core reads, and hostile files refused with a reason.

use bimcraft_core::dxf::{self, Shape};

use super::*;

/// Pairs as a DXF writes them: a code line, then its value.
fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c:>3}\n{v}\n")).collect()
}

/// A room 10 m by 6 m in metres (an AutoCAD 2004 file): its walls on WALLS (red), a column as a circle on COLS, a label.
pub(crate) fn room_dxf() -> String {
    let header = pairs(&[(9, "$ACADVER"), (1, "AC1018"), (9, "$INSUNITS"), (70, "6")]);
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER"), (70, "2")]);
    for (layer, color) in [("WALLS", "1"), ("COLS", "5")] {
        table += &pairs(&[
            (0, "LAYER"),
            (100, "AcDbSymbolTableRecord"),
            (100, "AcDbLayerTableRecord"),
            (2, layer),
            (70, "0"),
            (62, color),
            (6, "Continuous"),
        ]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    let line = |a: (&str, &str), b: (&str, &str)| {
        pairs(&[
            (0, "LINE"),
            (100, "AcDbEntity"),
            (8, "WALLS"),
            (100, "AcDbLine"),
            (10, a.0),
            (20, a.1),
            (30, "0"),
            (11, b.0),
            (21, b.1),
            (31, "0"),
        ])
    };
    let mut entities =
        line(("0", "0"), ("10", "0")) + &line(("10", "0"), ("10", "6")) + &line(("10", "6"), ("0", "6")) + &line(("0", "6"), ("0", "0"));
    entities +=
        &pairs(&[(0, "CIRCLE"), (100, "AcDbEntity"), (8, "COLS"), (100, "AcDbCircle"), (10, "5"), (20, "3"), (30, "0"), (40, "0.2")]);
    entities += &pairs(&[
        (0, "TEXT"),
        (100, "AcDbEntity"),
        (8, "WALLS"),
        (100, "AcDbText"),
        (10, "1"),
        (20, "1"),
        (30, "0"),
        (40, "0.3"),
        (1, "Sala"),
        (100, "AcDbText"),
    ]);
    let section = |name: &str, body: &str| format!("{}{body}{}", pairs(&[(0, "SECTION"), (2, name)]), pairs(&[(0, "ENDSEC")]));
    section("HEADER", &header) + &section("TABLES", &table) + &section("ENTITIES", &entities) + &pairs(&[(0, "EOF")])
}

fn room_dwg() -> Vec<u8> {
    dwg_from_dxf(&room_dxf()).unwrap()
}

#[test]
fn a_dwg_comes_back_as_dxf_the_core_reads_with_its_layers_shapes_and_unit() {
    let dwg = room_dwg();
    assert!(is_dwg(&dwg), "{:?}", dwg.get(..6));
    let read = to_dxf(&dwg, Limits::DEFAULT).unwrap();
    assert!(read.notes.is_empty(), "{:?}", read.notes);
    let d = dxf::read(&read.dxf).unwrap();
    assert_eq!(d.unit, "m");
    let lines: Vec<_> = d.entities.iter().filter(|e| matches!(e.shape, Shape::Line(..))).collect();
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|e| e.layer == "WALLS"));
    assert!(d.entities.iter().any(|e| e.layer == "COLS" && matches!(e.shape, Shape::Circle { .. })));
    assert!(d.entities.iter().any(|e| matches!(&e.shape, Shape::Text { text, .. } if text.contains("Sala"))));
    let walls = d.layers.iter().find(|l| l.name == "WALLS").unwrap();
    assert_eq!(walls.color, dxf::Color::Index(1));
}

#[test]
fn a_file_that_isnt_a_dwg_or_is_too_large_or_broken_says_why_and_never_panics() {
    assert!(to_dxf(b"", Limits::DEFAULT).unwrap_err().contains("isn't a DWG"));
    assert!(to_dxf(b"  0\nSECTION\n", Limits::DEFAULT).unwrap_err().contains("isn't a DWG"));
    let small = Limits { dwg_bytes: 10, ..Limits::DEFAULT };
    assert!(to_dxf(&room_dwg(), small).unwrap_err().contains("larger than"));
    let tight = Limits { dxf_bytes: 1000, ..Limits::DEFAULT };
    assert!(to_dxf(&room_dwg(), tight).unwrap_err().contains("more than"));
    // Its header and nothing more, or garbage after it.
    assert!(to_dxf(b"AC1032 this is not a dwg file at all", Limits::DEFAULT).is_err());
    let dwg = room_dwg();
    for cut in [7, 64, 256, dwg.len() / 2, dwg.len() - 1] {
        let _ = to_dxf(&dwg[..cut.min(dwg.len())], Limits::DEFAULT);
    }
}

#[test]
fn bytes_changed_anywhere_in_a_dwg_never_panic() {
    let dwg = room_dwg();
    let mut seed = 0x2545_f491_u32;
    for _ in 0..200 {
        let mut bad = dwg.clone();
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = 6 + seed as usize % (bad.len() - 6);
            bad[at] = (seed >> 8) as u8;
        }
        let _ = to_dxf(&bad, Limits::DEFAULT);
    }
}

/// The sample house's plan as the core writes it in DXF.
fn plan_dxf() -> String {
    let mut s = bimcraft_core::engine::Session::new();
    s.script(bimcraft_core::sample::SAMPLE).unwrap();
    let m = &s.model;
    let level = m.levels()[0].0;
    bimcraft_core::render::dxf(&bimcraft_core::render::plan(m, level, 1600, 1000), m)
}

fn per_layer(d: &dxf::Dxf) -> std::collections::BTreeMap<String, usize> {
    let mut n = std::collections::BTreeMap::new();
    for e in &d.entities {
        *n.entry(e.layer.clone()).or_insert(0) += 1;
    }
    n
}

#[test]
fn a_plan_written_as_dwg_2018_reads_back_with_its_layers_and_entities() {
    let text = plan_dxf();
    let dwg = to_dwg(&text, Limits::DEFAULT).unwrap();
    assert!(dwg.starts_with(b"AC1032") && is_dwg(&dwg), "{:?}", dwg.get(..6));
    let (before, after) = (dxf::read_text(&text).unwrap(), dxf::read(&to_dxf(&dwg, Limits::DEFAULT).unwrap().dxf).unwrap());
    assert_eq!(after.unit, "mm");
    assert_eq!(per_layer(&after), per_layer(&before));
    for layer in &before.layers {
        assert!(after.layer(&layer.name).is_some(), "{} was lost", layer.name);
    }
    // A dashed grid keeps its linetype.
    let dashed = |d: &dxf::Dxf| d.entities.iter().filter(|e| e.layer == "A-GRID" && e.linetype.is_some()).count();
    assert_eq!(dashed(&after), dashed(&before));
    // Through the test helper too.
    assert!(dwg_from_dxf(&text).unwrap().starts_with(b"AC1032"));
}

#[test]
fn a_plans_lettering_survives_as_text_and_mtext_in_the_dwg() {
    let before = dxf::read_text(&plan_dxf()).unwrap();
    let dwg = to_dwg(&plan_dxf(), Limits::DEFAULT).unwrap();
    let after = dxf::read(&to_dxf(&dwg, Limits::DEFAULT).unwrap().dxf).unwrap();
    let words = |d: &dxf::Dxf| {
        let mut w: Vec<String> = d
            .entities
            .iter()
            .filter(|e| e.layer == "A-ANNO-TEXT")
            .filter_map(|e| match &e.shape {
                Shape::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        w.sort();
        w
    };
    assert!(!words(&before).is_empty(), "the plan has no lettering to carry");
    assert_eq!(words(&after), words(&before));
}

#[test]
fn a_plans_dimensions_survive_as_dimension_entities_with_their_blocks_in_the_dwg() {
    let text = plan_dxf();
    let count = |t: &str| t.lines().filter(|l| l.trim() == "DIMENSION").count();
    let before = count(&text);
    assert!(before >= 3, "the plan has {before} dimensions to carry");
    let dwg = to_dwg(&text, Limits::DEFAULT).unwrap();
    let back = String::from_utf8_lossy(&to_dxf(&dwg, Limits::DEFAULT).unwrap().dxf).into_owned();
    assert_eq!(count(&back), before);
    // What they draw is still there: the core reads their blocks back as the same lines on the same layer.
    let (a, b) = (dxf::read_text(&text).unwrap(), dxf::read(back.as_bytes()).unwrap());
    assert_eq!(per_layer(&b).get("A-ANNO-DIMS"), per_layer(&a).get("A-ANNO-DIMS"));
}

#[test]
fn a_plans_doors_windows_and_furniture_survive_as_inserts_of_named_blocks_with_their_attributes_in_the_dwg() {
    let text = plan_dxf();
    let count = |t: &str, what: &str| t.lines().filter(|l| l.trim() == what).count();
    let (inserts, attribs) = (count(&text, "INSERT"), count(&text, "ATTRIB"));
    assert!(inserts >= 10, "the plan has {inserts} inserts to carry");
    assert_eq!(attribs, 2 * inserts);
    let dwg = to_dwg(&text, Limits::DEFAULT).unwrap();
    let back = String::from_utf8_lossy(&to_dxf(&dwg, Limits::DEFAULT).unwrap().dxf).into_owned();
    assert_eq!(count(&back, "INSERT"), inserts);
    assert_eq!(count(&back, "ATTRIB"), attribs);
    for tag in ["MARK", "TYPE"] {
        assert!(back.lines().any(|l| l.trim() == tag), "{tag} was lost");
    }
    let (a, b) = (dxf::read_text(&text).unwrap(), dxf::read(back.as_bytes()).unwrap());
    for layer in ["A-DOOR", "A-GLAZ"] {
        assert_eq!(per_layer(&b).get(layer), per_layer(&a).get(layer), "{layer}");
    }
}

#[test]
fn an_mtext_with_a_font_run_keeps_the_run_through_the_dwg() {
    let run = "{\\fArial|b0|i0|c0|p34;Oak floor}";
    let mut text = room_dxf();
    let at = text.rfind("  0\nENDSEC").unwrap();
    let mtext = pairs(&[
        (0, "MTEXT"),
        (100, "AcDbEntity"),
        (8, "WALLS"),
        (100, "AcDbMText"),
        (10, "1000"),
        (20, "1000"),
        (30, "0"),
        (40, "250"),
        (41, "0"),
        (71, "1"),
        (72, "1"),
        (1, run),
        (7, "Standard"),
        (11, "1"),
        (21, "0"),
        (31, "0"),
        (73, "1"),
        (44, "0.96"),
    ]);
    text.insert_str(at, &mtext);
    let dwg = to_dwg(&text, Limits::DEFAULT).unwrap();
    let back = String::from_utf8_lossy(&to_dxf(&dwg, Limits::DEFAULT).unwrap().dxf).into_owned();
    assert!(
        back.contains("Arial|b0|i0|c0|p34;Oak floor"),
        "{}",
        back.lines().skip_while(|l| l.trim() != "MTEXT").take(40).collect::<Vec<_>>().join("|")
    );
}

/// `plan_dxf` with paper-space ink and three viewports spliced in: what a sheet's export adds to a plan's file.
fn sheet_dxf() -> String {
    let plan = plan_dxf();
    let paper_line = |h: u32, a: (f64, f64), b: (f64, f64)| {
        pairs(&[
            (0, "LINE"),
            (5, &format!("{h:X}")),
            (330, "1B"),
            (100, "AcDbEntity"),
            (67, "1"),
            (8, "A-ANNO-TTLB"),
            (100, "AcDbLine"),
            (10, &a.0.to_string()),
            (20, &a.1.to_string()),
            (30, "0"),
            (11, &b.0.to_string()),
            (21, &b.1.to_string()),
            (31, "0"),
        ])
    };
    let viewport = |h: u32, at: (f64, f64), size: (f64, f64), scale: f64| {
        pairs(&[
            (0, "VIEWPORT"),
            (5, &format!("{h:X}")),
            (330, "1B"),
            (100, "AcDbEntity"),
            (67, "1"),
            (8, "G-ANNO-VPRT"),
            (100, "AcDbViewport"),
            (10, &at.0.to_string()),
            (20, &at.1.to_string()),
            (30, "0"),
            (40, &size.0.to_string()),
            (41, &size.1.to_string()),
            (68, "1"),
            (69, &format!("{}", h - 0x900F)),
            (12, "5000"),
            (22, "3000"),
            (16, "0"),
            (26, "0"),
            (36, "1"),
            (17, "0"),
            (27, "0"),
            (37, "0"),
            (42, "50"),
            (45, &(size.1 * scale).to_string()),
            (90, "32800"),
        ])
    };
    let mut paper = String::new();
    let corners =
        [((10.0, 10.0), (410.0, 10.0)), ((410.0, 10.0), (410.0, 287.0)), ((410.0, 287.0), (10.0, 287.0)), ((10.0, 287.0), (10.0, 10.0))];
    for (k, (a, b)) in corners.into_iter().enumerate() {
        paper += &paper_line(0x9000 + k as u32, a, b);
    }
    for (k, at) in [(100.0, 200.0), (250.0, 200.0), (300.0, 100.0)].into_iter().enumerate() {
        paper += &viewport(0x9010 + k as u32, (at.0, at.1), (120.0, 90.0), 100.0);
    }
    let end = plan.find("OBJECTS").unwrap();
    let at = plan[..end].rfind("  0\nENDSEC").unwrap();
    format!("{}{paper}{}", &plan[..at], &plan[at..])
}

/// How many entities of each kind the DXF `text` puts in paper space: those carrying 67 = 1.
fn paper_kinds(text: &str) -> std::collections::BTreeMap<String, usize> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut kinds = std::collections::BTreeMap::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i] == "0" {
            let kind = lines[i + 1];
            let mut j = i + 2;
            let mut paper = false;
            while j + 1 < lines.len() && lines[j] != "0" {
                paper |= lines[j] == "67" && lines[j + 1] == "1";
                j += 2;
            }
            if paper {
                *kinds.entry(kind.to_string()).or_insert(0) += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    kinds
}

#[test]
fn a_dxfs_paper_space_and_viewports_survive_the_dwg() {
    let text = sheet_dxf();
    let before = paper_kinds(&text);
    assert_eq!((before.get("LINE"), before.get("VIEWPORT")), (Some(&4), Some(&3)), "{before:?}");
    let dwg = to_dwg(&text, Limits::DEFAULT).unwrap();
    let back = String::from_utf8_lossy(&to_dxf(&dwg, Limits::DEFAULT).unwrap().dxf).into_owned();
    let after = paper_kinds(&back);
    assert_eq!((after.get("LINE"), after.get("VIEWPORT")), (Some(&4), Some(&3)), "{after:?}");
}

#[test]
fn a_dxf_too_large_or_that_isnt_one_or_a_dwg_too_large_is_refused_and_never_panics() {
    let text = plan_dxf();
    let e = to_dwg(&text, Limits { dxf_bytes: 1000, ..Limits::DEFAULT }).unwrap_err();
    assert!(e.contains("larger than"), "{e}");
    let e = to_dwg(&text, Limits { dwg_bytes: 1000, ..Limits::DEFAULT }).unwrap_err();
    assert!(e.contains("more than"), "{e}");
    for bad in ["", "not a drawing", "  0\nSECTION\n  2\nENTITIES\n  0\nLWPOLYLINE\n 90\n99999999\n", &text[..text.len() / 2]] {
        let _ = to_dwg(bad, Limits::DEFAULT);
    }
    assert!(to_dwg("not a drawing", Limits::DEFAULT).is_err());
}

#[test]
fn a_file_too_large_is_refused_before_it_is_read() {
    let dir = std::env::temp_dir().join(format!("bimcraft-dwg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.dwg");
    std::fs::write(&path, room_dwg()).unwrap();
    let e = read_file(&path, Limits { dwg_bytes: 100, ..Limits::DEFAULT }).unwrap_err();
    assert!(e.contains("larger than"), "{e}");
    assert!(read_file(&dir.join("nowhere.dwg"), Limits::DEFAULT).is_err());
}

#[test]
fn only_a_childs_arguments_make_one() {
    assert!(child_main(&[]).is_none());
    assert!(child_main(&["plan.dwg".into()]).is_none());
    assert!(child_main(&["--sample".into(), "x".into()]).is_none());
}

/// Real drawings, read where `BIMCRAFT_DWG_SAMPLES` names a folder of them (they're never committed): each comes back as DXF
/// the core reads, or says why, and none takes the process down.
#[test]
fn real_drawings_where_a_folder_of_them_is_named() {
    let Some(dir) = std::env::var_os("BIMCRAFT_DWG_SAMPLES") else { return };
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(Result::ok).map(|e| e.path()).collect();
    files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")));
    files.sort();
    for path in files {
        let start = std::time::Instant::now();
        let said = match read_file(&path, Limits::DEFAULT) {
            Ok(read) => match dxf::read(&read.dxf) {
                Ok(d) => format!(
                    "{} entities, unit {}, {} kB of DXF, left out {:?}, notes {:?}",
                    d.entities.len(),
                    d.unit,
                    read.dxf.len() >> 10,
                    d.left_out,
                    read.notes
                ),
                Err(e) => format!("DXF not read: {e}"),
            },
            Err(e) => format!("refused: {e}"),
        };
        eprintln!("{} ({} kB) in {:?}: {said}", path.display(), std::fs::metadata(&path).map_or(0, |m| m.len() >> 10), start.elapsed());
    }
}
