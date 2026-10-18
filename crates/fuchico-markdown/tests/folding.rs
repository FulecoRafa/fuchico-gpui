mod common;
use common::*;
use fuchico_markdown::*;

fn folded_text(doc: &str, r: Option<std::ops::Range<usize>>) -> Option<String> {
    r.map(|r| doc[r].to_string())
}

#[test]
fn heading_sections() {
    let doc = "# A\npara\n## B\nb text\n# C\nc";
    let m = model(doc);
    assert_eq!(
        folded_text(doc, m.foldable(0)).as_deref(),
        Some("\npara\n## B\nb text")
    );
    assert_eq!(folded_text(doc, m.foldable(2)).as_deref(), Some("\nb text"));
    assert_eq!(folded_text(doc, m.foldable(4)).as_deref(), Some("\nc"));
    assert_eq!(m.foldable(1), None);
    // A heading with nothing under it is not foldable.
    assert_eq!(model("# A\n# B").foldable(0), None);
}

#[test]
fn syntax_folds() {
    let doc = "```js\na\nb\n```\n\n> q\n> r\n\n- item\n  more\n- two";
    let m = model(doc);
    assert_eq!(
        folded_text(doc, m.foldable(0)).as_deref(),
        Some("\na\nb\n```")
    );
    assert_eq!(folded_text(doc, m.foldable(5)).as_deref(), Some("\n> r"));
    // ListItem folds; the List itself does not.
    assert_eq!(folded_text(doc, m.foldable(8)).as_deref(), Some("\n  more"));
    assert_eq!(m.foldable(10), None);
    // Frontmatter folds (our addition).
    let fm = "---\na: 1\nb: 2\n---\nx";
    assert_eq!(
        folded_text(fm, model(fm).foldable(0)).as_deref(),
        Some("\na: 1\nb: 2\n---")
    );
}

#[test]
fn regions_take_precedence_and_nest() {
    let doc = "# H\n:::fold Outer\n:::fold Inner\nx\n:::endfold\ny\n:::endfold\nz";
    let m = model(doc);
    assert_eq!(
        folded_text(doc, m.foldable(1)).as_deref(),
        Some("\n:::fold Inner\nx\n:::endfold\ny\n:::endfold")
    );
    assert_eq!(
        folded_text(doc, m.foldable(2)).as_deref(),
        Some("\nx\n:::endfold")
    );
    // Unterminated region: falls through to the other services (here the
    // two-line paragraph's syntax fold).
    assert_eq!(model(":::fold\nx").foldable(0), Some(7..9));
    // Blank marker disables region folding.
    let s = Settings {
        fold_start_marker: " ".into(),
        ..Default::default()
    };
    let d2 = ":::fold\na\n\n:::endfold";
    assert_eq!(
        folded_text(d2, model(d2).foldable(0)).as_deref(),
        Some("\na\n\n:::endfold")
    );
    assert_eq!(
        folded_text(d2, MarkdownModel::new(d2, s).foldable(0)).as_deref(),
        Some("\na")
    );
    // Indented marker.
    assert!(model("  :::fold\nx\n:::endfold").foldable(0).is_some());
}

#[test]
fn placeholders() {
    let doc = ":::fold Box\na\nb\n:::endfold\n# H\nx";
    let m = model(doc);
    let r = m.foldable(0).unwrap();
    assert_eq!(
        m.fold_placeholder(&r),
        FoldPlaceholder::Region {
            name: "Box".into(),
            line_count: 2
        }
    );
    assert_eq!(
        m.fold_placeholder(&r).count_label().as_deref(),
        Some("2 lines")
    );
    let h = m.foldable(4).unwrap();
    assert_eq!(m.fold_placeholder(&h), FoldPlaceholder::Ellipsis);
    let unnamed = ":::fold\na\n:::endfold";
    let m2 = model(unnamed);
    let r2 = m2.foldable(0).unwrap();
    assert_eq!(
        m2.fold_placeholder(&r2),
        FoldPlaceholder::Region {
            name: "Region".into(),
            line_count: 1
        }
    );
}

#[test]
fn fold_all_skips_inner() {
    let doc = "# A\n## A1\nx\n# B\n```\nc\n```";
    let m = model(doc);
    let all = m.fold_all();
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(m.lines().line_of(all[0].start), 0);
    assert_eq!(m.lines().line_of(all[1].start), 3);
    let mut fs = FoldState::new();
    for r in all {
        fs.fold(r);
    }
    assert_eq!(fs.ranges().len(), 2);
    fs.clear();
    assert!(fs.is_empty());
}

#[test]
fn folded_decorations() {
    let doc = "x\n:::fold Box\na\nb\n:::endfold\n# H\nunder";
    let m = model(doc);
    let mut fs = FoldState::new();
    assert!(m.toggle_fold(&mut fs, 1));
    let r = render_model(&m, Some(0), &fs);
    assert_eq!(
        r,
        [
            "x",
            "[region-hidden][fold:Box:2]",
            "<hidden:Folded>",
            "<hidden:Folded>",
            "<hidden:Folded>",
            "H",
            "under"
        ]
    );
    // Cursor on the marker line: the raw marker shows, plus the pill.
    let r = render_model(&m, Some(1), &fs);
    assert_eq!(r[1], ":::fold Box[fold:Box:2]");
    // Toggle again unfolds.
    assert!(m.toggle_fold(&mut fs, 1));
    assert!(fs.is_empty());
    m.toggle_fold(&mut fs, 5);
    let r = render_model(&m, Some(0), &fs);
    assert_eq!(r[5..], ["H[…]", "<hidden:Folded>"]);
}

#[test]
fn fold_state_maps_and_reveals() {
    let doc = "# A\nx\ny";
    let m = model(doc);
    let mut fs = FoldState::new();
    m.toggle_fold(&mut fs, 0);
    assert_eq!(fs.ranges(), vec![3..7]);
    fs.map(&[TextEdit::insert(0, "#")]);
    assert_eq!(fs.ranges(), vec![4..8]);
    fs.reveal(6);
    assert!(fs.is_empty());
}

#[test]
fn code_file_folding() {
    let js = "function f() {\n  if (x) {\n    y();\n  }\n}\nconst s = \"{\";";
    let idx = LineIndex::new(js);
    let r = code_foldable(js, &idx, 0, CodeFoldStyle::Brackets, 2).unwrap();
    assert_eq!(&js[r], "\n  if (x) {\n    y();\n  }\n");
    assert!(code_foldable(js, &idx, 5, CodeFoldStyle::Brackets, 2).is_none());
    assert_eq!(CodeFoldStyle::for_language("Python"), CodeFoldStyle::Indent);
}
