//! h/j/k/l, word motions, line motions, goto, find/till, half page.

use super::*;
use crate::{MemoryHost, Mode};

// ---- h / l -----------------------------------------------------------------

#[test]
fn right_and_left() {
    check("he|l^lo", "l", "hel|l^o");
    check("he|l^lo", "h", "h|e^llo");
    check("|h^ello", "h", "|h^ello");
    check("he|l^lo", "<ArrowRight>", "hel|l^o");
    check("he|l^lo", "<ArrowLeft>", "h|e^llo");
}

#[test]
fn right_off_the_end_lands_on_doc_end() {
    // the internal cursor moves to doc.len; internalSelToCM keeps it empty
    check("hell|o^", "l", "hello|");
    check("hello|", "l", "hello|");
}

#[test]
fn right_crosses_lines() {
    check("a|b^\ncd", "l", "ab|\n^cd");
    check("ab|\n^cd", "l", "ab\n|c^d");
    check("ab\n|c^d", "h", "ab|\n^cd");
}

#[test]
fn counts_on_char_motions() {
    check("|h^ello world", "3l", "hel|l^o world");
    check("hello wor|l^d", "4h", "hello| ^world");
}

#[test]
fn count_is_reset_after_char_motion() {
    let mut e = ed("|h^ello world");
    e.handle_keys("2l");
    assert_eq!(e.pending_status(), "");
    e.handle_keys("l");
    assert_eq!(render(&e), "hel|l^o world");
}

#[test]
fn select_mode_extends() {
    check("|h^ello", "vll", "^hel|lo");
    check("hel|l^o", "vhh", "h|ell^o");
    // crossing the anchor flips the range around the anchor char
    check("he|l^lo", "vhl", "he|l^lo");
    check("he|l^lo", "vlhh", "h|el^lo");
}

#[test]
fn moving_collapses_selection_in_normal_mode() {
    check("^hel|lo", "l", "hel|l^o");
    check("^hel|lo", "h", "h|e^llo");
}

#[test]
fn multiple_cursors_move_together() {
    check("¦a°bc |d^ef", "l", "a¦b°c d|e^f");
}

// ---- j / k -----------------------------------------------------------------

#[test]
fn down_and_up() {
    check("a|b^c\nde\nfgh", "j", "abc\nd|e^\nfgh");
    check("a|b^c\nde\nfgh", "jj", "abc\nde\nf|g^h");
    check("abc\nde\nf|g^h", "k", "abc\nd|e^\nfgh");
    check("a|b^c\nde", "k", "a|b^c\nde");
    check("abc\nd|e^", "j", "abc\nd|e^");
    check("a|b^c\nde\nfgh", "2j", "abc\nde\nf|g^h");
    check("a|b^c\nde\nfgh", "<ArrowDown>", "abc\nd|e^\nfgh");
    check("abc\nd|e^", "<ArrowUp>", "a|b^c\nde");
}

#[test]
fn down_to_shorter_line_lands_on_line_end() {
    check("ab|c^\nd\nefg", "j", "abc\nd|\n^efg");
}

#[test]
fn single_cursor_forgets_goal_column_quirk() {
    // mapSel with one range uses EditorSelection.single, which drops the
    // goal column: the second j uses the clamped column.
    check("ab|c^\nd\nefg", "jj", "abc\nd\ne|f^g");
}

#[test]
fn multiple_cursors_keep_goal_column() {
    check("ab|c^\nd\nefg\n¦x°", "jj", "abc\nd\nef|g^\n¦x°");
}

#[test]
fn select_mode_down() {
    check("a|b^c\ndef", "vj", "a^bc\nde|f");
}

#[test]
fn count_larger_than_document() {
    check("a|b^c\nde", "9j", "abc\nd|e^");
    check("abc\nd|e^", "9k", "a|b^c\nde");
}

// ---- words -----------------------------------------------------------------

#[test]
fn word_forward() {
    check("|h^ello world", "w", "^hello| world");
    check("^hello| world", "w", "hello^ world|");
    check("he|l^lo world", "w", "he^llo| world");
}

#[test]
fn e_is_w_quirk() {
    check("|h^ello world", "e", "^hello| world");
}

#[test]
fn word_backward() {
    check("hello wor|l^d", "b", "hello |worl^d");
    check("hello |world^", "b", "|hello ^world");
}

#[test]
fn word_count_is_not_consumed_quirk() {
    let mut e = ed("|h^ello world foo");
    e.handle_keys("2w");
    assert_eq!(render(&e), "^hello| world foo");
    assert_eq!(e.pending_status(), "2");
}

#[test]
fn word_punctuation_groups() {
    check("|f^oo.bar", "w", "^foo|.bar");
    check("^foo|.bar", "w", "foo^.|bar");
}

#[test]
fn word_select_mode_keeps_anchor() {
    check("|h^ello world", "vww", "^hello world|");
}

#[test]
fn long_words_are_extensions() {
    check_ext("|f^oo.bar baz", "W", "^foo.bar| baz");
    check_ext("foo.bar ba|z^", "B", "foo.bar |baz^");
    // without extensions, W is unbound: typed chars are swallowed
    let mut e = ed("|f^oo.bar baz");
    let fx = e.handle_keys("W");
    assert!(!fx.contains(&Effect::Unhandled));
    assert_eq!(render(&e), "|f^oo.bar baz");
}

// ---- line start / end ------------------------------------------------------

#[test]
fn line_end_and_start() {
    check("a|b^c\ndef", "gl", "ab|c^\ndef");
    check("ab|c^\ndef", "gh", "|a^bc\ndef");
    check("a|b^c\ndef", "<End>", "ab|c^\ndef");
    check("ab|c^\ndef", "<Home>", "|a^bc\ndef");
}

#[test]
fn line_end_select_mode() {
    check("|a^bc\ndef", "vgl", "^abc|\ndef");
}

#[test]
fn line_end_on_empty_line() {
    check("abc\n|\n^def", "gl", "abc\n|\n^def");
}

#[test]
fn first_non_blank() {
    check("   ab|c^", "gs", "   |a^bc");
    check("|a^bc", "gs", "|a^bc");
    check("   |", "gs", "| ^  ");
}

#[test]
fn goto_returns_to_normal() {
    let mut e = ed("a|b^c");
    e.handle_keys("g");
    assert_eq!(e.pending_status(), "g");
    e.handle_keys("l");
    assert_eq!(e.pending_status(), "");
}

#[test]
fn goto_doc_start_and_last_line() {
    check("abc\nd|e^f", "gg", "|a^bc\ndef");
    check("a|b^c\ndef", "ge", "abc\n|d^ef");
    // trailing empty line: goes to the line before it
    check("a|b^c\ndef\n", "ge", "abc\n|d^ef\n");
    check("a|b^c", "ge", "|a^bc");
}

#[test]
fn goto_last_line_select_mode() {
    check("a|b^c\ndef", "vge", "a^bc\nd|ef");
}

#[test]
fn goto_down_up() {
    check("a|b^c\ndef", "gj", "abc\nd|e^f");
    check("abc\nd|e^f", "gk", "a|b^c\ndef");
}

#[test]
fn goto_unknown_char_cancels_prefix() {
    // cm-helix `unhandledCommandsFilter`: typing in a minor mode resets it
    let mut e = ed("a|b^c");
    let fx = e.handle_keys("gx");
    assert!(!fx.contains(&Effect::Unhandled));
    assert_eq!(e.pending_status(), "");
    assert_eq!(render(&e), "a|b^c");
}

#[test]
fn unbound_non_char_key_is_unhandled() {
    let mut e = ed("a|b^c");
    assert!(e.handle_keys("<F5>").contains(&Effect::Unhandled));
    let mut e = ed("a|b^c");
    assert!(e.handle_keys("g<F5>").contains(&Effect::Unhandled));
    assert_eq!(e.pending_status(), "g");
}

#[test]
fn window_motions_extension() {
    let host = MemoryHost {
        visible: Some((2, 4)),
        ..Default::default()
    };
    let mut e = Editor::with_host("l1\nl2\nl3\nl4\nl5", Config::default(), Box::new(host));
    e.handle_keys("gt");
    assert_eq!(render(&e), "l1\n|l^2\nl3\nl4\nl5");
    e.handle_keys("gb");
    assert_eq!(render(&e), "l1\nl2\nl3\n|l^4\nl5");
    e.handle_keys("gc");
    assert_eq!(render(&e), "l1\nl2\n|l^3\nl4\nl5");
}

// ---- find / till -------------------------------------------------------------

#[test]
fn find_forward_inclusive() {
    check("|a^bcabc", "fc", "^abc|abc");
    check("|a^bcabc", "2fc", "^abcabc|");
    check("|a^bcabc", "fz", "|a^bcabc");
}

#[test]
fn till_forward() {
    check("|a^bcabc", "tc", "^ab|cabc");
}

#[test]
fn find_backward() {
    check("abcab|c^", "Fa", "abc|abc^");
    check("abcab|c^", "Ta", "abca|bc^");
    check("abcab|c^", "2Fa", "|abcabc^");
}

#[test]
fn find_select_mode_keeps_anchor() {
    check("|a^bcabc", "vfcfc", "^abcabc|");
}

#[test]
fn find_status_and_escape() {
    let mut e = ed("|a^bc");
    e.handle_keys("f");
    assert_eq!(e.pending_status(), "f");
    e.handle_keys("<esc>");
    assert_eq!(e.pending_status(), "");
    e.handle_keys("c");
    // `c` was a change command, not a find target
    assert_eq!(e.mode().ty, crate::ModeType::Insert);
}

#[test]
fn find_crosses_lines() {
    check("|a^b\ncd", "fd", "^ab\ncd|");
}

// ---- half page -----------------------------------------------------------------

#[test]
fn half_page_down_up() {
    let text: String = (1..=10).map(|i| format!("line{i}\n")).collect();
    let host = MemoryHost {
        half_page: Some(3),
        ..Default::default()
    };
    let mut e = Editor::with_host(&text, Config::parity(), Box::new(host));
    e.handle_keys("<C-d>");
    assert_eq!(e.line_col().0, 4);
    e.handle_keys("<PageDown>");
    assert_eq!(e.line_col().0, 7);
    e.handle_keys("<C-u>");
    assert_eq!(e.line_col().0, 4);
    e.handle_keys("<PageUp><PageUp>");
    assert_eq!(e.line_col().0, 1);
}

#[test]
fn mode_reset_after_select_motion() {
    let mut e = ed("|a^bc");
    e.handle_keys("v");
    assert_eq!(e.mode(), Mode::SELECT);
    e.handle_keys("l");
    assert_eq!(e.mode(), Mode::SELECT);
    e.handle_keys("v");
    assert_eq!(e.mode(), Mode::NORMAL);
}
