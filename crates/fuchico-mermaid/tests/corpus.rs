//! Parity corpus: every pele diagram type, the demo notes and tricky cases.
//! Each sample must render (or fail) in both themes without panicking, with
//! the same success/error split the original (pele 1.5.2 in the browser)
//! has: `error-*` samples and ZenUML (not a pele type) fail, the rest render.

use fuchico_mermaid::{MermaidTheme, rasterize, render};
use std::path::PathBuf;

fn corpus() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mmd"))
        .map(|p| {
            (
                p.file_stem().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn expect_error(name: &str) -> bool {
    name.starts_with("error-") || name == "zenuml-basic"
}

#[test]
fn corpus_split_matches_the_original() {
    let samples = corpus();
    assert!(
        samples.len() >= 55,
        "corpus went missing: {}",
        samples.len()
    );
    let mut mismatches = Vec::new();
    for (name, src) in &samples {
        for theme in [MermaidTheme::light(), MermaidTheme::dark()] {
            let r = std::panic::catch_unwind(|| render(src, &theme));
            let Ok(r) = r else {
                mismatches.push(format!("{name}: panicked"));
                continue;
            };
            match (r, expect_error(name)) {
                (Ok(d), false) => {
                    assert!(!d.svg.contains("var("), "{name}: unresolved var()");
                    assert!(d.svg.starts_with("<svg"), "{name}");
                    assert!(d.width > 0.0 && d.height > 0.0, "{name}");
                    let r = rasterize(&d.svg, 1.0).unwrap_or_else(|e| panic!("{name}: {e}"));
                    assert!(r.width > 0.0 && r.height > 0.0, "{name}");
                }
                (Err(e), true) => assert!(!e.message.is_empty(), "{name}"),
                (Ok(_), true) => mismatches.push(format!("{name}: rendered, expected error")),
                (Err(e), false) => mismatches.push(format!("{name}: {e}")),
            }
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

#[test]
fn every_pele_type_is_covered() {
    let kinds: std::collections::BTreeSet<String> = corpus()
        .iter()
        .filter(|(n, _)| !expect_error(n))
        .map(|(n, s)| {
            render(s, &MermaidTheme::light())
                .unwrap_or_else(|e| panic!("{n}: {e}"))
                .kind
        })
        .collect();
    for k in [
        "flowchart",
        "sequence",
        "class",
        "state",
        "er",
        "gantt",
        "pie",
        "gitGraph",
        "mindmap",
        "timeline",
        "journey",
        "quadrantChart",
        "xychart",
        "requirement",
        "c4",
        "sankey",
        "block",
        "packet",
        "kanban",
        "architecture",
        "radar",
        "treemap",
        "info",
        "agentflow",
        "usecase",
        "treeView",
        "venn",
        "cynefin",
        "wardley",
        "swimlane",
        "ishikawa",
        "eventmodeling",
    ] {
        assert!(
            kinds
                .iter()
                .any(|x| x.eq_ignore_ascii_case(k) || x.starts_with(k)),
            "no sample renders as {k}; got {kinds:?}"
        );
    }
}

#[test]
fn syntax_errors_carry_pele_line_and_caret() {
    for (name, src) in corpus() {
        if !name.starts_with("error-") || name == "error-empty" || name == "error-unknown-type" {
            continue;
        }
        let e = render(&src, &MermaidTheme::light()).unwrap_err();
        if name == "error-gantt-bad-task" {
            assert_eq!(e.message, "Invalid date:notadate");
            continue;
        }
        assert!(e.line.is_some(), "{name}: {e}");
        assert!(e.message.contains('^'), "{name}: {e}");
    }
}
