//! Every one of the 143 fence languages must highlight a representative
//! sample (`tests/lang_samples/<slug>.txt`) with at least two distinct
//! styles. Set `LANG_FILTER=slug1,slug2` to check a subset.

use std::collections::{BTreeSet, HashSet};

use fuchico_markdown::highlight::{find_grammar, highlight_snippet};
use fuchico_markdown::languages::LANGUAGES;

fn slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[test]
fn every_language_highlights_a_sample() {
    assert_eq!(LANGUAGES.len(), 143);
    // Comma-separated exact slugs.
    let filter: Option<Vec<String>> = std::env::var("LANG_FILTER")
        .ok()
        .map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lang_samples");
    let mut seen = HashSet::new();
    let mut failures = Vec::new();
    let mut ok = 0;
    for lang in LANGUAGES {
        let s = slug(lang.name);
        assert!(seen.insert(s.clone()), "slug collision: {s}");
        if filter.as_ref().is_some_and(|f| !f.contains(&s)) {
            continue;
        }
        let Some(grammar) = lang.grammar else {
            failures.push(format!("{} ({s}): no grammar", lang.name));
            continue;
        };
        let Some(syntax) = find_grammar(grammar) else {
            failures.push(format!(
                "{} ({s}): grammar {grammar:?} not in syntax set",
                lang.name
            ));
            continue;
        };
        let Ok(sample) = std::fs::read_to_string(format!("{dir}/{s}.txt")) else {
            failures.push(format!("{} ({s}): missing sample {s}.txt", lang.name));
            continue;
        };
        let styles: BTreeSet<_> = highlight_snippet(&sample, syntax)
            .into_iter()
            .map(|(_, id)| id.name())
            .collect();
        if styles.len() < 2 {
            failures.push(format!("{} ({s}): only {styles:?}", lang.name));
        } else {
            ok += 1;
        }
    }
    println!("{ok} languages highlight with >= 2 styles");
    assert!(
        failures.is_empty(),
        "{} failing:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn non_markdown_files_get_file_highlighting_without_live_preview() {
    use fuchico_markdown::{
        DecorationContext, FoldState, LineClass, MarkdownModel, Settings, StyleId,
    };
    let src = "# heading?\nx = \"**s**\"\n";
    let folds = FoldState::default();
    let ctx = DecorationContext::new(None, &folds);

    let mut py = MarkdownModel::with_path(src, Settings::default(), Some("notes/a.py"));
    assert!(!py.is_markdown());
    let d = py.decorations(0..2, &ctx);
    assert!(
        d.lines[0]
            .styles
            .iter()
            .any(|r| r.styles.contains(StyleId::Comment))
    );
    assert!(
        !d.lines[0]
            .classes
            .iter()
            .any(|c| matches!(c, LineClass::Heading(_)))
    );
    assert!(
        d.lines[1]
            .styles
            .iter()
            .any(|r| r.styles.contains(StyleId::String))
    );
    assert!(d.lines[1].replacements.is_empty());
    assert!(py.foldable(0).is_none());
    // Edits keep the highlighter in sync.
    py.update("x = 1  # c\n", &[]);
    let d = py.decorations(0..1, &ctx);
    assert!(
        d.lines[0]
            .styles
            .iter()
            .any(|r| r.styles.contains(StyleId::Comment))
    );

    // Unknown extensions are plain text; Markdown keeps the live preview.
    let txt = MarkdownModel::with_path(src, Settings::default(), Some("a.txt"));
    assert!(!txt.is_markdown());
    assert!(
        txt.decorations(0..2, &ctx)
            .lines
            .iter()
            .all(|l| l.styles.iter().all(|r| r.styles.is_empty()))
    );
    let mut md = MarkdownModel::with_path(src, Settings::default(), Some("a.md"));
    assert!(md.is_markdown());
    assert!(
        md.decorations(0..1, &ctx).lines[0]
            .classes
            .iter()
            .any(|c| matches!(c, LineClass::Heading(_)))
    );
    md.set_path(Some("a.rs"));
    assert!(!md.is_markdown());
}
