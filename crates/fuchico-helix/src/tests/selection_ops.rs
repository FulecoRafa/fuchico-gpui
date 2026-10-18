//! Selection manipulation: x, %, ;, Alt-;, Alt-:, _, Alt-s, `,`, ( ),
//! syntax expand/shrink/siblings, pointer selections, block cursors.

use super::*;
use crate::text::Text;
use crate::{Host, Mode, VisualLines};

#[test]
fn extend_line() {
    check("a|b^c\ndef", "x", "^abc\n|def");
    check("a|b^c\ndef", "xx", "^abc\ndef|");
    check("a|b^c\ndef\nghi", "2x", "^abc\ndef\nghi|");
    check("abc\n|d^ef", "x", "abc\n^def|");
    // a count always extends, even from a non-line selection
    check("a|b^c\ndef\nghi", "1x", "^abc\ndef\n|ghi");
}

#[test]
fn extend_line_multi_line_selection() {
    check("a^bc\nd|ef\nghi", "x", "^abc\ndef\n|ghi");
}

#[test]
fn select_all() {
    check("a|b^c\nd", "%", "^abc\nd|");
}

#[test]
fn collapse() {
    check("^abc|", ";", "ab|c^");
    check("|abc^", ";", "|a^bc");
    check("^ab| °cd¦", ";", "a|b^ c¦d°");
}

#[test]
fn flip_and_ensure_forward() {
    check("^abc|", "<A-;>", "|abc^");
    check("|a^bc", "<A-;>", "|a^bc");
    check("|abc^", "<A-:>", "^abc|");
    check("^abc|", "<A-:>", "^abc|");
}

#[test]
fn trim_selections() {
    check("^  ab  |", "_", "  ^ab|  ");
    check("|  ab  ^", "_", "  |ab^  ");
    check("^ab|", "_", "^ab|");
    check("^\n ab\t|", "_", "\n ^ab|\t");
}

#[test]
fn split_on_newline() {
    check("^ab\ncd|", "<A-s>", "^ab|\n°cd¦");
    check("^ab\ncd\n|ef", "<A-s>", "^ab|\n°cd¦\nef");
}

#[test]
fn keep_primary() {
    check("°a¦b |c^d", ",", "ab |c^d");
    check("|a^b", ",", "|a^b");
}

#[test]
fn rotate_selections() {
    check("°a¦b |c^d", ")", "^a|b ¦c°d");
    check("°a¦b |c^d", "(", "^a|b ¦c°d");
    check("^a|b ¦c°d", "(", "°a¦b |c^d");
    check("|a^b", ")", "|a^b");
}

#[test]
fn siblings_without_syntax_select_all() {
    check("|a^b\nc", "<A-n>", "^ab\nc|");
    check("|a^b\nc", "<A-p>", "^ab\nc|");
    check("|a^b\nc", "<A-ArrowRight>", "^ab\nc|");
}

#[test]
fn expand_without_syntax_is_noop() {
    check("|a^b", "<A-o>", "|a^b");
    check("|a^b", "<A-o><A-i>", "|a^b");
}

/// A fake syntax tree: words, then lines, then the document.
struct Tree;
impl VisualLines for Tree {}
impl Host for Tree {
    fn syntax_parent(&mut self, doc: &Text, from: usize, to: usize) -> Option<(usize, usize)> {
        let s = doc.to_string();
        let chars: Vec<char> = s.chars().collect();
        let mut f = from;
        while f > 0 && chars[f - 1].is_alphanumeric() {
            f -= 1;
        }
        let mut t = to;
        while t < chars.len() && chars[t].is_alphanumeric() {
            t += 1;
        }
        if (f, t) != (from, to) {
            return Some((f, t));
        }
        let line = doc.line_at(from);
        if (line.from, line.to) != (from, to) && to <= line.to {
            return Some((line.from, line.to));
        }
        None
    }
    fn syntax_sibling(
        &mut self,
        _doc: &Text,
        _from: usize,
        to: usize,
        forward: bool,
    ) -> Option<(usize, usize)> {
        if forward {
            Some((to + 1, to + 3))
        } else {
            None
        }
    }
}

fn tree(s: &str) -> Editor {
    let (text, sel) = parse(s);
    let mut e = Editor::with_host(&text, Config::parity(), Box::new(Tree));
    e.set_selection(sel);
    e
}

#[test]
fn expand_and_shrink_with_syntax() {
    let mut e = tree("ab c|d^e fg\nx");
    e.handle_keys("<A-o>");
    assert_eq!(render(&e), "ab |cde^ fg\nx");
    e.handle_keys("<A-o>");
    assert_eq!(render(&e), "|ab cde fg^\nx");
    // no parent: the history freezes, nothing moves
    e.handle_keys("<A-o>");
    assert_eq!(render(&e), "|ab cde fg^\nx");
    e.handle_keys("<A-i>");
    assert_eq!(render(&e), "ab |cde^ fg\nx");
    e.handle_keys("<A-i>");
    assert_eq!(render(&e), "ab c|d^e fg\nx");
    e.handle_keys("<A-i>");
    assert_eq!(render(&e), "ab c|d^e fg\nx");
    // redo the expansions from history
    e.handle_keys("<A-o>");
    assert_eq!(render(&e), "ab |cde^ fg\nx");
}

#[test]
fn siblings_with_syntax() {
    let mut e = tree("|a^b cd");
    e.handle_keys("<A-n>");
    assert_eq!(render(&e), "ab^ c|d");
    e.handle_keys("<A-p>");
    assert_eq!(render(&e), "^ab cd|");
}

#[test]
fn pointer_selection_becomes_block() {
    let mut e = ed("|a^bc");
    e.set_selection_from_pointer(Selection::single(1, 1));
    assert_eq!(render(&e), "a|b^c");
    e.set_selection_from_pointer(Selection::single(0, 2));
    assert_eq!(render(&e), "^ab|c");
    // clamped to the document
    e.set_selection_from_pointer(Selection::single(9, 9));
    assert_eq!(render(&e), "abc|");
    // insert mode keeps bar cursors
    e.handle_keys("i");
    e.set_selection_from_pointer(Selection::single(1, 1));
    assert_eq!(render(&e), "a|bc");
}

#[test]
fn block_cursor_positions() {
    let e = ed("^abc|");
    assert_eq!(e.block_cursors(), vec![2]);
    let e = ed("|abc^");
    assert_eq!(e.block_cursors(), vec![0]);
    let e = ed("a|b^c ¦d°");
    assert_eq!(e.block_cursors(), vec![1, 4]);
    let mut e = ed("^abc|");
    e.handle_keys("a");
    assert_eq!(e.mode(), Mode::INSERT);
    assert_eq!(e.block_cursors(), vec![3]);
}

#[test]
fn line_col() {
    assert_eq!(ed("ab\nc|d^").line_col(), (2, 2));
    assert_eq!(ed("|a^b").line_col(), (1, 1));
}

#[test]
fn set_selection_effects() {
    let mut e = ed("|a^b");
    let fx = e.set_selection(Selection::single(1, 2));
    assert!(fx.contains(&Effect::SelectionChanged));
    let fx = e.set_selection(Selection::single(1, 2));
    assert!(!fx.contains(&Effect::SelectionChanged));
}
