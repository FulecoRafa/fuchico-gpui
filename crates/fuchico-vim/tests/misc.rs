mod common;
use common::*;
use fuchico_vim::{Effect, KeyEvent, Key, Modifiers};

#[test]
fn prompt_typing_by_keys() {
    let mut e = ed("a\nb\nc");
    e.handle_keys(":23");
    assert_eq!(e.prompt_value(), Some("23"));
    e.handle_keys("<BS>");
    assert_eq!(e.prompt_value(), Some("2"));
    e.handle_keys("<CR>");
    assert_eq!(e.line_col(), (2, 1));
}

#[test]
fn backspace_on_empty_prompt_closes() {
    let mut e = ed("abc");
    e.handle_keys("/");
    e.handle_keys("<BS>");
    assert_eq!(e.prompt_kind(), None);
    e.handle_keys(":");
    e.handle_keys("<Esc>");
    assert_eq!(e.prompt_kind(), None);
}

#[test]
fn prompt_status_effect() {
    let mut e = ed("abc");
    let effs = e.handle_keys(":");
    assert!(has(&effs, |f| matches!(f, Effect::StatusChanged)));
    let effs = e.prompt_close(false);
    assert!(has(&effs, |f| matches!(f, Effect::StatusChanged)));
}

#[test]
fn unhandled_keys() {
    let mut e = ed("abc");
    let effs = e.handle_key(KeyEvent::new(Key::F(5), Modifiers::default()));
    assert!(has(&effs, |f| matches!(f, Effect::Unhandled)));
    // unknown printable keys are swallowed in normal mode
    let effs = e.handle_keys("Z");
    assert!(!has(&effs, |f| matches!(f, Effect::Unhandled)));
}

#[test]
fn multiline_paste_cursor() {
    let (t, c) = run_c("a\nb\nc", "yjGp");
    assert_eq!(t, "a\nb\nc\na\nb");
    assert_eq!(c, (4, 1));
    // cm-vim has no gp
    assert_eq!(run("abc", "ylgp"), "abc");
}

#[test]
fn visual_dot_repeat() {
    assert_eq!(run("abcdef", "vld."), "ef");
    assert_eq!(run("a\nb\nc\nd", "Vjd."), "");
}

#[test]
fn count_insert_newline() {
    assert_eq!(run("x", "3ox<Esc>"), "x\nx\nx\nx");
    assert_eq!(run("x", "2Ay<Esc>"), "xyy");
}

#[test]
fn change_dot_repeat_counts() {
    assert_eq!(run("a b c d", "cwX<Esc>w2."), "X X d");
}

#[test]
fn undo_cursor_position() {
    let (t, c) = run_c("foo bar baz", "wdwu");
    assert_eq!(t, "foo bar baz");
    assert_eq!(c, (1, 5));
}


#[test]
fn pending_operator_survives_set_text() {
    // `x` is the operatorMotion delete: after a pending `d` it acts as `dd`
    let mut e = ed("abc\ndef");
    e.handle_keys("d");
    e.set_text("xyz\nuvw");
    e.handle_keys("x");
    assert_eq!(e.text(), "uvw");
}

#[test]
fn clipboard_register() {
    let mut e = ed("foo");
    e.handle_keys("\"+yiw");
    assert_eq!(e.register('+').as_deref(), Some("foo"));
}

#[test]
fn macro_with_ex_command() {
    let mut e = ed("a\nb\nc");
    // cm-vim logs only `:` itself (the dialog keys are not vim keys), so
    // the replay opens the prompt and moves down; the Ex line is lost
    e.handle_keys("qq:s/$/!/<CR>jq");
    assert_eq!(e.register('q').as_deref(), Some(":j"));
    e.handle_keys("@q");
    assert_eq!(e.text(), "a!\nb\nc");
    // `@:` replays the last Ex command
    e.prompt_close(false);
    e.handle_keys("@:");
    assert_eq!(e.text(), "a!\nb\nc!");
}

#[test]
fn search_offset_line() {
    let mut e = ed("foo\nbar\nbaz");
    e.handle_keys("/baz<CR>");
    assert_eq!(e.line_col(), (3, 1));
    // clipToLine: the exclusive motion stops at the end of line 2
    e.handle_keys("ggdn");
    assert_eq!(e.text(), "\nbaz");
}

#[test]
fn star_sets_register() {
    let mut e = ed("foo bar foo");
    e.handle_keys("*");
    assert_eq!(e.register('/').as_deref(), Some("\\bfoo\\b"));
}

#[test]
fn block_cursors_visual() {
    let mut e = ed("abc");
    e.handle_keys("vl");
    assert_eq!(e.block_cursors(), vec![1]);
}
