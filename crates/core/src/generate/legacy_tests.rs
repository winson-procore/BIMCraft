//! The corridor archetypes' files keep their bytes, and the plan handed to lift, openings, script and report does not read
//! the corridor's fields. The hashes were taken before the house layout was added (Slice 12).

use super::*;

/// (name, archetype, seed, unit, value, fnv1a64 of `script.bimcraft.txt`), as `done/fixtures/house-layout-baseline` lists them.
const SCRIPTS: [(&str, &str, u64, &str, f64, &str); 20] = [
    ("office-sqft-s1", "office", 1, "sqft", 10000.0, "b71a2eb837d0e1e4"),
    ("office-sqft-s3", "office", 3, "sqft", 10000.0, "f87d08ce9a7d7586"),
    ("office-sqft-s4", "office", 4, "sqft", 10000.0, "f9a5ff7ce7b0be42"),
    ("office-sqft-s7", "office", 7, "sqft", 10000.0, "b8c30350837fa63a"),
    ("office-sqft-s10", "office", 10, "sqft", 10000.0, "5f5557fc7e0a8761"),
    ("clinic-sqft-s1", "clinic", 1, "sqft", 7000.0, "d01385a1c9bde867"),
    ("clinic-sqft-s3", "clinic", 3, "sqft", 7000.0, "e77b1dc2ea9737fb"),
    ("clinic-sqft-s4", "clinic", 4, "sqft", 7000.0, "9f2e44d78c5ce3df"),
    ("clinic-sqft-s7", "clinic", 7, "sqft", 7000.0, "a5fe063b5ac7a3ce"),
    ("clinic-sqft-s10", "clinic", 10, "sqft", 7000.0, "5588d09323ebf92c"),
    ("hospital-sqft-s1", "hospital", 1, "sqft", 20000.0, "fbd47c13faa17bfa"),
    ("hospital-sqft-s3", "hospital", 3, "sqft", 20000.0, "6fcbafff7c9d2be1"),
    ("hospital-sqft-s4", "hospital", 4, "sqft", 20000.0, "a9161a4e5b4b3105"),
    ("hospital-sqft-s7", "hospital", 7, "sqft", 20000.0, "d0617757b11cc066"),
    ("hospital-sqft-s10", "hospital", 10, "sqft", 20000.0, "1f2d0228f632279a"),
    ("clinic-sqft-s9", "clinic", 9, "sqft", 7000.0, "f35cf7fa8332ad7b"),
    ("hospital-sqft-s8", "hospital", 8, "sqft", 20000.0, "7100e81972196c65"),
    ("office-m2-s1", "office", 1, "m2", 900.0, "b68145feb42e455d"),
    ("clinic-m2-s3", "clinic", 3, "m2", 650.0, "e77b1dc2ea9737fb"),
    ("hospital-m2-s10", "hospital", 10, "m2", 1800.0, "31b46ca00601d64b"),
];

fn built(archetype: &str, seed: u64, unit: &str, value: f64) -> Built {
    let area = if unit == "sqft" { AreaRequest::sqft(value) } else { AreaRequest::m2(value) };
    build_with_area(Archetype::parse(archetype).unwrap(), seed, area).unwrap()
}

fn file<'a>(b: &'a Built, name: &str) -> &'a str {
    &b.files.iter().find(|(n, _)| *n == name).unwrap().1
}

#[test]
fn legacy_scripts_keep_their_bytes() {
    let mut wrong = Vec::new();
    for (name, archetype, seed, unit, value, hash) in SCRIPTS {
        let got = fnv1a64(file(&built(archetype, seed, unit, value), "script.bimcraft.txt"));
        if got != hash {
            wrong.push(format!("(\"{name}\", {got})"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// (archetype, seed, m2, then the hashes of report, topology and building files).
const FILES: [(&str, u64, f64, [&str; 3]); 3] = [
    ("office", 1, 900.0, ["e807be8c7a5dd5f1", "e861b63f8db2b67e", "b1793dbd44590210"]),
    ("clinic", 3, 650.0, ["dfdc45be244d1061", "c7a1c5c1faae85a9", "5b077d2a95d251e9"]),
    ("hospital", 10, 1800.0, ["2f487a67a4e68736", "9acc063d0ce3bad5", "c946d90baf919830"]),
];

#[test]
fn legacy_reports_keep_their_bytes() {
    let mut wrong = Vec::new();
    for (archetype, seed, m2, hashes) in FILES {
        let b = built(archetype, seed, "m2", m2);
        for (name, hash) in ["report.json", "topology-graph.json", "building-ir.json"].into_iter().zip(hashes) {
            let got = fnv1a64(file(&b, name));
            if got != hash {
                wrong.push(format!("{archetype} {name} {got}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn corridor_fields_stay_in_layout() {
    let corridor = regex_lite_hits;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/generate");
    let mut hits = Vec::new();
    for name in ["lift.rs", "openings.rs", "script.rs", "report.rs"] {
        let text = std::fs::read_to_string(root.join(name)).unwrap();
        let mut in_tests = false;
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                in_tests = true;
            }
            let code = line.split("//").next().unwrap_or("");
            if !in_tests && corridor(code) {
                hits.push(format!("{name}:{}", i + 1));
            }
        }
    }
    assert!(hits.is_empty(), "corridor fields read outside the layout: {hits:?}");
}

/// `corridor_bottom|corridor_top|\.band\b|\.lobby\b|\.corridor\b`, without a regex crate.
fn regex_lite_hits(code: &str) -> bool {
    if code.contains("corridor_bottom") || code.contains("corridor_top") {
        return true;
    }
    [".band", ".lobby", ".corridor"].iter().any(|word| {
        code.match_indices(word).any(|(i, _)| !code[i + word.len()..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_'))
    })
}
