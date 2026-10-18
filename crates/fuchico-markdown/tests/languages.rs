//! Every one of the 143 fence languages must highlight a representative
//! sample (`tests/lang_samples/<slug>.txt`) with at least two distinct
//! styles. Set `LANG_FILTER=slug1,slug2` to check a subset.

use std::collections::{BTreeSet, HashSet};

use fuchico_markdown::highlight::{find_grammar, highlight_snippet};
use fuchico_markdown::languages::LANGUAGES;

fn slug(name: &str) -> String {
    name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

#[test]
fn every_language_highlights_a_sample() {
    assert_eq!(LANGUAGES.len(), 143);
    // Comma-separated exact slugs.
    let filter: Option<Vec<String>> =
        std::env::var("LANG_FILTER").ok().map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
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
            failures.push(format!("{} ({s}): grammar {grammar:?} not in syntax set", lang.name));
            continue;
        };
        let Ok(sample) = std::fs::read_to_string(format!("{dir}/{s}.txt")) else {
            failures.push(format!("{} ({s}): missing sample {s}.txt", lang.name));
            continue;
        };
        let styles: BTreeSet<_> = highlight_snippet(&sample, syntax).into_iter().map(|(_, id)| id.name()).collect();
        if styles.len() < 2 {
            failures.push(format!("{} ({s}): only {styles:?}", lang.name));
        } else {
            ok += 1;
        }
    }
    println!("{ok} languages highlight with >= 2 styles");
    assert!(failures.is_empty(), "{} failing:\n{}", failures.len(), failures.join("\n"));
}
