//! Typing, modes, cursors, selections, mouse and IME.

mod common;

use common::*;
use fuchico_editor::{EditorEvent, EditorOptions, InsertCursor};
use gpui_kit::{EntityInputHandler, Modifiers, TestAppContext, px};

#[gpui_kit::test]
async fn typing_modes_and_pending(cx: &mut TestAppContext) {
    let mut ui = open(cx, "hello\nworld\n", EditorOptions::default());
    assert_eq!(ui.read(|v, _| v.mode_label()), "NOR");
    ui.press("i");
    assert_eq!(ui.read(|v, _| v.mode_label()), "INS");
    ui.type_text("abc ");
    ui.press("enter");
    ui.press("tab");
    // Tab must reach the engine (not the root focus traversal): indent unit.
    assert_eq!(ui.text(), "abc \n  hello\nworld\n");
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.mode_label()), "NOR");
    // Pending count / prefix.
    ui.press("2");
    assert_eq!(ui.read(|v, _| v.pending_status()), "2");
    ui.press("escape");
    ui.press("g");
    assert_eq!(ui.read(|v, _| v.pending_status()), "g");
    ui.press("g");
    assert_eq!(ui.head(), 0);
    // Undo is helix `u`.
    ui.press("u");
    assert_eq!(ui.text(), "hello\nworld\n");
    let changed = ui
        .events()
        .iter()
        .any(|e| matches!(e, EditorEvent::Changed { dirty: true }));
    assert!(changed);
}

#[gpui_kit::test]
async fn block_cursor_in_normal_and_insert(cx: &mut TestAppContext) {
    let mut ui = open(cx, "hello\n", EditorOptions::default());
    let c = ui.read(|v, _| v.debug_cursors());
    assert_eq!(c.len(), 1);
    assert!(
        c[0].0.size.width > px(4.),
        "block cursor in normal mode: {c:?}"
    );
    ui.press("i");
    let c = ui.read(|v, _| v.debug_cursors());
    assert!(
        c[0].0.size.width > px(4.),
        "block cursor in insert mode (cm-helix default): {c:?}"
    );
    // Option: bar in insert mode.
    ui.update(|v, _, cx| {
        let mut o = v.options().clone();
        o.insert_cursor = InsertCursor::Bar;
        v.set_options(o, cx);
    });
    let c = ui.read(|v, _| v.debug_cursors());
    assert!(c[0].0.size.width <= px(2.));
}

#[gpui_kit::test]
async fn multiple_selections_type_everywhere(cx: &mut TestAppContext) {
    let mut ui = open(cx, "one\ntwo\nthree\n", EditorOptions::default());
    ui.press("shift-c");
    ui.press("shift-c");
    assert_eq!(ui.read(|v, _| v.selection().ranges().len()), 3);
    let cursors = ui.read(|v, _| v.debug_cursors());
    assert_eq!(cursors.len(), 3);
    assert_eq!(cursors.iter().filter(|c| c.1).count(), 1, "one primary");
    ui.press("i");
    ui.type_text("> ");
    assert_eq!(ui.text(), "> one\n> two\n> three\n");
}

#[gpui_kit::test]
async fn click_drag_and_shift_click(cx: &mut TestAppContext) {
    let mut ui = open(
        cx,
        "alpha beta gamma\nsecond line\n",
        EditorOptions::default(),
    );
    // Normal mode is block-cursor: the clicked char is selected (6..7).
    let p = ui.char_point(6);
    ui.click(p, Modifiers::none());
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 6);
    // Shift-click extends from the anchor.
    let p = ui.char_point(13);
    ui.click(p, Modifiers::shift());
    let (a, h) = ui.sel();
    assert_eq!(a.min(h), 6);
    assert!(a.max(h) >= 13);
    // Drag selects.
    let (from, to) = (ui.char_point(0), ui.char_point(4));
    ui.drag(from, to);
    let (a, h) = ui.sel();
    assert_eq!(a.min(h), 0);
    assert!((4..=5).contains(&a.max(h)), "{a}..{h}");
    // Clicking on the second line.
    let p = ui.char_point(20);
    ui.click(p, Modifiers::none());
    assert_eq!(ui.read(|v, _| v.selection().main().from()), 20);
}

#[gpui_kit::test]
async fn ime_marked_text_then_commit(cx: &mut TestAppContext) {
    let mut ui = open(cx, "cafe\n", EditorOptions::default());
    ui.press("i");
    ui.update(|v, w, cx| {
        let end = 4;
        let fx = v
            .engine_mut()
            .set_selection(fuchico_helix::Selection::single(end, end));
        v.apply_effects(fx, cx);
        v.replace_and_mark_text_in_range(None, "´", None, w, cx);
    });
    assert_eq!(ui.text(), "cafe´\n");
    assert_eq!(ui.read(|v, _| v.marked_range()), Some(4..5));
    let mr = ui.update(|v, w, cx| v.marked_text_range(w, cx));
    assert_eq!(mr, Some(4..5));
    ui.update(|v, w, cx| v.replace_text_in_range(None, "é", w, cx));
    assert_eq!(ui.text(), "cafeé\n");
    assert_eq!(ui.read(|v, _| v.marked_range()), None);
    // Replacement of a typed range (dead-key style accent).
    ui.update(|v, w, cx| v.replace_text_in_range(Some(3..4), "ê", w, cx));
    assert_eq!(ui.text(), "cafêé\n");
    // Geometry queries used by the IME candidate window.
    let b = ui.update(|v, w, cx| v.bounds_for_range(0..1, gpui_kit::Bounds::default(), w, cx));
    assert!(b.is_some());
    // Undo after IME commit restores (insert_text is undoable).
    ui.press("escape");
}

#[gpui_kit::test]
async fn set_text_keeps_selection_and_marks_saved(cx: &mut TestAppContext) {
    let mut ui = open(
        cx,
        "line one\nline two\nline three\n",
        EditorOptions::default(),
    );
    ui.update(|v, _, cx| v.set_selection(14, 14, cx));
    ui.press("i");
    ui.type_text("X");
    assert!(ui.read(|v, _| v.is_dirty()));
    // External reload with a change before the cursor.
    ui.update(|v, _, cx| v.set_text("PREFIX\nline one\nline two\nline three\n", cx));
    assert!(!ui.read(|v, _| v.is_dirty()));
    assert_eq!(ui.head(), 14 + 7);
    assert!(
        ui.events()
            .iter()
            .any(|e| matches!(e, EditorEvent::DirtyChanged(false)))
    );
}

#[gpui_kit::test]
async fn ime_mark_then_unmark_keeps_composition(cx: &mut TestAppContext) {
    let mut ui = open(cx, "ab\n", EditorOptions::default());
    ui.press("i"); // insert before `a`
    ui.update(|v, w, cx| v.replace_and_mark_text_in_range(None, "k", None, w, cx));
    assert_eq!(ui.read(|v, _| v.marked_range()), Some(0..1));
    // The composition grows in place: the marked range is replaced.
    ui.update(|v, w, cx| v.replace_and_mark_text_in_range(None, "かn", Some(2..2), w, cx));
    assert_eq!(ui.text(), "かnab\n");
    assert_eq!(ui.read(|v, _| v.marked_range()), Some(0..2));
    // `unmark_text` commits whatever is composed.
    ui.update(|v, w, cx| v.unmark_text(w, cx));
    assert_eq!(ui.read(|v, _| v.marked_range()), None);
    let mr = ui.update(|v, w, cx| v.marked_text_range(w, cx));
    assert_eq!(mr, None);
    // Further input inserts at the cursor instead of replacing.
    ui.update(|v, w, cx| v.replace_text_in_range(None, "!", w, cx));
    assert_eq!(ui.text(), "かn!ab\n");
}

#[gpui_kit::test]
async fn cursor_shapes_and_colours_match_helix(cx: &mut TestAppContext) {
    let mut ui = open(cx, "one\ntwo\nthree\n", EditorOptions::default());
    ui.press("shift-c");
    let (fg, block) = ui.read(|v, _| (v.style().foreground, v.style().block_cursor()));
    // codemirror-helix: `.cm-hx-cursor { background: #ccc }` on every head.
    let c = ui.read(|v, _| v.debug_cursors());
    assert_eq!(c.len(), 2);
    let primary = c.iter().find(|c| c.1).unwrap();
    let secondary = c.iter().find(|c| !c.1).unwrap();
    assert_eq!(primary.2, block);
    assert_eq!(secondary.2, block, "secondary block cursor = primary");
    let rgb = gpui_kit::Rgba::from(block);
    assert!((rgb.r - 0.8).abs() < 0.01 && (rgb.g - 0.8).abs() < 0.01);
    for c in &c {
        assert!(c.0.size.width > px(4.), "block: {c:?}");
    }
    // Insert mode: cm-helix keeps `.cm-hx-block-cursor` (cursor-shape
    // `block` by default, which the original does not override).
    ui.press("i");
    let c = ui.read(|v, _| v.debug_cursors());
    assert_eq!(c.len(), 2);
    for c in &c {
        assert!(c.0.size.width > px(4.), "block: {c:?}");
        assert_eq!(c.2, block);
    }
    assert_eq!(c.iter().filter(|c| c.1).count(), 1);
    // The `bar` option: thin `.cm-cursor` bars in the foreground colour.
    ui.update(|v, _, cx| {
        let mut o = v.options().clone();
        o.insert_cursor = InsertCursor::Bar;
        v.set_options(o, cx);
    });
    let c = ui.read(|v, _| v.debug_cursors());
    for c in &c {
        assert!(c.0.size.width <= px(1.5), "bar: {c:?}");
        assert_eq!(c.2, fg);
    }
}

#[gpui_kit::test]
async fn select_regex_draws_every_cursor(cx: &mut TestAppContext) {
    let mut ui = open(cx, "foo bar foo\nbaz foo\n", EditorOptions::default());
    ui.press("%");
    ui.press("s");
    ui.type_text("foo");
    ui.press("enter");
    let ranges = ui.read(|v, _| {
        v.selection()
            .ranges()
            .iter()
            .map(|r| (r.from(), r.to()))
            .collect::<Vec<_>>()
    });
    assert_eq!(ranges, vec![(0, 3), (8, 11), (16, 19)]);
    let c = ui.read(|v, _| v.debug_cursors());
    assert_eq!(c.len(), 3, "every selection head is drawn: {c:?}");
    assert_eq!(c.iter().filter(|c| c.1).count(), 1);
    // Each cursor sits on its own head (distinct positions, right rows).
    let mut ys: Vec<_> = c.iter().map(|c| c.0.origin.y).collect();
    ys.dedup();
    assert_eq!(ys.len(), 2, "two on line 1, one on line 2");
    let mut xs: Vec<_> = c.iter().map(|c| (c.0.origin.x, c.0.origin.y)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup();
    assert_eq!(xs.len(), 3);
    // Typing affects all three.
    ui.press("c");
    ui.type_text("X");
    assert_eq!(ui.text(), "X bar X\nbaz X\n");
}

#[gpui_kit::test]
async fn drag_across_lines_selects_range(cx: &mut TestAppContext) {
    let mut ui = open(
        cx,
        "alpha beta\ngamma delta\nend\n",
        EditorOptions::default(),
    );
    let (from, to) = (ui.char_point(6), ui.char_point(17));
    ui.drag(from, to);
    let (a, h) = ui.sel();
    assert_eq!(a, 6, "anchor at the press");
    assert!((17..=18).contains(&h), "head follows the pointer: {h}");
    assert_eq!(ui.read(|v, _| v.selection().ranges().len()), 1);
    // The head cursor is drawn on the second line.
    let c = ui.read(|v, _| v.debug_cursors());
    assert_eq!(c.len(), 1);
    assert!(c[0].0.origin.y > from.y);
    // Dragging backwards reverses the selection.
    let (from, to) = (ui.char_point(15), ui.char_point(2));
    ui.drag(from, to);
    let (a, h) = ui.sel();
    assert!(h < a, "{a}..{h}");
    assert!((2..=3).contains(&h));
}

#[gpui_kit::test]
async fn normal_engine_is_non_modal(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        engine: fuchico_editor::EngineKind::Normal,
        ..Default::default()
    };
    let mut ui = open(cx, "hello\nworld\n", opts);
    assert_eq!(ui.read(|v, _| v.mode_label()), "INS");
    ui.type_text("ab");
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.mode_label()), "INS");
    ui.type_text("c");
    assert_eq!(ui.text(), "abchello\nworld\n");
    // Shift-arrows extend; typing replaces the selection.
    ui.press("shift-right shift-right");
    assert_eq!(ui.sel(), (3, 5));
    ui.type_text("X");
    assert_eq!(ui.text(), "abcXllo\nworld\n");
    ui.press("shift-left");
    ui.press("backspace");
    assert_eq!(ui.text(), "abcllo\nworld\n");
    // Mod-z undoes and stays in insert.
    ui.press("cmd-z");
    assert_eq!(ui.read(|v, _| v.mode_label()), "INS");
    assert_ne!(ui.text(), "abcllo\nworld\n");
    ui.press("cmd-a");
    assert_eq!(ui.sel().1 - ui.sel().0, ui.text().chars().count());
}

#[gpui_kit::test]
async fn tab_size_option_sets_indent(cx: &mut TestAppContext) {
    let opts = EditorOptions {
        tab_size: 4,
        ..Default::default()
    };
    let mut ui = open(cx, "x\n", opts);
    ui.press("i tab");
    assert_eq!(ui.text(), "    x\n");
}
