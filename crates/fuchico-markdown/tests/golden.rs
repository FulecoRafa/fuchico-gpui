//! Golden tests: displayed text per construct with the cursor off / on the
//! line (hidden marks removed, widgets shown as `[kind:...]`).

mod common;
use common::*;
use fuchico_markdown::*;

/// (doc, line, cursor off, cursor on)
fn check(doc: &str, line: usize, off: &str, on: &str) {
    let other = if line == 0 { None } else { Some(0) };
    assert_eq!(render(doc, other)[line], off, "cursor off, doc {doc:?}");
    assert_eq!(render(doc, Some(line))[line], on, "cursor on, doc {doc:?}");
}

#[test]
fn atx_heading() {
    check("x\n# Title", 1, "Title", "# Title");
    check("x\n### Title ###", 1, "Title ", "### Title ###");
    assert_eq!(classes("# T", 0), vec!["cm-md-h1"]);
    assert_eq!(classes("###### T", 0), vec!["cm-md-h6"]);
    let m = model("## T");
    assert_eq!(m.line_info(0).unwrap().height, LineHeight::H2);
}

#[test]
fn setext_heading() {
    check("x\n\nTitle\n===", 3, "", "===");
    check("x\n\nTitle\n===", 2, "Title", "Title");
    // Setext headings get no `cm-md-hN` line class (only ATXHeading did).
    assert!(classes("Title\n---", 0).is_empty());
}

#[test]
fn emphasis_strong_strike_code() {
    check("x\na *b* **c** ~~d~~ `e`", 1, "a b c d e", "a *b* **c** ~~d~~ `e`");
    check("x\n_u_ __v__", 1, "u v", "_u_ __v__");
    check("x\n``a ` b``", 1, "a ` b", "``a ` b``");
    let s = styles("**b** *i*", 0);
    assert_eq!(s, vec![("**b**".into(), "strong".into()), ("*i*".into(), "em".into())]);
}

#[test]
fn blockquote_and_nested() {
    check("x\n> q", 1, "q", "> q");
    check("x\n> > deep", 1, "deep", "> > deep");
    assert_eq!(classes("> q\n> r", 1), vec!["cm-md-blockquote"]);
    assert_eq!(model("> > d").line_info(0).unwrap().quote_depth, 2);
}

#[test]
fn callouts() {
    let doc = "x\n> [!TIP] Title here\n> body";
    check(doc, 1, "[callout:💡 Tip] Title here", "> [!TIP] Title here");
    check(doc, 2, "body", "> body");
    assert_eq!(classes(doc, 2), vec!["cm-callout cm-callout-tip"]);
    let info = model(doc).line_info(1).unwrap().callout.clone().unwrap();
    assert_eq!(info.kind, CalloutKind::Tip);
    assert_eq!(info.title, "Title here");
    assert!(info.header);
    // Case-insensitive type.
    check("x\n> [!caution]", 1, "[callout:🔥 Caution]", "> [!caution]");
    // Unknown type: a plain blockquote.
    assert_eq!(classes("> [!FOO] x", 0), vec!["cm-md-blockquote"]);
}

#[test]
fn hr_and_lists() {
    check("x\n\n---", 2, "---", "---");
    assert_eq!(classes("x\n\n***", 2), vec!["cm-md-hr"]);
    let m = model("- a\n  - b\n1. c");
    assert_eq!(m.line_info(1).unwrap().list_depth, 2);
    assert_eq!(m.line_info(2).unwrap().kind, BlockKind::ListItem { ordered: true, task: None });
}

#[test]
fn task_checkbox_always_rendered() {
    check("x\n- [ ] a", 1, "- [cb: ] a", "- [cb: ] a");
    check("x\n- [x] a", 1, "- [cb:x] a", "- [cb:x] a");
    let doc = "- [X] a";
    assert_eq!(checkbox_toggle(doc, 2), TextEdit::new(2..5, "[ ]"));
    assert_eq!(checkbox_toggle("- [ ] a", 2), TextEdit::new(2..5, "[x]"));
    let m = model("- [ ] a");
    let fs = FoldState::new();
    let d = m.decorations(0..1, &DecorationContext::new(None, &fs));
    match &d.lines[0].replacements[0].widget {
        Some(Widget::Checkbox { toggle, .. }) => assert_eq!(apply_edits("- [ ] a", std::slice::from_ref(toggle)), "- [x] a"),
        w => panic!("{w:?}"),
    }
}

#[test]
fn links_and_autolinks() {
    check("x\nsee [t](http://u) now", 1, "see [link:t->http://u] now", "see [t](http://u) now");
    check("x\n<https://a.b>", 1, "[link:https://a.b->https://a.b]", "<https://a.b>");
    // Titles make the raw text fail `^\[([^\]]*)\]\(([^)]*)\)$`? No: the
    // regex accepts anything but `)` in the parens.
    check("x\n[t](u \"T\")", 1, "[link:t->u \"T\"]", "[t](u \"T\")");
    // Images are not link widgets (and need `inline_images`).
    check("x\n![a](p.png)", 1, "![a](p.png)", "![a](p.png)");
    let s = styles("[t](http://u)", 0);
    assert_eq!(s[1], ("http://u".into(), "link url".into()));
}

#[test]
fn inline_images_setting() {
    let set = Settings { inline_images: true, ..Default::default() };
    let m = MarkdownModel::new("x\n![a](p.png)", set);
    assert_eq!(render_model(&m, Some(0), &FoldState::new())[1], "[img:a->p.png]");
    assert_eq!(render_model(&m, Some(1), &FoldState::new())[1], "![a](p.png)");
}

#[test]
fn wikilinks() {
    check("x\n[[Note A]] [[nope|Alias]]", 1, "[wiki:Note A] [wiki:Alias?]", "[[Note A]] [[nope|Alias]]");
    check("x\n`[[Note A]]`", 1, "[[Note A]]", "`[[Note A]]`");
    let m = model("[[nope]]");
    let fs = FoldState::new();
    let d = m.decorations(0..1, &DecorationContext::new(None, &fs));
    match &d.lines[0].replacements[0].widget {
        Some(Widget::Wikilink { title, resolved: None, .. }) => assert_eq!(title, "No note named \"nope\""),
        w => panic!("{w:?}"),
    }
}

#[test]
fn tags() {
    check("x\nhi #tag and #a/b-c", 1, "hi [tag:tag] and [tag:a/b-c]", "hi #tag and #a/b-c");
    check("x\nno#tag # x #1", 1, "no#tag # x #1", "no#tag # x #1");
    // Inside code (inline or fenced, any language) tags are not pills.
    check("x\n`#t`", 1, "#t", "`#t`");
    check("x\n```c\n#include\n```", 2, "#include", "#include");
}

#[test]
fn fenced_code() {
    let doc = "x\n```rust\nlet a = 1;\n```";
    check(doc, 1, "rust", "```rust");
    check(doc, 3, "", "```");
    assert_eq!(classes(doc, 1), vec!["cm-fence-header"]);
    assert_eq!(classes(doc, 2), vec!["cm-fence-body"]);
    assert_eq!(classes(doc, 3), vec!["cm-fence-footer"]);
    let s = styles(doc, 2);
    assert!(s.contains(&("let".into(), "keyword".into())), "{s:?}");
    assert!(s.contains(&("1".into(), "number".into())), "{s:?}");
    // Unknown language: monospace.
    assert_eq!(styles("```zzz\nfoo\n```", 1), vec![("foo".into(), "mono".into())]);
    let m = model(doc);
    let fs = FoldState::new();
    let d = m.decorations(0..4, &DecorationContext::new(None, &fs));
    match &d.lines[1].line_end[..] {
        [LineEndWidget::FenceToolbar(t)] => {
            assert_eq!(t.language, "rust");
            assert_eq!(&doc[t.lang_range.clone()], "rust");
            assert_eq!(t.copy_text, "let a = 1;");
            assert_eq!(t.options[0], "");
            assert_eq!(t.options[1], "typescript");
            assert_eq!(apply_edits(doc, &[t.set_language("python")]), "x\n```python\nlet a = 1;\n```");
        }
        w => panic!("{w:?}"),
    }
    assert_eq!(m.line_info(1).unwrap().kind, BlockKind::FenceOpen { language: "rust".into(), resolved: Some("Rust") });
}

#[test]
fn fence_toolbar_without_language() {
    let doc = "```\nab\n```";
    let m = model(doc);
    let fs = FoldState::new();
    let d = m.decorations(0..3, &DecorationContext::new(None, &fs));
    let LineEndWidget::FenceToolbar(t) = &d.lines[0].line_end[0] else { panic!() };
    assert_eq!(t.lang_range, 3..3);
    assert_eq!(t.set_language("go"), TextEdit::new(3..3, "go"));
}

#[test]
fn mermaid_button() {
    let doc = "```Mermaid \ngraph TD\nA-->B\n```\n";
    let m = model(doc);
    let fs = FoldState::new();
    let d = m.decorations(0..1, &DecorationContext::new(None, &fs));
    let btn = d.lines[0].line_end.iter().find_map(|w| match w {
        LineEndWidget::MermaidButton { anchor, text } => Some((*anchor, text.clone())),
        _ => None,
    });
    assert_eq!(btn, Some((0, "graph TD\nA-->B\n".into())));
}

#[test]
fn tables_are_block_widgets() {
    let doc = "x\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\nafter";
    for cur in [None, Some(1), Some(2)] {
        let r = render(doc, cur);
        assert_eq!(r[1..4], ["<table 2x2>", "<hidden:Block>", "<hidden:Block>"]);
    }
    let m = model(doc);
    let t = m.table(0).unwrap();
    assert_eq!(t.align, vec![None, Some(Align::Center)]);
    assert_eq!(&doc[t.cell(1, 1).unwrap().range.clone()], "2");
}

#[test]
fn frontmatter() {
    let doc = "---\ntitle: Hi\n  nested: 1\n---\n# H";
    assert_eq!(classes(doc, 0), vec!["cm-frontmatter-line", "cm-frontmatter-fence"]);
    assert_eq!(classes(doc, 1), vec!["cm-frontmatter-line"]);
    assert_eq!(classes(doc, 3), vec!["cm-frontmatter-line", "cm-frontmatter-fence"]);
    assert_eq!(styles(doc, 1), vec![("title".into(), "fm-key".into())]);
    assert_eq!(styles(doc, 2), vec![("nested".into(), "fm-key".into())]);
    // The `---` fences are not a setext underline / hr.
    check(doc, 3, "---", "---");
    assert_eq!(classes(doc, 4), vec!["cm-md-h1"]);
}

#[test]
fn bare_urls_entities_emoji_html() {
    let s = styles("go www.x.com/a. &amp; :ok: <b>x</b>", 0);
    assert!(s.contains(&("www.x.com/a".into(), "url".into())), "{s:?}");
    assert!(s.contains(&("&amp;".into(), "string".into())), "{s:?}");
    assert!(s.contains(&(":ok:".into(), "string".into())), "{s:?}");
    assert!(s.contains(&("b".into(), "type".into())), "{s:?}");
}

#[test]
fn regions_cursor_on_off() {
    let doc = "x\n:::fold My region\nbody\n:::endfold\ny";
    assert_eq!(render(doc, Some(0)), ["x", "[region:My region]", "body", "[region-end]", "y"]);
    assert_eq!(render(doc, Some(1))[1], ":::fold My region");
    assert_eq!(render(doc, Some(3))[3], ":::endfold");
    assert_eq!(classes(doc, 2), vec!["cm-region-body-line"]);
    assert_eq!(render("x\n:::fold\n:::endfold", Some(0))[1], "[region:Region]");
}

#[test]
fn indent_guides() {
    let m = model("a\n    b\n\n  c\n\td");
    assert_eq!(m.indent_guides(1), vec![0, 2]);
    assert_eq!(m.indent_guides(2), vec![0]);
    assert_eq!(m.indent_guides(0), Vec::<usize>::new());
    assert_eq!(m.indent_guides(4), vec![0]);
}

#[test]
fn visible_window_only() {
    let doc = (0..100).map(|i| format!("# h{i}")).collect::<Vec<_>>().join("\n");
    let m = model(&doc);
    let fs = FoldState::new();
    let d = m.decorations(40..45, &DecorationContext::new(Some(42), &fs));
    assert_eq!(d.lines.len(), 5);
    assert_eq!(d.get(42).unwrap().replacements.len(), 0);
    assert_eq!(d.get(43).unwrap().replacements.len(), 1);
}
