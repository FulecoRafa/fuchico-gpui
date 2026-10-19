mod common;
use common::*;
use fuchico_vim::{CursorShape, Effect, ModeType};

#[test]
fn insert_variants() {
    assert_eq!(run("abc", "ix<Esc>"), "xabc");
    assert_eq!(run("abc", "ax<Esc>"), "axbc");
    assert_eq!(run("abc", "Ax<Esc>"), "abcx");
    assert_eq!(run("  abc", "$Ix<Esc>"), "  xabc");
    assert_eq!(run("  abc", "$gIx<Esc>"), "x  abc");
    assert_eq!(run("a\nb", "ox<Esc>"), "a\nx\nb");
    assert_eq!(run("a\nb", "Ox<Esc>"), "x\na\nb");
    assert_eq!(run("a\nb", "jOx<Esc>"), "a\nx\nb");
}

#[test]
fn insert_cursor_after_esc() {
    assert_eq!(cur("abc", "Axy<Esc>"), (1, 5));
    assert_eq!(cur("abc", "i<Esc>"), (1, 1));
    assert_eq!(cur("abc", "a<Esc>"), (1, 1));
}

#[test]
fn insert_keys() {
    assert_eq!(run("abc", "A<BS><BS><Esc>"), "a");
    assert_eq!(run("foo bar", "A<C-w><Esc>"), "foo ");
    assert_eq!(run("foo bar", "A<C-u><Esc>"), "");
    assert_eq!(run("ab", "ax<CR>y<Esc>"), "ax\nyb");
    assert_eq!(run("a", "A<C-c>"), "a");
}

#[test]
fn insert_text_api() {
    let mut e = ed("abc");
    e.handle_keys("A");
    e.insert_text(" hello");
    e.handle_keys("<Esc>");
    assert_eq!(e.text(), "abc hello");
    e.handle_keys("u");
    assert_eq!(e.text(), "abc");
}

#[test]
fn insert_text_in_normal_runs_keys() {
    let mut e = ed("abc");
    e.insert_text("x");
    assert_eq!(e.text(), "bc");
}

#[test]
fn ctrl_o() {
    let mut e = ed("abc def");
    e.handle_keys("i<C-o>w");
    assert_eq!(e.mode_label(), "INSERT");
    assert_eq!(e.line_col(), (1, 5));
    e.handle_keys("X<Esc>");
    assert_eq!(e.text(), "abc Xdef");
}

#[test]
fn mode_effects() {
    let mut e = ed("abc");
    let effs = e.handle_keys("i");
    assert!(has(&effs, |f| matches!(
        f,
        Effect::ModeChanged { from: ModeType::Normal, to: ModeType::Insert }
    )));
    assert_eq!(e.cursor_shape(), CursorShape::Bar);
    e.handle_keys("<Esc>");
    assert_eq!(e.cursor_shape(), CursorShape::Block);
    e.handle_keys("d");
    assert_eq!(e.cursor_shape(), CursorShape::Half);
    assert_eq!(e.pending_status(), "d");
    e.handle_keys("<Esc>");
    assert_eq!(e.pending_status(), "");
}

#[test]
fn doc_changed_effect() {
    let mut e = ed("abc");
    let effs = e.handle_keys("x");
    assert!(has(&effs, |f| matches!(f, Effect::DocChanged(_))));
}

#[test]
fn jk_mapping() {
    let mut e = ed("abc");
    e.execute_command("imap jk <Esc>");
    e.handle_keys("ijk");
    assert_eq!(e.mode_label(), "NORMAL");
    assert_eq!(e.text(), "abc");
    // a lone j is typed
    e.handle_keys("ij");
    assert_eq!(e.text(), "jabc");
    e.advance_clock(1000.0);
    e.handle_keys("k<Esc>");
    assert_eq!(e.text(), "jkabc");
}

// ---- visual ----------------------------------------------------------------------

#[test]
fn visual_char() {
    let mut e = ed("foo bar");
    e.handle_keys("v");
    assert_eq!(e.mode_label(), "VISUAL");
    assert_eq!(e.mode_type(), ModeType::Select);
    e.handle_keys("ll");
    let r = e.selection().main();
    assert_eq!((r.anchor, r.head), (0, 3));
    e.handle_keys("d");
    assert_eq!(e.text(), " bar");
    assert_eq!(e.mode_label(), "NORMAL");
}

#[test]
fn visual_ops() {
    assert_eq!(run("foo bar", "veU"), "FOO bar");
    assert_eq!(run("FOO bar", "veu"), "foo bar");
    assert_eq!(run("foo bar", "ve~"), "FOO bar");
    assert_eq!(run("foo bar", "wvecX<Esc>"), "foo X");
    assert_eq!(run("foo bar", "veyP"), "foofoo bar");
    assert_eq!(run("foo bar", "vex"), " bar");
    assert_eq!(run("foo bar", "verx"), "xxx bar");
    assert_eq!(run("a\nb\nc", "vjd"), "\nc");
    assert_eq!(run("a\nb", "vjJ"), "a b");
    assert_eq!(run("foo bar", "yiwwviwp"), "foo foo");
}

#[test]
fn visual_line() {
    let mut e = ed("a\nb\nc");
    e.handle_keys("Vj");
    assert_eq!(e.mode_label(), "VISUAL LINE");
    e.handle_keys("d");
    assert_eq!(e.text(), "c");
    assert_eq!(run("a\nb\nc", "jVy"), "a\nb\nc");
    assert_eq!(run("a\nb\nc", "Vj>"), "  a\n  b\nc");
    assert_eq!(run("a\nb\nc", "jVyP"), "a\nb\nb\nc");
}

#[test]
fn visual_block() {
    let mut e = ed("abc\ndef\nghi");
    e.handle_keys("<C-v>jl");
    assert_eq!(e.mode_label(), "VISUAL BLOCK");
    assert_eq!(e.selection().ranges().len(), 2);
    e.handle_keys("d");
    assert_eq!(e.text(), "c\nf\nghi");
    assert_eq!(run("abc\ndef", "<C-v>jIx<Esc>"), "xabc\nxdef");
    assert_eq!(run("abc\ndef", "<C-v>j$Ax<Esc>"), "abcx\ndefx");
}

#[test]
fn visual_switch_and_reselect() {
    let mut e = ed("foo bar");
    e.handle_keys("vlV");
    assert_eq!(e.mode_label(), "VISUAL LINE");
    e.handle_keys("v");
    assert_eq!(e.mode_label(), "VISUAL");
    e.handle_keys("v");
    assert_eq!(e.mode_label(), "NORMAL");
    e.handle_keys("gv");
    assert_eq!(e.mode_label(), "VISUAL");
    e.handle_keys("<Esc>");
    assert_eq!(e.mode_label(), "NORMAL");
}

#[test]
fn visual_o() {
    let mut e = ed("foo bar");
    e.handle_keys("wvlo");
    let r = e.selection().main();
    assert_eq!((r.anchor, r.head), (6, 4));
}

#[test]
fn visual_text_object() {
    assert_eq!(run("f(a b)", "favi(d"), "f()");
    assert_eq!(run("foo bar baz", "wvawd"), "foo baz");
}

#[test]
fn visual_marks() {
    let mut e = ed("abc\ndef\nghi");
    e.handle_keys("jvl<Esc>gg`<");
    assert_eq!(e.line_col(), (2, 1));
    e.handle_keys("`>");
    assert_eq!(e.line_col(), (2, 2));
}

#[test]
fn host_selection_enters_visual() {
    let mut e = ed("foo bar");
    e.set_selection(fuchico_vim::Selection::single(0, 3));
    assert_eq!(e.mode_label(), "VISUAL");
    e.handle_keys("d");
    assert_eq!(e.text(), " bar");
}
