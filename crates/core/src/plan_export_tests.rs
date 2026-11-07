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
