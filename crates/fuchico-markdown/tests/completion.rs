mod common;
use chrono::NaiveDate;
use common::*;
use fuchico_core::fuzzy::fuzzy_match;
use fuchico_markdown::completion::{extract_headings, should_open_completion, MAX_RENDERED_OPTIONS};
use fuchico_markdown::*;

fn no_read(_: &str) -> Option<String> {
    None
}

fn run(doc: &str, pos: usize, files: &[String], tags: &[String]) -> Vec<CompletionOption> {
    let m = model(doc);
    let ctx = CompletionContext {
        pos,
        vault_files: files,
        all_tags: tags,
        today: NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
        read_note: &no_read,
    };
    m.completions(&ctx)
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn wikilink_ranking_and_apply() {
    let files = s(&["/v/Alpha.md", "/v/sub/alphabet.md", "/v/Beta.md", "/v/al.md"]);
    let doc = "see [[al";
    let opts = run(doc, doc.len(), &files, &[]);
    let labels: Vec<_> = opts.iter().map(|o| o.label.as_str()).collect();
    // Order = fuzzy score descending, stable for ties.
    let mut expect: Vec<(&str, f64)> = ["Alpha", "alphabet", "Beta", "al"]
        .iter()
        .filter_map(|l| {
            let r = fuzzy_match("al", l);
            r.matched.then_some((*l, r.score))
        })
        .collect();
    expect.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    assert_eq!(labels, expect.iter().map(|e| e.0).collect::<Vec<_>>());
    let o = &opts[0];
    assert_eq!(o.source, CompletionSource::Wikilink);
    assert_eq!(o.range, 6..8);
    assert_eq!(o.detail.as_deref(), Some(files.iter().find(|f| f.contains(&format!("/{}.md", o.label))).unwrap().as_str()));
    assert_eq!(apply_edits(doc, std::slice::from_ref(&o.edit)), format!("see [[{}]]", o.label));
    assert_eq!(o.cursor, 6 + o.label.len() + 2);
    assert!(o.picked);
    // Existing `]]` is not duplicated.
    let doc2 = "[[Be]]";
    let o = &run(doc2, 4, &files, &[])[0];
    assert_eq!(apply_edits(doc2, std::slice::from_ref(&o.edit)), "[[Beta]]");
    assert_eq!(o.cursor, 6);
}

#[test]
fn heading_completion() {
    let doc = "# One\n## Two ##\n```\n# Not\n```\n[[#t";
    let opts = run(doc, doc.len(), &[], &[]);
    assert_eq!(opts[0].label, "Two");
    assert_eq!(opts[0].source, CompletionSource::Heading);
    assert_eq!(apply_edits(doc, &[opts[0].edit.clone()]), "# One\n## Two ##\n```\n# Not\n```\n[[#Two]]");
    assert_eq!(extract_headings(doc), vec!["One", "Two"]);
    // Other note via the host reader.
    let files = s(&["/v/Other.md"]);
    let m = model("[[Other#");
    let read = |p: &str| (p == "/v/Other.md").then(|| "# Remote\n".to_string());
    let ctx = CompletionContext {
        pos: 8,
        vault_files: &files,
        all_tags: &[],
        today: NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
        read_note: &read,
    };
    let o = m.completions(&ctx);
    assert_eq!(o.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), vec!["Remote"]);
    // Unresolvable target: nothing from the heading source (the wikilink
    // source does not match because `#` is excluded from its query? no: it
    // is not, `[^[\]]*` accepts `#`).
    let o = run("[[Nope#x", 8, &files, &[]);
    assert!(o.iter().all(|o| o.source != CompletionSource::Heading));
}

#[test]
fn tag_completion() {
    let tags = s(&["project", "personal", "pro/x", "zzz"]);
    let doc = "a #pro";
    let opts = run(doc, doc.len(), &[], &tags);
    let labels: Vec<_> = opts.iter().filter(|o| o.source == CompletionSource::Tag).map(|o| o.label.clone()).collect();
    assert!(labels.contains(&"project".to_string()));
    assert!(!labels.contains(&"zzz".to_string()));
    let o = opts.iter().find(|o| o.label == "project").unwrap();
    assert_eq!(o.range, 3..6);
    assert_eq!(apply_edits(doc, std::slice::from_ref(&o.edit)), "a #project");
    assert_eq!(o.kind, CompletionType::Text);
    // `#` must follow whitespace or line start.
    assert!(run("a#pro", 5, &[], &tags).iter().all(|o| o.source != CompletionSource::Tag));
}

#[test]
fn task_completion() {
    let doc = "- [ ] pay @due";
    let opts = run(doc, doc.len(), &[], &[]);
    let t: Vec<_> = opts.iter().filter(|o| o.source == CompletionSource::Task).collect();
    assert!(!t.is_empty());
    assert_eq!(t[0].kind, CompletionType::Keyword);
    assert!(!t[0].picked);
    assert_eq!(t[0].range.start, 10);
    let applied = apply_edits(doc, &[t[0].edit.clone()]);
    assert!(applied.starts_with("- [ ] pay 📅 2026-"), "{applied}");
    assert_eq!(t[0].cursor, 10 + t[0].edit.insert.len());
    // Bare `@` offers due dates then "Repeat: ..." options.
    let opts = run("x @", 3, &[], &[]);
    assert!(opts.iter().any(|o| o.label.starts_with("Repeat: ")));
    let recur = run("x @repeat", 9, &[], &[]);
    assert_eq!(recur[0].kind, CompletionType::Variable);
}

#[test]
fn word_completion() {
    let doc = "elephant elegant element\n\nele";
    let opts = run(doc, doc.len(), &[], &[]);
    assert!(opts.is_empty(), "prefix shorter than 4");
    let doc = "elephant elegant element\n\nelem";
    let opts = run(doc, doc.len(), &[], &[]);
    assert_eq!(opts[0].label, "element");
    assert!(opts.iter().all(|o| o.source == CompletionSource::Word));
    assert!(opts.iter().all(|o| o.label.to_lowercase() != "elem"));
    // Not inside code, not inside an open wikilink.
    let doc = "elephant\n`elep`";
    assert!(run(doc, doc.len() - 1, &[], &[]).is_empty());
    let doc = "elephant\n```\nelep\n```";
    assert!(run(doc, doc.len() - 4, &[], &[]).is_empty());
    // Same in a recognised-language fence (deliberate fix, see README).
    let doc = "elephant\n```rust\nelep\n```";
    assert!(run(doc, doc.len() - 4, &[], &[]).is_empty());
    let doc = "elephant [[elep";
    assert!(run(doc, doc.len(), &[], &[]).iter().all(|o| o.source != CompletionSource::Word));
}

#[test]
fn provider_order_dedupe_and_cap() {
    // Tag and word sources both fire for `#abcd`; tags come first.
    let doc = "abcdef #abcd";
    let opts = run(doc, doc.len(), &[], &s(&["abcdx"]));
    let src: Vec<_> = opts.iter().map(|o| o.source).collect();
    assert_eq!(src, vec![CompletionSource::Tag, CompletionSource::Word]);
    // Consecutive duplicates collapse: tag "abcdef" then word "abcdef".
    let opts = run(doc, doc.len(), &[], &s(&["abcdef"]));
    assert_eq!(opts.len(), 1);
    // Cap at 100 rendered options.
    let files: Vec<String> = (0..300).map(|i| format!("/v/n{i}.md")).collect();
    let tags: Vec<String> = (0..300).map(|i| format!("n{i}")).collect();
    assert!(run("[[n", 3, &files, &tags).len() <= MAX_RENDERED_OPTIONS);
}

#[test]
fn reopen_trigger() {
    assert!(should_open_completion("x [[", false));
    assert!(should_open_completion("x [[a#", false));
    assert!(should_open_completion("#t", false));
    assert!(should_open_completion("x @d", false));
    assert!(!should_open_completion("x [[note", true));
    assert!(!should_open_completion("abc", false));
}
