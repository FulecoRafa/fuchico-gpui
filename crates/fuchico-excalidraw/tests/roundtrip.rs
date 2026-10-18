//! Lossless round-trip tests on the demo drawings and a synthesized file
//! that covers every element type plus unknown fields.

use std::collections::HashSet;

use fuchico_excalidraw::model::Num;
use fuchico_excalidraw::{ExcalidrawFile, Scene};
use serde_json::Value;

const DEMO: &[&str] = &[
    "/Users/fuleco/Documents/Dev/obsidian-helix/demo-notes/desenhos/Desenho.excalidraw",
    "/Users/fuleco/Documents/Dev/obsidian-helix/demo-notes/outro.excalidraw",
];
const FIXTURE: &str = include_str!("fixtures/all_elements.excalidraw");

/// Numbers compared as f64 (serde may print `2` vs `2.0`).
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        o => o.clone(),
    }
}

fn sources() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = DEMO
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok().map(|s| (p.to_string(), s)))
        .collect();
    assert!(out.len() == DEMO.len(), "demo drawings missing");
    out.push(("fixture".into(), FIXTURE.into()));
    out
}

#[test]
fn scene_round_trip_is_exact_for_untouched() {
    for (name, src) in sources() {
        let orig: Value = serde_json::from_str(&src).unwrap();
        let scene = Scene::parse(&src).unwrap();
        let out = scene.to_value();
        assert_eq!(out, orig, "{name}: untouched scene must serialize to the identical value");
        // And parse→serialize→parse is stable.
        let again = Scene::parse(&scene.to_json()).unwrap().to_value();
        assert_eq!(again, out, "{name}: second round trip differs");
    }
}

#[test]
fn typed_model_round_trip_is_semantic() {
    for (name, src) in sources() {
        let orig: Value = serde_json::from_str(&src).unwrap();
        let file = ExcalidrawFile::parse(&src).unwrap();
        let out: Value = serde_json::from_str(&file.to_json()).unwrap();
        assert_eq!(norm(&out), norm(&orig), "{name}: typed model is lossy");
        let file2 = ExcalidrawFile::parse(&file.to_json()).unwrap();
        assert_eq!(file2, file, "{name}: parse→serialize→parse differs");
    }
}

#[test]
fn every_element_kind_parses() {
    let scene = Scene::parse(FIXTURE).unwrap();
    let kinds: HashSet<String> = scene.elements().map(|e| e.kind.clone()).collect();
    for k in ["frame", "rectangle", "text", "diamond", "ellipse", "arrow", "line", "freedraw", "image", "embeddable"] {
        assert!(kinds.contains(k), "missing {k}");
    }
    // Deleted elements are kept in the file but hidden from the scene.
    assert!(scene.get("gone").is_none() || scene.get("gone").unwrap().is_deleted());
    assert!(scene.elements().all(|e| e.id != "gone"));
    let arrow = scene.get("arrow1").unwrap();
    assert_eq!(arrow.f.start_binding.get().unwrap().element_id, "rect1");
    assert_eq!(scene.get("text1").unwrap().container_id(), Some("rect1"));
    assert!(scene.file_data_url("fileA").unwrap().starts_with("data:image/png;base64,"));
    for el in scene.elements() {
        let _ = fuchico_excalidraw::shape::generate(el);
    }
}

#[test]
fn editing_one_element_leaves_others_byte_identical() {
    let orig: Value = serde_json::from_str(FIXTURE).unwrap();
    let mut scene = Scene::parse(FIXTURE).unwrap();
    let el = scene.get_mut("ell1").unwrap();
    el.f.x.set(Num(42.0));
    let out = scene.to_value();
    let oe = orig["elements"].as_array().unwrap();
    let ne = out["elements"].as_array().unwrap();
    assert_eq!(oe.len(), ne.len());
    for (a, b) in oe.iter().zip(ne) {
        if a.get("id").and_then(|v| v.as_str()) == Some("ell1") {
            assert_eq!(b["x"].as_f64(), Some(42.0));
            // Unchanged fields of the edited element survive too.
            let mut a2 = norm(a);
            a2["x"] = serde_json::json!(42.0);
            assert_eq!(norm(b), a2);
        } else {
            assert_eq!(a, b, "untouched element changed");
        }
    }
    assert_eq!(out["files"], orig["files"]);
    assert_eq!(out["topLevelUnknown"], orig["topLevelUnknown"]);
    assert_eq!(out["appState"], orig["appState"]);
}

#[test]
fn malformed_and_empty() {
    assert!(Scene::parse("{not json").is_err());
    assert!(Scene::parse("[1,2]").is_err());
    assert!(Scene::parse(r#"{"elements": 5}"#).is_err());
    let empty = Scene::parse("").unwrap();
    let v = empty.to_value();
    assert_eq!(v["type"], "excalidraw");
    assert_eq!(v["version"], 2);
    assert!(v["elements"].as_array().unwrap().is_empty());
}
