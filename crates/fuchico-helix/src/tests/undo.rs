//! Undo / redo: checkpoints, insert sessions, counts.

use super::*;

#[test]
fn undo_redo_delete() {
    let mut e = ed("|a^bc");
    e.handle_keys("d");
    assert_eq!(render(&e), "|b^c");
    e.handle_keys("u");
    assert_eq!(render(&e), "|a^bc");
    e.handle_keys("U");
    assert_eq!(render(&e), "|b^c");
    // nothing more to redo
    e.handle_keys("U");
    assert_eq!(render(&e), "|b^c");
}

#[test]
fn undo_restores_selection_before_command() {
    let mut e = ed("|a^bc");
    e.handle_keys("ld");
    assert_eq!(render(&e), "a|c^");
    e.handle_keys("u");
    assert_eq!(render(&e), "a|b^c");
}

#[test]
fn nothing_to_undo() {
    let mut e = ed("|a^bc");
    let fx = e.handle_keys("u");
    assert_eq!(render(&e), "|a^bc");
    assert!(!fx.iter().any(|f| matches!(f, Effect::DocChanged(_))));
    assert!(!e.history().can_undo());
}

#[test]
fn undo_redo_with_counts() {
    let mut e = ed("|a^bcd");
    e.handle_keys("ddd");
    assert_eq!(e.text(), "d");
    e.handle_keys("2u");
    assert_eq!(e.text(), "bcd");
    assert_eq!(e.pending_status(), "");
    e.handle_keys("U");
    assert_eq!(e.text(), "cd");
    e.handle_keys("U");
    assert_eq!(e.text(), "d");
    e.handle_keys("3u");
    assert_eq!(e.text(), "abcd");
}

#[test]
fn undo_count_beyond_history_does_nothing() {
    let mut e = ed("|a^bc");
    e.handle_keys("d5u");
    assert_eq!(e.text(), "bc");
}

#[test]
fn insert_session_is_one_step() {
    let mut e = ed("|a^");
    e.handle_keys("iXY");
    e.insert_text("Z");
    e.handle_keys("<Backspace><Esc>");
    assert_eq!(e.text(), "XYa");
    e.handle_keys("u");
    assert_eq!(e.text(), "a");
    assert_eq!(render(&e), "|a^");
    e.handle_keys("U");
    assert_eq!(e.text(), "XYa");
}

#[test]
fn empty_insert_session_adds_no_checkpoint() {
    let mut e = ed("|a^b");
    e.handle_keys("d");
    e.handle_keys("i<Esc>a<Esc>");
    e.handle_keys("u");
    assert_eq!(e.text(), "ab");
    assert!(!e.history().can_undo());
}

#[test]
fn change_is_one_step() {
    let mut e = ed("^ab|c");
    e.handle_keys("cXY<Esc>");
    assert_eq!(e.text(), "XYc");
    e.handle_keys("u");
    assert_eq!(e.text(), "abc");
}

#[test]
fn open_line_is_one_step() {
    let mut e = ed("|a^");
    e.handle_keys("ob<Esc>");
    assert_eq!(e.text(), "a\nb");
    e.handle_keys("u");
    assert_eq!(e.text(), "a");
}

#[test]
fn new_edit_drops_redo() {
    let mut e = ed("|a^bc");
    e.handle_keys("dd");
    e.handle_keys("u");
    assert_eq!(e.text(), "bc");
    e.handle_keys("d");
    assert_eq!(e.text(), "c");
    e.handle_keys("U");
    assert_eq!(e.text(), "c");
    e.handle_keys("u");
    assert_eq!(e.text(), "bc");
    e.handle_keys("u");
    assert_eq!(e.text(), "abc");
}

#[test]
fn motions_add_no_checkpoint() {
    let mut e = ed("|a^bc");
    e.handle_keys("llhx%");
    assert!(!e.history().can_undo());
}

#[test]
fn host_edits_are_not_undoable() {
    let mut e = ed("|a^bc");
    e.apply_changes(vec![crate::Change::insert(3, "!")]);
    assert_eq!(e.text(), "abc!");
    assert!(!e.history().can_undo());
    e.set_text("xabc!");
    assert_eq!(render(&e), "x|a^bc!");
    assert!(!e.history().can_undo());
}

#[test]
fn paste_and_replace_are_undoable() {
    let mut e = editing::with_reg("|a^", '"', &["XY"]);
    e.handle_keys("p");
    assert_eq!(e.text(), "aXY");
    e.handle_keys("u");
    assert_eq!(e.text(), "a");
    e.handle_keys("rz");
    assert_eq!(e.text(), "z");
    e.handle_keys("u");
    assert_eq!(e.text(), "a");
}
