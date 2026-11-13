//! A plan saved as CAD through the commands every front end drives: `save` to .dxf writes the core's own DXF, of the level
//! `view` names.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::dxf;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A folder of this test's own.
fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-plan-export-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample() -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s
}

fn walls(d: &dxf::Dxf) -> usize {
    d.entities.iter().filter(|e| e.layer == "A-WALL").count()
}

#[test]
fn save_to_dxf_writes_the_lowest_levels_plan_or_the_one_view_names() {
    let mut s = sample();
    let path = dir("dxf").join("plan.dxf");
    let r = run(&mut s, "save", json!({"path": path}));
    assert_eq!(r, json!({"path": path, "format": "dxf"}));
    let first = dxf::read(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((first.version.as_str(), first.unit.as_str()), ("AC1032", "mm"));
    assert!(walls(&first) > 0 && first.layer("A-DOOR").is_some());
    // The project is still its own file's, not the drawing's.
    assert_eq!(s.path, None);
    let upper = dir("dxf").join("upper.dxf");
    run(&mut s, "save", json!({"path": upper, "view": "Level 2"}));
    let second = dxf::read(&std::fs::read(&upper).unwrap()).unwrap();
    assert_ne!(first.entities.len(), second.entities.len());
    let e = s.execute("save", &json!({"path": dir("dxf").join("x.dxf"), "view": "Level 9"})).unwrap_err();
    assert!(e.contains("Level 9"), "{e}");
}

/// A DWG writer as an app hands one: this one says what it was given and writes a header and its length.
fn fake_writer(given: std::sync::Arc<std::sync::Mutex<String>>) -> crate::cad::DwgWriter {
    std::sync::Arc::new(move |dxf: &str, _: crate::cad::DwgVersion| {
        *given.lock().unwrap() = dxf.to_string();
        Ok(format!("AC1032 {}", dxf.len()).into_bytes())
    })
}

#[test]
fn save_to_dwg_takes_a_version_and_refuses_any_other() {
    let mut s = sample();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = std::sync::Arc::clone(&seen);
    s.dwg_writer = Some(std::sync::Arc::new(move |_: &str, v: crate::cad::DwgVersion| {
        log.lock().unwrap().push(v);
        Ok(b"AC10".to_vec())
    }));
    let path = dir("version").join("plan.dwg");
    let plain = run(&mut s, "save", json!({"path": path}));
    let v2018 = run(&mut s, "save", json!({"path": path, "version": "2018"}));
    let v2000 = run(&mut s, "save", json!({"path": path, "version": " 2000 "}));
    use crate::cad::DwgVersion::{Dwg2000, Dwg2018};
    assert_eq!(*seen.lock().unwrap(), [Dwg2018, Dwg2018, Dwg2000]);
    assert_eq!(plain, v2018);
    assert!(plain.get("notes").is_none(), "{plain}");
    let notes = v2000["notes"].to_string();
    assert!(notes.contains("true colour") && notes.contains("colour index"), "{notes}");
    // Every other value is an error that names `version`, and nothing is written.
    let refused = dir("version").join("refused.dwg");
    for bad in [json!("2004"), json!("R2000"), json!("AC1015"), json!(""), json!(2000)] {
        let e = s.execute("save", &json!({"path": refused, "version": bad})).unwrap_err();
        assert!(e.contains("version"), "{bad}: {e}");
        assert!(!refused.exists());
    }
    assert_eq!(seen.lock().unwrap().len(), 3);
    // It is for a DWG only.
    let e = s.execute("save", &json!({"path": dir("version").join("plan.dxf"), "version": "2000"})).unwrap_err();
    assert!(e.contains("version") && e.contains(".dwg"), "{e}");
}

#[test]
fn save_to_dwg_hands_the_plans_dxf_to_the_sessions_writer() {
    let mut s = sample();
    let given = std::sync::Arc::default();
    s.dwg_writer = Some(fake_writer(std::sync::Arc::clone(&given)));
    let path = dir("dwg").join("plan.dwg");
    assert_eq!(run(&mut s, "save", json!({"path": path})), json!({"path": path, "format": "dwg"}));
    let dxf_text = given.lock().unwrap().clone();
    let read = dxf::read_text(&dxf_text).unwrap();
    assert!(walls(&read) > 0);
    assert_eq!(std::fs::read(&path).unwrap(), format!("AC1032 {}", dxf_text.len()).into_bytes());
    // The same plan as .dxf gives.
    let as_dxf = dir("dwg").join("plan.dxf");
    run(&mut s, "save", json!({"path": as_dxf}));
    assert_eq!(std::fs::read_to_string(&as_dxf).unwrap(), dxf_text);
    // A writer that fails says why, and nothing is written.
    s.dwg_writer = Some(std::sync::Arc::new(|_: &str, _: crate::cad::DwgVersion| Err("the library gave up".to_string())));
    let failed = dir("dwg").join("failed.dwg");
    let e = s.execute("save", &json!({"path": failed})).unwrap_err();
    assert!(e.contains("the library gave up"), "{e}");
    assert!(!failed.exists());
    assert_eq!(s.path, None);
}

#[test]
fn without_a_dwg_writer_save_says_to_save_as_dxf() {
    let mut s = sample();
    let path = dir("no-writer").join("plan.dwg");
    let e = s.execute("save", &json!({"path": path})).unwrap_err();
    assert!(e.contains("this build can't write DWG; save as .dxf"), "{e}");
    assert!(!path.exists());
    // render to a .dwg path takes the same writer, and says the same without one.
    let e = s.execute("render", &json!({"path": path})).unwrap_err();
    assert!(e.contains("save as .dxf"), "{e}");
}

#[test]
fn a_drawing_property_is_kept_by_the_project_and_written_in_the_dxfs_header() {
    let mut s = sample();
    assert_eq!(run(&mut s, "drawing_property", json!({})), json!({"properties": {}}));
    let r = run(&mut s, "drawing_property", json!({"name": "building_type", "value": "residential"}));
    assert_eq!(r, json!({"properties": {"building_type": "residential"}}));
    run(&mut s, "drawing_property", json!({"name": "client", "value": "Ada & Co."}));
    let path = dir("props").join("plan.dxf");
    run(&mut s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let header = &text[..text.find("ENDSEC").unwrap()];
    assert!(header.contains("  9\n$CUSTOMPROPERTYTAG\n  1\nbuilding_type\n  9\n$CUSTOMPROPERTY\n  1\nresidential\n"), "{header}");
    assert!(header.contains("  9\n$CUSTOMPROPERTYTAG\n  1\nclient\n  9\n$CUSTOMPROPERTY\n  1\nAda & Co.\n"), "{header}");
    // The core still reads its own DXF.
    assert!(walls(&dxf::read(text.as_bytes()).unwrap()) > 0);

    // The project's file keeps them, and a value is changed or taken away by name.
    let file = dir("props").join("house.bimcraft");
    run(&mut s, "save", json!({"path": file}));
    let mut again = Session::new();
    run(&mut again, "open", json!({"path": file}));
    assert_eq!(
        run(&mut again, "drawing_property", json!({})),
        json!({"properties": {"building_type": "residential", "client": "Ada & Co."}})
    );
    run(&mut again, "drawing_property", json!({"name": "building_type", "value": "mixed-use"}));
    let r = run(&mut again, "drawing_property", json!({"name": "client", "delete": true}));
    assert_eq!(r, json!({"properties": {"building_type": "mixed-use"}}));
    run(&mut again, "undo", json!({}));
    assert_eq!(again.model.drawing_properties.len(), 2);

    // A project with none writes none.
    let plain = dir("props").join("plain.dxf");
    run(&mut sample(), "save", json!({"path": plain}));
    assert!(!std::fs::read_to_string(&plain).unwrap().contains("$CUSTOMPROPERTY"));
}

#[test]
fn a_drawing_property_a_drawing_cannot_hold_is_refused_and_changes_nothing() {
    let mut s = sample();
    run(&mut s, "drawing_property", json!({"name": "building_type", "value": "office"}));
    let long = "x".repeat(crate::model::MAX_TEXT + 1);
    for (p, says) in [
        (json!({"name": "", "value": "x"}), "`name`"),
        (json!({"name": "  ", "value": "x"}), "`name`"),
        (json!({"name": " padded", "value": "x"}), "`name`"),
        (json!({"name": "two\nlines", "value": "x"}), "`name`"),
        (json!({"name": long, "value": "x"}), "`name`"),
        (json!({"name": "a", "value": "two\nlines"}), "`value`"),
        (json!({"name": "a", "value": ""}), "`value`"),
        (json!({"name": "a", "value": 3}), "`value`"),
        (json!({"name": "a", "value": long}), "`value`"),
        (json!({"name": "a"}), "`value`"),
        (json!({"value": "x"}), "`name`"),
        (json!({"name": "absent", "delete": true}), "absent"),
        (json!({"name": "building_type", "value": "x", "delete": true}), "`delete`"),
        (json!({"name": "Building_Type", "value": "x"}), "building_type"),
        (json!({"name": "a", "valu": "x"}), "valu"),
    ] {
        let e = s.execute("drawing_property", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(run(&mut s, "drawing_property", json!({})), json!({"properties": {"building_type": "office"}}));
    for i in 1..crate::model::MAX_DRAWING_PROPERTIES {
        run(&mut s, "drawing_property", json!({"name": format!("p{i}"), "value": "v"}));
    }
    let e = s.execute("drawing_property", &json!({"name": "one too many", "value": "v"})).unwrap_err();
    assert!(e.contains(&crate::model::MAX_DRAWING_PROPERTIES.to_string()), "{e}");
}
