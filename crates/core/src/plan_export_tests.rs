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
    std::sync::Arc::new(move |dxf: &str| {
        *given.lock().unwrap() = dxf.to_string();
        Ok(format!("AC1032 {}", dxf.len()).into_bytes())
    })
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
    s.dwg_writer = Some(std::sync::Arc::new(|_: &str| Err("the library gave up".to_string())));
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
