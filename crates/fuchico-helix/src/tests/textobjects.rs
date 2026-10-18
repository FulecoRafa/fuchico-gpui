//! Match mode: mm, ma/mi (pairs, m, p; w and quotes with extensions).

use super::*;
use crate::Mode;

#[test]
fn match_bracket_normal() {
    check("|(^ab)", "mm", "(ab^)|");
    check("(ab^)|", "mm", "^(|ab)");
    check("|[^a(b)c]", "mm", "[a(b)c^]|");
    check("|{^ x }", "mm", "{ x ^}|");
}

#[test]
fn match_bracket_not_on_bracket() {
    check("(a|b^)", "mm", "(a|b^)");
    // quotes are recognised but never matched
    check("|\"^ab\"", "mm", "|\"^ab\"");
}

#[test]
fn match_bracket_unbalanced() {
    check("|(^ab", "mm", "|(^ab");
}

#[test]
fn match_bracket_angle_is_extension() {
    check("|<^a>", "mm", "|<^a>");
    check_ext("|<^a>", "mm", "<a^>|");
}

#[test]
fn match_bracket_select_mode_extends() {
    let mut e = ed("|(^ab)");
    e.handle_keys("vmm");
    assert_eq!(render(&e), "^(ab)|");
    assert_eq!(e.mode(), Mode::SELECT);
}

#[test]
fn around_and_inside_pairs() {
    check("(a|b^c)", "ma(", "^(abc)|");
    check("(a|b^c)", "mi(", "(^abc|)");
    check("(a|b^c)", "ma)", "^(abc)|");
    check("x [a|b^] y", "mi[", "x [^ab|] y");
    check("{ a|b^ }", "ma{", "^{ ab }|");
}

#[test]
fn inside_pair_nested() {
    check("(a (b|c^) d)", "mi(", "(a (^bc|) d)");
    // quirk: with the cursor on an opening bracket the backward scan stops
    // before the head, so the enclosing pair is selected
    check("(a |(^bc) d)", "mi(", "(^a (bc) d|)");
}

#[test]
fn inside_pair_backward_range() {
    check("(a|bc^d)", "mi(", "(|abcd^)");
}

#[test]
fn inside_pair_not_found_keeps_selection() {
    check("a|b^c", "mi(", "a|b^c");
    check("(abc) |x^", "mi(", "(abc) |x^");
}

#[test]
fn around_pair_multiple_cursors() {
    let mut e = ed("(a|b^) (¦c°d)");
    e.handle_keys("mi(");
    assert_eq!(render(&e), "(^ab|) (°cd¦)");
}

#[test]
fn match_in_brackets_m() {
    check("(a [b|c^] d)", "mam", "(a ^[bc]| d)");
    check("(a [b|c^] d)", "mim", "(a [^bc|] d)");
    check("(a |x^ d)", "mam", "^(a x d)|");
    check("a|b^c", "mam", "a|b^c");
}

#[test]
fn paragraph() {
    // the range ends at the following empty line (its newline included)
    check("x\n\na\n|b^c\n\ny", "mip", "x\n\n^a\nbc\n|\ny");
    check("x\n\na\n|b^c\n\ny", "map", "x\n\n^a\nbc\n|\ny");
    check("a\n|b^", "mip", "^a\nb|");
}

#[test]
fn quotes_and_words_noop_in_parity() {
    check("say \"h|i^\" now", "mi\"", "say \"h|i^\" now");
    check("he|l^lo world", "miw", "he|l^lo world");
}

#[test]
fn quote_objects_extension() {
    check_ext("say \"h|i^\" now", "mi\"", "say \"^hi|\" now");
    check_ext("say \"h|i^\" now", "ma\"", "say ^\"hi\"| now");
    check_ext("say 'h|i^' now", "mi'", "say '^hi|' now");
    check_ext("|n^o quotes", "mi\"", "|n^o quotes");
}

#[test]
fn word_objects_extension() {
    check_ext("he|l^lo world", "miw", "^hello| world");
    check_ext("he|l^lo world", "maw", "^hello |world");
    check_ext("hello wo|r^ld", "maw", "hello^ world|");
    check_ext("a-|b^.c d", "miW", "^a-b.c| d");
    check_ext("a-|b^.c d", "miw", "a-^b|.c d");
}

#[test]
fn angle_pair_extension() {
    check("<a|b^>", "mi<", "<a|b^>");
    check_ext("<a|b^>", "mi<", "<^ab|>");
}

#[test]
fn match_mode_status_and_cancel() {
    let mut e = ed("(a|b^)");
    e.handle_keys("m");
    assert_eq!(e.pending_status(), "m");
    e.handle_keys("i");
    assert_eq!(e.pending_status(), "mi");
    e.handle_keys("<Esc>");
    assert_eq!(e.pending_status(), "");
    assert_eq!(render(&e), "(a|b^)");
    // an unbound char cancels the minor mode
    e.handle_keys("mq");
    assert_eq!(e.mode(), Mode::NORMAL);
    e.handle_keys("l");
    assert_eq!(render(&e), "(ab|)^");
}
