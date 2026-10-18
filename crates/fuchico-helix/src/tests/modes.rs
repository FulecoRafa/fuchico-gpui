//! Mode state: counts, minor modes, pending status, registers, select mode,
//! unknown keys.

use super::*;
use crate::{Mode, ModeType};

#[test]
fn count_accumulates() {
    let mut e = ed("|a^");
    let fx = e.handle_keys("1");
    assert!(fx.contains(&Effect::StatusChanged));
    e.handle_keys("2");
    assert_eq!(e.pending_status(), "12");
    e.handle_keys("0");
    assert_eq!(e.pending_status(), "120");
    assert_eq!(e.mode().count, Some(120));
}

#[test]
fn lone_zero_is_ignored() {
    let mut e = ed("|a^");
    let fx = e.handle_keys("0");
    assert_eq!(e.pending_status(), "");
    assert!(!fx.contains(&Effect::StatusChanged));
    e.handle_keys("03");
    assert_eq!(e.pending_status(), "3");
}

#[test]
fn escape_resets_pending_state() {
    let mut e = ed("|a^");
    e.handle_keys("3g");
    assert_eq!(e.pending_status(), "3g");
    e.handle_keys("<Esc>");
    assert_eq!(e.pending_status(), "");
    assert!(e.mode().is_plain_normal());
    e.handle_keys("\"a<Esc>");
    assert_eq!(e.pending_register(), None);
}

#[test]
fn pending_status_strings() {
    let cases = [
        ("g", "g"),
        ("m", "m"),
        ("mi", "mi"),
        ("ma", "ma"),
        ("ms", "ms"),
        ("f", "f"),
        ("2t", "2t"),
        ("F", "F"),
        ("T", "T"),
        ("r", "r"),
        ("\"", "\""),
        (" ", "<space>"),
        ("[", "["),
        ("]", "]"),
        ("4]", "4]"),
    ];
    for (keys, status) in cases {
        let mut e = ed("|a^");
        e.handle_keys(keys);
        assert_eq!(e.pending_status(), status, "keys {keys:?}");
    }
}

#[test]
fn view_mode_status_extension() {
    let mut e = ed_ext("|a^");
    e.handle_keys("z");
    assert_eq!(e.pending_status(), "z");
    let mut e = ed("|a^");
    e.handle_keys("z");
    assert_eq!(e.pending_status(), "");
}

#[test]
fn register_selection() {
    let mut e = ed("|a^");
    e.handle_keys("\"x");
    assert_eq!(e.pending_register(), Some('x'));
    assert_eq!(e.pending_status(), "");
    e.handle_keys("3");
    assert_eq!(e.pending_register(), Some('x'));
    assert_eq!(e.pending_status(), "3");
}

#[test]
fn toggle_select_mode() {
    let mut e = ed("|a^b");
    let fx = e.handle_keys("v");
    assert_eq!(e.mode(), Mode::SELECT);
    assert_eq!(e.mode_label(), "SEL");
    assert!(fx.contains(&Effect::ModeChanged {
        from: ModeType::Normal,
        to: ModeType::Select
    }));
    e.handle_keys("l");
    assert_eq!(render(&e), "^ab|");
    assert_eq!(e.mode(), Mode::SELECT);
    e.handle_keys("v");
    assert_eq!(e.mode(), Mode::NORMAL);
    assert_eq!(e.mode_label(), "NOR");
}

#[test]
fn escape_from_select_goes_to_normal() {
    let mut e = ed("|a^b");
    e.handle_keys("vl<Esc>");
    assert_eq!(e.mode(), Mode::NORMAL);
    // the selection is kept
    assert_eq!(render(&e), "^ab|");
}

#[test]
fn select_mode_minor_returns_to_select() {
    let mut e = ed("|a^b\ncd");
    e.handle_keys("vgl");
    assert_eq!(e.mode(), Mode::SELECT);
    e.handle_keys("gQ");
    assert_eq!(e.mode(), Mode::SELECT);
}

#[test]
fn insert_label() {
    let mut e = ed("|a^b");
    e.handle_keys("i");
    assert_eq!(e.mode_label(), "INS");
    assert_eq!(e.pending_status(), "");
}

#[test]
fn unbound_char_is_swallowed_in_normal() {
    let mut e = ed("|a^b");
    let fx = e.handle_keys("Q");
    assert!(!fx.contains(&Effect::Unhandled));
    assert_eq!(e.text(), "ab");
    let fx = e.handle_keys("<C-q>");
    assert!(fx.contains(&Effect::Unhandled));
}

#[test]
fn unbound_char_cancels_minor_mode() {
    let mut e = ed("|a^b");
    e.handle_keys("3gQ");
    assert_eq!(e.pending_status(), "");
    assert!(e.mode().is_plain_normal());
    // a non-char key keeps the minor mode and is Unhandled
    let mut e = ed("|a^b");
    e.handle_keys("g");
    let fx = e.handle_keys("<C-q>");
    assert!(fx.contains(&Effect::Unhandled));
    assert_eq!(e.pending_status(), "g");
}

#[test]
fn count_survives_words_quirk() {
    let mut e = ed("|a^b cd");
    e.handle_keys("3w");
    assert_eq!(e.pending_status(), "3");
}

#[test]
fn count_reset_by_motion() {
    let mut e = ed("|a^bcd");
    e.handle_keys("2l");
    assert_eq!(e.pending_status(), "");
    assert_eq!(render(&e), "ab|c^d");
}

#[test]
fn yank_resets_register_and_mode() {
    let mut e = ed("|a^b");
    e.handle_keys("v\"ay");
    assert_eq!(e.mode(), Mode::NORMAL);
    assert_eq!(e.registers().get('a').unwrap(), &vec!["a".to_string()]);
}

#[test]
fn find_drops_pending_register_quirk() {
    let mut e = ed("|a^bc");
    e.handle_keys("\"af");
    assert_eq!(e.pending_register(), None);
    e.handle_keys("c");
    assert_eq!(render(&e), "^abc|");
}

#[test]
fn tab_is_swallowed_in_normal() {
    let mut e = ed("|a^b");
    let fx = e.handle_keys("<Tab><S-Tab>");
    assert!(!fx.contains(&Effect::Unhandled));
    assert_eq!(e.text(), "ab");
}

#[test]
fn colon_resets_count() {
    let mut e = ed("|a^b");
    e.handle_keys("3:");
    assert_eq!(e.pending_status(), "");
    e.prompt_close(false);
}
