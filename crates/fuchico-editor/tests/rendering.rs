//! Gutter, folds, rulers, soft wrap, scrolling, virtualization and the
//! live-preview widgets' hit regions.

mod common;

use std::time::Instant;

use common::*;
use fuchico_editor::{EditorOptions, LineNumbers};
use gpui_kit::{Modifiers, TestAppContext, px, size};

#[gpui_kit::test]
async fn gutter_absolute_and_relative(cx: &mut TestAppContext) {
    let mut ui = open(cx, "a\nb\nc\nd\n", EditorOptions::default());
    let g = ui.read(|v, _| v.debug_gutter());
    assert_eq!(
        &g[..4],
        &[
            (0, "1".into()),
            (1, "2".into()),
            (2, "3".into()),
            (3, "4".into())
        ]
    );
    ui.update(|v, _, cx| {
        let mut o = v.options().clone();
        o.line_numbers = LineNumbers::Relative;
        v.set_options(o, cx);
    });
    ui.press("j");
    ui.press("j");
    let g = ui.read(|v, _| v.debug_gutter());
    assert_eq!(
        &g[..4],
        &[
            (0, "2".into()),
            (1, "1".into()),
            (2, "3".into()),
            (3, "1".into())
        ]
    );
    ui.update(|v, _, cx| {
        let mut o = v.options().clone();
        o.line_numbers = LineNumbers::Off;
        v.set_options(o, cx);
    });
    assert!(ui.read(|v, _| v.debug_gutter()).is_empty());
}

#[gpui_kit::test]
async fn rulers_at_columns(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        rulers: vec![10, 20],
        ..Default::default()
    };
    let mut ui = open(cx, "0123456789abcdefghij\n", opts);
    let (r, cw) = ui.read(|v, _| v.debug_rulers());
    assert_eq!(r.len(), 2);
    assert!(((r[1] - r[0]) - cw * 10.).abs() < px(0.5), "{r:?} {cw:?}");
    // The ruler at column 10 sits on the left edge of char 10.
    let b = ui.read(|v, _| v.bounds_for_char(10)).unwrap();
    assert!((b.origin.x - r[0]).abs() < px(1.), "{b:?} {r:?}");
}

#[gpui_kit::test]
async fn fold_chevron_click_and_fold_keys(cx: &mut TestAppContext) {
    let text = "# One\nbody one\nmore\n# Two\nbody two\n";
    let mut ui = open(cx, text, EditorOptions::default());
    assert!(ui.read(|v, _| v.bounds_for_char(6)).is_some());
    let p = ui.hit("FoldGutter(0)");
    ui.click(p, Modifiers::none());
    assert_eq!(ui.read(|v, _| v.folds().ranges().len()), 1);
    // "body one" (char 6) is hidden; "# Two" still visible.
    assert!(
        ui.read(|v, _| v.bounds_for_char(6)).is_none(),
        "folded line laid out"
    );
    assert!(ui.read(|v, _| v.bounds_for_char(21)).is_some());
    // The pill on the folded line unfolds.
    let p = ui.hit("FoldPill(");
    ui.click(p, Modifiers::none());
    assert!(ui.read(|v, _| v.folds().ranges().is_empty()));
    // Ctrl-Alt-[ folds everything, Ctrl-Alt-] unfolds.
    ui.press("ctrl-alt-[");
    assert_eq!(ui.read(|v, _| v.folds().ranges().len()), 2);
    ui.press("ctrl-alt-]");
    assert!(ui.read(|v, _| v.folds().ranges().is_empty()));
    // Cmd-Alt-[ folds the section under the cursor.
    ui.press("cmd-alt-[");
    assert_eq!(ui.read(|v, _| v.folds().ranges().len()), 1);
    ui.press("cmd-alt-]");
    assert!(ui.read(|v, _| v.folds().ranges().is_empty()));
}

#[gpui_kit::test]
async fn region_markers_fold(cx: &mut TestAppContext) {
    let text = "intro\n:::fold\nhidden\n:::endfold\nafter\n";
    let mut ui = open(cx, text, EditorOptions::default());
    let p = ui.hit("FoldGutter(1)");
    ui.click(p, Modifiers::none());
    assert_eq!(ui.read(|v, _| v.folds().ranges().len()), 1);
    assert!(ui.read(|v, _| v.bounds_for_char(14)).is_none());
}

#[gpui_kit::test]
async fn soft_wrap_rows_and_visual_motion(cx: &mut TestAppContext) {
    let long = "word ".repeat(80);
    let text = format!("{long}\nnext\n");
    let opts = EditorOptions {
        soft_wrap: true,
        ..Default::default()
    };
    let mut ui = open_sized(cx, &text, opts, size(px(500.), px(400.)));
    // A wrapped line: char 200 is on a lower row than char 0.
    let a = ui.read(|v, _| v.bounds_for_char(0)).unwrap();
    let b = ui.read(|v, _| v.bounds_for_char(200)).unwrap();
    assert!(b.origin.y > a.origin.y + px(10.), "{a:?} {b:?}");
    assert!(b.origin.x < px(500.));
    // `j` moves by visual row (VisualLines override): still on line 1.
    ui.press("j");
    assert_eq!(
        ui.read(|v, _| v.line_col().0),
        1,
        "j stayed on the wrapped line"
    );
    assert!(ui.head() > 0);
    // Without wrap the same `j` goes to the next line.
    ui.update(|v, _, cx| {
        let mut o = v.options().clone();
        o.soft_wrap = false;
        v.set_options(o, cx);
        v.set_selection(0, 0, cx);
    });
    ui.press("j");
    assert_eq!(ui.read(|v, _| v.line_col().0), 2);
}

#[gpui_kit::test]
async fn scroll_follows_cursor_and_host_get_set(cx: &mut TestAppContext) {
    let text: String = (1..=300).map(|i| format!("line {i}\n")).collect();
    let mut ui = open(cx, &text, EditorOptions::default());
    assert_eq!(ui.read(|v, _| v.scroll_position()), 0.);
    // Ctrl-d (half page) moves the cursor; once it leaves the viewport the
    // engine's Effect::Scroll (nearest) scrolls.
    ui.press("ctrl-d");
    let l1 = ui.read(|v, _| v.line_col().0);
    assert!(l1 > 5, "half page moved to {l1}");
    ui.press("ctrl-d");
    ui.press("ctrl-d");
    let y1 = ui.read(|v, _| v.scroll_position());
    assert!(y1 > 0., "ctrl-d scrolled");
    assert!(ui.read(|v, _| v.first_visible_line()) > 1);
    // `ge` to the end keeps the cursor visible.
    ui.press("g");
    ui.press("e");
    let (first, last) = ui.read(|v, _| v.visible_lines()).unwrap();
    assert!(last >= 299 && first > 200, "{first}..{last}");
    // Host restores a scroll offset.
    ui.update(|v, _, cx| v.set_scroll_position(400., cx));
    assert_eq!(ui.read(|v, _| v.scroll_position()), 400.);
    let first = ui.read(|v, _| v.first_visible_line());
    assert!(first > 10 && first < 40, "{first}");
    // reveal_line centres the line.
    ui.update(|v, _, cx| v.reveal_line(150, cx));
    let (a, b) = ui.read(|v, _| v.visible_lines()).unwrap();
    assert!(a < 150 && 150 < b, "{a}..{b}");
    assert_eq!(ui.read(|v, _| v.line_col().0), 150);
    // Wheel scrolling.
    let before = ui.read(|v, _| v.scroll_position());
    ui.cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: gpui_kit::point(px(300.), px(300.)),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-120.))),
        ..Default::default()
    });
    ui.frame();
    assert_eq!(ui.read(|v, _| v.scroll_position()), before + 120.);
}

#[gpui_kit::test]
async fn large_note_lays_out_only_visible_lines(cx: &mut TestAppContext) {
    let mut text = String::new();
    for i in 0..5000 {
        match i % 50 {
            0 => text.push_str(&format!("## Section {i}\n")),
            7 => text.push_str("- [ ] a task with **bold** and `code` #tag\n"),
            _ => text.push_str(&format!(
                "Line {i}: some *italic* text and a [[wiki link]] here.\n"
            )),
        }
    }
    let mut ui = open(cx, &text, EditorOptions::default());
    ui.update(|v, _, cx| v.reveal_line(2500, cx));
    let laid = ui.read(|v, _| v.debug_laid_out_lines());
    assert!(laid.len() < 80, "laid out {} lines", laid.len());
    assert!(laid.contains(&2499));
    ui.press("i");
    let t = Instant::now();
    for _ in 0..50 {
        ui.type_text("x");
    }
    let per_key = t.elapsed() / 50;
    eprintln!("5k-line note: {per_key:?} per keystroke (debug build, incl. a full frame)");
    assert!(per_key.as_millis() < 120, "{per_key:?} per keystroke");
    assert!(ui.text().contains("xxxxxxxxxx"));
}

#[gpui_kit::test]
async fn live_preview_hits_and_hidden_syntax(cx: &mut TestAppContext) {
    let text = "first line\n# Heading\nsome **bold** [link](https://x.y) [[Note]] #tag\n- [ ] task\n\n```rust\nfn main() {}\n```\n\n---\n> quote\n";
    let mut ui = open(cx, text, EditorOptions::default());
    let names: Vec<String> = ui
        .read(|v, _| v.debug_hits())
        .into_iter()
        .map(|h| h.0)
        .collect();
    for want in [
        "Checkbox(",
        "Link(",
        "Wikilink {",
        "Tag(",
        "Copy {",
        "Lang(",
    ] {
        assert!(
            names.iter().any(|n| n.starts_with(want)),
            "missing {want}: {names:?}"
        );
    }
    // Off the cursor line `**` is hidden: "bold" starts right where `**` would.
    let line2 = "first line\n# Heading\n".chars().count();
    let star = line2 + "some ".len();
    let s = ui.read(|v, _| v.bounds_for_char(star)).unwrap();
    let b = ui.read(|v, _| v.bounds_for_char(star + 2)).unwrap();
    assert!(
        b.origin.x - s.origin.x < px(1.),
        "markers hidden off-line: {s:?} {b:?}"
    );
    // On the cursor line they are shown.
    ui.update(|v, _, cx| v.set_selection(line2, line2, cx));
    let s = ui.read(|v, _| v.bounds_for_char(star)).unwrap();
    let b = ui.read(|v, _| v.bounds_for_char(star + 2)).unwrap();
    assert!(
        b.origin.x - s.origin.x > px(10.),
        "markers shown on the cursor line"
    );
}

/// A real PNG (`w`×`h`, two-colour) in a fresh temp vault at `att/pic.png`.
fn temp_vault_with_png(tag: &str, w: u32, h: u32) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fuchico-editor-{tag}-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    std::fs::create_dir_all(dir.join("att")).unwrap();
    let img = image::RgbaImage::from_fn(w, h, |x, _| {
        if x < w / 2 {
            image::Rgba([230, 120, 40, 255])
        } else {
            image::Rgba([40, 120, 230, 255])
        }
    });
    img.save(dir.join("att/pic.png")).unwrap();
    dir
}

#[gpui_kit::test]
async fn inline_images_load_from_vault(cx: &mut TestAppContext) {
    let vault = temp_vault_with_png("img", 120, 60);
    let options = EditorOptions {
        inline_images: true,
        vault_root: Some(vault.clone()),
        vault_files: vec!["att/pic.png".into(), "note.md".into()],
        path: Some("note.md".into()),
        ..EditorOptions::default()
    };
    let text = "intro\n![[pic.png]]\nmiddle\n![alt](att/pic.png)\n`![[pic.png]]` in code\nend\n";
    let mut ui = open(cx, text, options);
    // The decode runs on the background executor: wait, then relayout.
    for _ in 0..3 {
        ui.cx.run_until_parked();
        ui.frame();
    }
    let imgs = ui.read(|v, _| v.debug_images());
    let lines: Vec<_> = imgs.iter().map(|i| i.0).collect();
    assert_eq!(lines, vec![1, 3], "both syntaxes, not in code: {imgs:?}");
    for (_, b) in &imgs {
        assert_eq!((b.size.width, b.size.height), (px(120.), px(60.)));
    }
    // The preview sits below its source line and pushes the next line down.
    let src = ui.char_point(text.find("![[").unwrap()).y;
    let next = ui.char_point(text.find("middle").unwrap()).y;
    assert!(imgs[0].1.origin.y > src);
    assert!(next >= imgs[0].1.origin.y + px(60.), "{next:?} vs {imgs:?}");

    // Off by default (parity: the original editor shows no inline images).
    ui.update(|v, _, cx| {
        let mut o = v.options().clone();
        o.inline_images = false;
        v.set_options(o, cx);
    });
    ui.frame();
    assert!(ui.read(|v, _| v.debug_images()).is_empty());
    let _ = std::fs::remove_dir_all(vault);
}

#[gpui_kit::test]
async fn table_block_has_no_line_numbers(cx: &mut TestAppContext) {
    let text = "intro\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\nafter\n";
    let mut ui = open(cx, text, EditorOptions::default());
    let g = ui.read(|v, _| v.debug_gutter());
    let lines: Vec<_> = g.iter().map(|(l, _)| *l).collect();
    // CodeMirror's lineNumbers widgetMarker returns null for block
    // widgets: the replaced table lines (2..=4) get no number, and the
    // following lines keep their real numbers.
    assert!(lines.contains(&0) && lines.contains(&1));
    for l in 2..=4 {
        assert!(!lines.contains(&l), "table line {l} numbered: {g:?}");
    }
    let after = g.iter().find(|(l, _)| *l == 6).expect("line 7 numbered");
    assert_eq!(after.1, "7");
}
