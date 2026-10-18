//! `@codemirror/search` parity: the find/replace panel, F3 / Mod-g,
//! Mod-Shift-l, Mod-d, Mod-Alt-g, Mod-u / Mod-Shift-u, Alt-drag
//! rectangular selection, `highlightSelectionMatches` and the outline
//! overlay.

mod common;

use common::*;
use fuchico_editor::{EditorOptions, EngineKind, FindField};
use gpui_kit::{
    Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, point, px,
};

fn normal() -> EditorOptions {
    EditorOptions {
        engine: EngineKind::Normal,
        ..Default::default()
    }
}

fn center(ui: &mut Ui, sel: &'static str) -> Point<Pixels> {
    ui.frame();
    ui.cx
        .debug_bounds(sel)
        .unwrap_or_else(|| panic!("no element {sel}"))
        .center()
}

fn click_el(ui: &mut Ui, sel: &'static str) {
    let at = center(ui, sel);
    ui.click(at, Modifiers::none());
}

/// Left edge (plus a hair) of the char at `pos`, vertically centred.
fn char_left(ui: &mut Ui, pos: usize) -> Point<Pixels> {
    ui.frame();
    let b = ui
        .read(|v, _| v.bounds_for_char(pos))
        .expect("visible char");
    point(b.origin.x + px(1.), b.center().y)
}

fn ranges(ui: &mut Ui) -> Vec<(usize, usize)> {
    ui.read(|v, _| {
        v.selection()
            .ranges()
            .iter()
            .map(|r| (r.from(), r.to()))
            .collect()
    })
}

#[gpui_kit::test]
async fn panel_typing_enter_and_buttons(cx: &mut TestAppContext) {
    let mut ui = open(cx, "one two one three one\n", normal());
    ui.press("cmd-f");
    // Empty selection, first open: empty query, search field focused.
    assert_eq!(ui.read(|v, _| v.find_query()), Some((String::new(), 0, 0)));
    assert_eq!(ui.read(|v, _| v.focused_field()), Some(FindField::Search));
    assert!(ui.cx.debug_bounds("find-replace").is_some());
    ui.type_text("one");
    assert_eq!(ui.read(|v, _| v.find_query()), Some(("one".into(), 0, 3)));
    assert_eq!(ui.sel(), (0, 0), "typing doesn't move the selection");
    // Search matches are painted while the panel is open.
    let hl = ui.read(|v, _| v.debug_search_highlights());
    assert_eq!(hl.len(), 3);
    click_el(&mut ui, "find-next");
    assert_eq!(ranges(&mut ui), vec![(0, 3)]);
    click_el(&mut ui, "find-next");
    assert_eq!(ranges(&mut ui), vec![(8, 11)]);
    let hl = ui.read(|v, _| v.debug_search_highlights());
    assert!(
        hl.contains(&(8..11, true)) && hl.contains(&(0..3, false)),
        "{hl:?}"
    );
    click_el(&mut ui, "find-prev");
    assert_eq!(ranges(&mut ui), vec![(0, 3)]);
    click_el(&mut ui, "find-all");
    assert_eq!(ranges(&mut ui), vec![(0, 3), (8, 11), (18, 21)]);
    click_el(&mut ui, "find-close");
    assert_eq!(ui.read(|v, _| v.find_query()), None);
    assert!(ui.read(|v, _| v.debug_search_highlights()).is_empty());
}

#[gpui_kit::test]
async fn replace_next_and_replace_all(cx: &mut TestAppContext) {
    let mut ui = open(cx, "one two one three one\n", normal());
    ui.press("cmd-f");
    ui.type_text("one");
    // Tab moves to the replace field.
    ui.press("tab");
    assert_eq!(ui.read(|v, _| v.focused_field()), Some(FindField::Replace));
    ui.type_text("1");
    assert_eq!(ui.read(|v, _| v.search_query().replace.clone()), "1");
    // First replace only selects the next match...
    click_el(&mut ui, "find-replace-one");
    assert_eq!(ui.text(), "one two one three one\n");
    assert_eq!(ranges(&mut ui), vec![(0, 3)]);
    // ...then replaces it and selects the following one.
    click_el(&mut ui, "find-replace-one");
    assert_eq!(ui.text(), "1 two one three one\n");
    assert_eq!(ranges(&mut ui), vec![(6, 9)]);
    // Enter in the replace field is `replaceNext`.
    ui.press("enter");
    assert_eq!(ui.text(), "1 two 1 three one\n");
    assert_eq!(ranges(&mut ui), vec![(14, 17)]);
    click_el(&mut ui, "find-replace-all");
    assert_eq!(ui.text(), "1 two 1 three 1\n");
    // Replacements are undo steps (replace-all is one step).
    click_el(&mut ui, "find-close");
    ui.press("cmd-z");
    assert_eq!(ui.text(), "1 two 1 three one\n");
    ui.press("cmd-z");
    assert_eq!(ui.text(), "1 two one three one\n");
}

#[gpui_kit::test]
async fn toggles_case_regexp_word(cx: &mut TestAppContext) {
    let mut ui = open(cx, "Cat cat concat c.t\n", normal());
    ui.press("cmd-f");
    ui.type_text("cat");
    assert_eq!(ui.read(|v, _| v.find_query()).unwrap().2, 3);
    click_el(&mut ui, "find-case");
    assert!(ui.read(|v, _| v.search_query().case_sensitive));
    assert_eq!(ui.read(|v, _| v.find_query()).unwrap().2, 2);
    click_el(&mut ui, "find-word");
    assert!(ui.read(|v, _| v.search_query().whole_word));
    assert_eq!(ui.read(|v, _| v.find_query()).unwrap().2, 1);
    click_el(&mut ui, "find-word");
    click_el(&mut ui, "find-case");
    // Regexp: `c.t` matches every cat plus the literal `c.t`.
    let at = center(&mut ui, "find-search");
    ui.click(at, Modifiers::none());
    ui.press("cmd-a");
    ui.type_text("c.t");
    assert_eq!(ui.read(|v, _| v.search_query().search.clone()), "c.t");
    assert_eq!(ui.read(|v, _| v.find_query()).unwrap().2, 1);
    click_el(&mut ui, "find-regexp");
    assert!(ui.read(|v, _| v.search_query().regexp));
    assert_eq!(ui.read(|v, _| v.find_query()).unwrap().2, 4);
}

#[gpui_kit::test]
async fn f3_and_mod_g_work_with_the_panel_closed(cx: &mut TestAppContext) {
    let mut ui = open(cx, "ab x ab y ab\n", normal());
    // No query yet: findNext opens the panel.
    ui.press("f3");
    assert!(ui.read(|v, _| v.search_panel()).is_some());
    ui.type_text("ab");
    ui.press("escape");
    assert!(ui.read(|v, _| v.search_panel()).is_none());
    assert_eq!(ui.read(|v, _| v.focused_field()), None);
    ui.press("cmd-g");
    assert_eq!(ranges(&mut ui), vec![(0, 2)]);
    ui.press("f3");
    assert_eq!(ranges(&mut ui), vec![(5, 7)]);
    ui.press("cmd-shift-g");
    assert_eq!(ranges(&mut ui), vec![(0, 2)]);
    // findPrevious wraps around.
    ui.press("shift-f3");
    assert_eq!(ranges(&mut ui), vec![(10, 12)]);
    // Reopening seeds from the (non-empty) selection; the query persists.
    ui.press("cmd-f");
    assert_eq!(ui.read(|v, _| v.find_query()), Some(("ab".into(), 2, 3)));
}

#[gpui_kit::test]
async fn escape_in_editor_closes_panel_for_normal_engine(cx: &mut TestAppContext) {
    let mut ui = open(cx, "abc\n", normal());
    ui.press("cmd-f");
    // Clicking the text gives the keyboard back to the editor.
    let at = ui.char_point(1);
    ui.click(at, Modifiers::none());
    assert_eq!(ui.read(|v, _| v.focused_field()), None);
    assert!(ui.read(|v, _| v.search_panel()).is_some());
    ui.type_text("Z");
    assert!(ui.text().contains('Z'), "editor typing: {}", ui.text());
    ui.press("escape");
    assert!(ui.read(|v, _| v.search_panel()).is_none());
}

#[gpui_kit::test]
async fn select_selection_matches_mod_shift_l(cx: &mut TestAppContext) {
    let mut ui = open(cx, "foo bar foo baz foo\n", normal());
    ui.update(|v, _, cx| v.set_selection(8, 11, cx));
    ui.press("cmd-shift-l");
    assert_eq!(ranges(&mut ui), vec![(0, 3), (8, 11), (16, 19)]);
    assert_eq!(ui.read(|v, _| v.selection().main_index()), 1);
}

#[gpui_kit::test]
async fn select_next_occurrence_mod_d(cx: &mut TestAppContext) {
    let mut ui = open(cx, "foo bar foo foo\n", normal());
    ui.update(|v, _, cx| v.set_selection(1, 1, cx));
    // Empty range: select the word.
    ui.press("cmd-d");
    assert_eq!(ranges(&mut ui), vec![(0, 3)]);
    ui.press("cmd-d");
    assert_eq!(ranges(&mut ui), vec![(0, 3), (8, 11)]);
    ui.press("cmd-d");
    assert_eq!(ranges(&mut ui), vec![(0, 3), (8, 11), (12, 15)]);
    // All found: nothing more.
    ui.press("cmd-d");
    assert_eq!(ranges(&mut ui).len(), 3);
}

#[gpui_kit::test]
async fn goto_line_dialog(cx: &mut TestAppContext) {
    let text = "l1\nl2\nline three\nl4\n";
    let mut ui = open(cx, text, normal());
    ui.press("cmd-alt-g");
    assert_eq!(ui.read(|v, _| v.goto_dialog()), Some("1".into()));
    assert!(ui.cx.debug_bounds("goto-input").is_some());
    // Prefilled text is selected: typing replaces it.
    ui.type_text("3:5");
    assert_eq!(ui.read(|v, _| v.goto_dialog()), Some("3:5".into()));
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.goto_dialog()), None);
    // `line.from + min(col, length)`: col is a 0-based offset in CM.
    assert_eq!(ui.sel(), (11, 11));
    // `go` button, relative line.
    ui.press("cmd-alt-g");
    ui.type_text("-2");
    click_el(&mut ui, "goto-go");
    assert_eq!(ui.read(|v, _| v.goto_dialog()), None);
    assert_eq!(ui.read(|v, _| v.line_col().0), 1);
    // Escape and `×` close without moving.
    ui.press("cmd-alt-g");
    ui.press("escape");
    assert_eq!(ui.read(|v, _| v.goto_dialog()), None);
    ui.press("cmd-alt-g");
    click_el(&mut ui, "goto-close");
    assert_eq!(ui.read(|v, _| v.goto_dialog()), None);
    assert_eq!(ui.read(|v, _| v.line_col().0), 1);
}

#[gpui_kit::test]
async fn undo_and_redo_selection(cx: &mut TestAppContext) {
    let mut ui = open(cx, "hello world again\n", normal());
    let a = ui.char_point(3);
    ui.click(a, Modifiers::none());
    let b = ui.char_point(9);
    ui.click(b, Modifiers::none());
    let after = ui.sel();
    ui.press("cmd-u");
    let mid = ui.sel();
    assert_ne!(mid, after);
    ui.press("cmd-u");
    assert_eq!(ui.sel(), (0, 0));
    ui.press("cmd-shift-u");
    assert_eq!(ui.sel(), mid);
    ui.press("cmd-shift-u");
    assert_eq!(ui.sel(), after);
    // A drag is one step.
    let (f, t) = (ui.char_point(0), ui.char_point(5));
    ui.drag(f, t);
    let dragged = ui.sel();
    assert_ne!(dragged, after);
    ui.press("cmd-u");
    assert_eq!(ui.sel(), after);
}

#[gpui_kit::test]
async fn undo_selection_undoes_changes_too(cx: &mut TestAppContext) {
    let mut ui = open(cx, "abc\n", normal());
    ui.type_text("xy");
    assert_eq!(ui.text(), "xyabc\n");
    ui.press("cmd-u");
    assert_eq!(ui.text(), "abc\n");
}

#[gpui_kit::test]
async fn alt_drag_rectangular_selection(cx: &mut TestAppContext) {
    let mut ui = open(cx, "abcdef\nab\nabcdef\n", normal());
    let from = char_left(&mut ui, 1);
    let to = char_left(&mut ui, 14);
    let alt = Modifiers::alt();
    ui.cx.simulate_mouse_move(from, None, alt);
    ui.cx.simulate_mouse_down(from, MouseButton::Left, alt);
    ui.frame();
    ui.cx.simulate_mouse_move(to, Some(MouseButton::Left), alt);
    ui.cx.simulate_mouse_up(to, MouseButton::Left, alt);
    ui.cx.run_until_parked();
    ui.frame();
    // Columns 1..4 on each line; the short line is clipped to its end.
    assert_eq!(ranges(&mut ui), vec![(1, 4), (8, 9), (11, 14)]);
}

#[gpui_kit::test]
async fn highlight_selection_matches(cx: &mut TestAppContext) {
    let mut ui = open(cx, "foo bar foo baz foo\n", normal());
    assert!(ui.read(|v, _| v.debug_selection_matches()).is_empty());
    ui.update(|v, _, cx| v.set_selection(0, 3, cx));
    ui.frame();
    assert_eq!(
        ui.read(|v, _| v.debug_selection_matches()),
        vec![8..11, 16..19]
    );
    // Multiple ranges: no highlight.
    ui.press("cmd-shift-l");
    ui.frame();
    assert!(ui.read(|v, _| v.debug_selection_matches()).is_empty());
}

fn outline_doc(n: usize) -> String {
    (1..=n).map(|i| format!("## Head {i}\nbody\n")).collect()
}

#[gpui_kit::test]
async fn outline_clamps_and_selects_heading_line(cx: &mut TestAppContext) {
    let mut ui = open(cx, "# Intro\nx\n## Setup\ny\n", normal());
    ui.press("cmd-o");
    assert_eq!(ui.read(|v, _| v.outline_active()), Some(0));
    ui.press("up");
    assert_eq!(
        ui.read(|v, _| v.outline_active()),
        Some(0),
        "clamped at top"
    );
    ui.press("down");
    ui.press("down");
    ui.press("down");
    assert_eq!(
        ui.read(|v, _| v.outline_active()),
        Some(1),
        "clamped at bottom"
    );
    // L<line> labels and the 14px indent per level.
    let l0 = ui.cx.debug_bounds("outline-line-0");
    assert!(l0.is_some());
    ui.press("enter");
    assert_eq!(ui.read(|v, _| v.outline_items()), None);
    assert_eq!(ranges(&mut ui), vec![(10, 18)], "the whole `## Setup` line");
}

#[gpui_kit::test]
async fn outline_indent_hover_click_and_backdrop(cx: &mut TestAppContext) {
    let mut ui = open(cx, "# Intro\n## Setup\n### Deep\n", normal());
    ui.press("cmd-o");
    let b0 = ui.cx.debug_bounds("outline-item-0").unwrap();
    let b1 = ui.cx.debug_bounds("outline-item-1").unwrap();
    let h0 = ui.cx.debug_bounds("outline-line-0").unwrap();
    assert!(b0.size.height > px(0.) && b1.origin.y > b0.origin.y);
    assert!(h0.origin.x > b0.center().x, "L label on the right");
    // 14px indent per heading level.
    let x0 = ui.cx.debug_bounds("outline-hash-0").unwrap().origin.x;
    let x1 = ui.cx.debug_bounds("outline-hash-1").unwrap().origin.x;
    let x2 = ui.cx.debug_bounds("outline-hash-2").unwrap().origin.x;
    assert_eq!((x1 - x0, x2 - x0), (px(14.), px(28.)));
    // Hover selects.
    let at = center(&mut ui, "outline-item-2");
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    ui.frame();
    assert_eq!(ui.read(|v, _| v.outline_active()), Some(2));
    // Clicking a row picks it.
    ui.click(at, Modifiers::none());
    assert_eq!(ui.read(|v, _| v.outline_items()), None);
    assert_eq!(ui.read(|v, _| v.line_col().0), 3);
    // Clicking the backdrop closes without moving.
    ui.press("cmd-o");
    let bd = ui.cx.debug_bounds("outline-backdrop").unwrap();
    ui.click(
        point(bd.origin.x + px(5.), bd.origin.y + px(5.)),
        Modifiers::none(),
    );
    assert_eq!(ui.read(|v, _| v.outline_items()), None);
    assert_eq!(ui.read(|v, _| v.line_col().0), 3);
}

#[gpui_kit::test]
async fn outline_lists_all_items_and_scrolls(cx: &mut TestAppContext) {
    let mut ui = open(cx, &outline_doc(30), normal());
    ui.press("cmd-o");
    assert_eq!(ui.read(|v, _| v.outline_items()).unwrap().len(), 30);
    let panel = ui.cx.debug_bounds("fuchico-editor-outline").unwrap();
    // max-height: 60% of the editor (700px window).
    assert!(panel.size.height <= px(420.5), "{:?}", panel.size);
    let last = ui
        .cx
        .debug_bounds("outline-item-29")
        .expect("all rows render");
    assert!(
        last.origin.y > panel.origin.y + panel.size.height,
        "overflowing"
    );
    let before = ui.cx.debug_bounds("outline-item-0").unwrap().origin.y;
    let at = center(&mut ui, "outline-item-3");
    ui.cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(-200.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    ui.cx.run_until_parked();
    ui.frame();
    let after = ui.cx.debug_bounds("outline-item-0").unwrap().origin.y;
    assert!(after < before, "list scrolled: {before:?} -> {after:?}");
    // The wheel stops at the overlay: the editor didn't scroll.
    assert_eq!(ui.read(|v, _| v.scroll_position()), 0.);
}

#[gpui_kit::test]
async fn outline_empty_state(cx: &mut TestAppContext) {
    let mut ui = open(cx, "no headings here\n", normal());
    ui.press("cmd-o");
    assert!(ui.cx.debug_bounds("outline-empty").is_some());
    ui.press("escape");
    let mut ui2 = open(cx, "# Alpha\n", normal());
    ui2.press("cmd-o");
    ui2.type_text("zzz");
    assert_eq!(ui2.read(|v, _| v.outline_items()), Some(vec![]));
    assert!(ui2.cx.debug_bounds("outline-empty").is_some());
    // A blank query lists everything (`query.trim()`).
    ui2.press("backspace");
    ui2.press("backspace");
    ui2.press("backspace");
    ui2.type_text(" ");
    assert_eq!(ui2.read(|v, _| v.outline_items()).unwrap().len(), 1);
}

#[gpui_kit::test]
async fn helix_block_cursor_seeds_and_highlights(cx: &mut TestAppContext) {
    // With the Helix engine the 1-char block is a real selection, as in
    // codemirror-helix: Mod-f seeds it and same-text chars are marked.
    let mut ui = open(cx, "a b a\n", EditorOptions::default());
    ui.frame();
    assert_eq!(ui.read(|v, _| v.debug_selection_matches()), vec![4..5]);
    ui.press("cmd-f");
    assert_eq!(ui.read(|v, _| v.find_query()), Some(("a".into(), 0, 2)));
    // Escape belongs to the panel while it has the keyboard.
    ui.press("escape");
    assert!(ui.read(|v, _| v.search_panel()).is_none());
    assert_eq!(ui.read(|v, _| v.mode_label()), "NOR");
}
