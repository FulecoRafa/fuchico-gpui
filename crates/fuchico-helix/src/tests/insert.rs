//! Entering insert mode and the insert-mode keymap.

use super::*;
use crate::{Mode, ModeType};

#[track_caller]
fn check_ins(before: &str, keys: &str, after: &str) {
    let mut e = ed(before);
    e.handle_keys("i");
    assert_eq!(e.mode().ty, ModeType::Insert);
    e.handle_keys(keys);
    assert_eq!(render(&e), after, "insert keys {keys:?} from {before:?}");
}

#[track_caller]
fn check_ins_ext(before: &str, keys: &str, after: &str) {
    let mut e = ed_ext(before);
    e.handle_keys("i");
    e.handle_keys(keys);
    assert_eq!(render(&e), after, "insert keys {keys:?} from {before:?}");
}

// ---- entering ---------------------------------------------------------------------

#[test]
fn insert_and_append() {
    check("he|l^lo", "i", "he|llo");
    check("he|l^lo", "a", "hel|lo");
    check("^hel|lo", "i", "|hello");
    check("^hel|lo", "a", "hel|lo");
    check("a|b^c\ndef", "A", "abc|\ndef");
    check("  a|b^c", "I", "  |abc");
    check("   |", "I", "|   ");
}

#[test]
fn insert_on_every_selection() {
    check("°a¦b ^c|d", "a", "a¡b c|d");
    let mut e = ed("°a¦b ^c|d");
    e.handle_keys("ix");
    assert_eq!(render(&e), "x¡ab x|cd");
}

#[test]
fn mode_labels() {
    let mut e = ed("a|b^c");
    assert_eq!(e.mode_label(), "NOR");
    e.handle_keys("i");
    assert_eq!(e.mode_label(), "INS");
    e.handle_keys("<esc>v");
    assert_eq!(e.mode_label(), "SEL");
}

#[test]
fn mode_changed_effects() {
    let mut e = ed("a|b^c");
    let fx = e.handle_keys("i");
    assert!(fx.contains(&Effect::ModeChanged {
        from: ModeType::Normal,
        to: ModeType::Insert
    }));
    let fx = e.handle_keys("<esc>");
    assert!(fx.contains(&Effect::ModeChanged {
        from: ModeType::Insert,
        to: ModeType::Normal
    }));
}

// ---- typing -------------------------------------------------------------------------

#[test]
fn typing_inserts() {
    check_ins("he|l^lo", "XY", "heXY|llo");
    check_ins("he|l^lo", "<space>", "he |llo");
    check_ins("|", "abc", "abc|");
}

#[test]
fn typing_replaces_non_empty_selection() {
    let mut e = ed("^hel|lo");
    e.handle_keys("i");
    e.set_selection(Selection::single(0, 3));
    e.handle_keys("X");
    assert_eq!(render(&e), "X|lo");
}

#[test]
fn ime_text_input() {
    let mut e = ed("a|b^c");
    e.handle_keys("i");
    let fx = e.insert_text("日本");
    assert_eq!(render(&e), "a日本|bc");
    assert!(fx.iter().any(|f| matches!(f, Effect::DocChanged(_))));
}

#[test]
fn insert_text_in_normal_mode_is_keys() {
    let mut e = ed("a|b^c");
    e.insert_text("l");
    assert_eq!(render(&e), "ab|c^");
    assert_eq!(e.text(), "abc");
}

#[test]
fn doc_changed_reports_char_and_byte_ranges() {
    let mut e = ed("é|b^c");
    e.handle_keys("i");
    let fx = e.handle_keys("x");
    let edits = fx
        .iter()
        .find_map(|f| match f {
            Effect::DocChanged(edits) => Some(edits.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].from, 1);
    assert_eq!(edits[0].to, 1);
    assert_eq!(edits[0].from_byte, 2);
    assert_eq!(edits[0].to_byte, 2);
    assert_eq!(edits[0].insert, "x");
}

// ---- escape -------------------------------------------------------------------------

#[test]
fn escape_turns_cursor_into_block() {
    check_ins("he|l^lo", "<esc>", "he|l^lo");
    check_ins("hello|", "<esc>", "hello|");
    check_ins("ab|\n^cd", "<esc>", "ab|\n^cd");
}

#[test]
fn escape_keeps_ranges() {
    let mut e = ed("^hel|lo");
    e.handle_keys("i");
    e.set_selection(Selection::single(0, 3));
    e.handle_keys("<esc>");
    assert_eq!(render(&e), "^hel|lo");
    assert_eq!(e.mode(), Mode::NORMAL);
}

// ---- deleting -------------------------------------------------------------------------

#[test]
fn backspace_and_delete() {
    check_ins("ab|c^", "<bs>", "a|c");
    check_ins("|a^bc", "<bs>", "|abc");
    check_ins("a|b^c", "<del>", "a|c");
    check_ins("abc|", "<del>", "abc|");
    check_ins("a\n|b^", "<bs>", "a|b");
}

#[test]
fn backspace_by_indent_unit() {
    check_ins("    |a^bc", "<bs>", "  |abc");
    check_ins("   |a^bc", "<bs>", "  |abc");
    check_ins("  |a^bc", "<bs>", "|abc");
    check_ins("\t|a^bc", "<bs>", "|abc");
    check_ins(" \t|a^bc", "<bs>", " |abc");
    // not in leading whitespace: a single char
    check_ins("a    |b^", "<bs>", "a   |b");
}

#[test]
fn backspace_deletes_selection() {
    let mut e = ed("|hello");
    e.handle_keys("i");
    e.set_selection(Selection::single(1, 4));
    e.handle_keys("<bs>");
    assert_eq!(render(&e), "h|o");
}

#[test]
fn backspace_grapheme() {
    check_ins("e\u{301}|x^", "<bs>", "|x");
}

#[test]
fn delete_word() {
    check_ins("foo bar|", "<A-bs>", "foo |");
    check_ins("foo bar|", "<C-bs>", "foo |");
    check_ins("foo   |", "<A-bs>", "foo|");
    check_ins("|f^oo bar", "<A-del>", "| bar");
    check_ins("foo.bar|", "<A-bs>", "foo.|");
}

#[test]
fn delete_to_line_boundary() {
    check_ins("abc\nfoo |b^ar", "<Meta-Backspace>", "abc\n|bar");
    check_ins("abc\nfoo |b^ar", "<Meta-Delete>", "abc\nfoo |");
    // at the line start: joins with the previous line
    check_ins("abc\n|f^oo", "<Meta-Backspace>", "abc|foo");
}

#[test]
fn helix_insert_kills_are_extensions() {
    check_ins_ext("foo bar|", "<C-w>", "foo |");
    check_ins_ext("foo |b^ar", "<C-u>", "|bar");
    check_ins_ext("foo |b^ar", "<C-k>", "foo |");
    // without extensions they are unhandled
    let mut e = ed("foo bar|");
    e.handle_keys("i");
    assert!(e.handle_keys("<C-w>").contains(&Effect::Unhandled));
    assert_eq!(e.text(), "foo bar");
}

// ---- enter ------------------------------------------------------------------------------

#[test]
fn enter_keeps_indentation() {
    check_ins("  ab|c^", "<ret>", "  ab\n  |c");
    check_ins("ab|c^", "<ret>", "ab\n|c");
    // whitespace after the cursor is eaten
    check_ins("ab|  ^cd", "<ret>", "ab\n|cd");
}

#[test]
fn enter_in_leading_whitespace_first_line() {
    check_ins("  |a^bc", "<ret>", "\n  |abc");
}

#[test]
fn enter_slice_bug_quirk() {
    // `line.text.slice(0, from)` with an absolute `from`: on later lines the
    // leading-whitespace check looks at the wrong text.
    check_ins("x\n  |a^bc", "<ret>", "x\n  \n  |abc");
}

#[test]
fn enter_explodes_brackets() {
    check_ins("(|)^", "<ret>", "(\n|\n)");
    check_ins("  [|]^", "<ret>", "  [\n  |\n  ]");
    check_ins("{|}^", "<ret>", "{\n|\n}");
    check_ins("(|x^)", "<ret>", "(\n|x)");
}

#[test]
fn enter_multiple_cursors() {
    let mut e = ed("a¦b° c|d^");
    e.handle_keys("i<ret>");
    assert_eq!(render(&e), "a\n¡b c\n|d");
}

#[test]
fn enter_continues_markup_extension() {
    check_ins_ext("- item|", "<ret>", "- item\n- |");
    check_ins_ext("1. item|", "<ret>", "1. item\n2. |");
    check_ins_ext("> quote|", "<ret>", "> quote\n> |");
    // an empty item is cleared
    check_ins_ext("- item\n- |", "<ret>", "- item\n|");
}

// ---- moves ----------------------------------------------------------------------------

#[test]
fn arrows() {
    check_ins("ab|c^", "<left>", "a|bc");
    check_ins("ab|c^", "<right>", "abc|");
    check_ins("a|b^c\ndef", "<down>", "abc\nd|ef");
    check_ins("abc\nd|e^f", "<up>", "a|bc\ndef");
    check_ins("a|b^c", "<up>", "|abc");
    check_ins("a|b^c", "<down>", "abc|");
}

#[test]
fn arrow_keeps_goal_column() {
    check_ins("ab|c^\nd\nefg", "<down><down>", "abc\nd\nef|g");
}

#[test]
fn home_and_end() {
    check_ins("  ab|c^", "<home>", "  |abc");
    check_ins("  ab|c^", "<home><home>", "|  abc");
    check_ins("  ab|c^", "<home><home><home>", "  |abc");
    check_ins("a|b^c\ndef", "<end>", "abc|\ndef");
}

// ---- tab -----------------------------------------------------------------------------

#[test]
fn tab_indents_lines() {
    check_ins("ab|c^", "<tab>", "  ab|c");
    check_ins("  ab|c^", "<S-Tab>", "ab|c");
}

// ---- registers ------------------------------------------------------------------------

#[test]
fn ctrl_r_inserts_register() {
    let mut e = super::editing::with_reg("a|b^c", 'x', &["XY"]);
    e.handle_keys("i<C-r>");
    assert_eq!(e.pending_status(), "<C-r>");
    e.handle_keys("x");
    assert_eq!(render(&e), "aXY|bc");
    assert_eq!(e.mode(), Mode::INSERT);
}

#[test]
fn ctrl_r_escape_cancels_only_the_prompt() {
    let mut e = ed("a|b^c");
    e.handle_keys("i<C-r><esc>");
    assert_eq!(e.mode(), Mode::INSERT);
    e.handle_keys("z");
    assert_eq!(e.text(), "azbc");
}

#[test]
fn ctrl_r_special_registers() {
    let mut e = ed("a|b^c");
    e.handle_keys("i<C-r>#");
    assert_eq!(render(&e), "a1|bc");
    e.handle_keys("<C-r>.");
    // `.` is the (empty) selection contents
    assert_eq!(render(&e), "a1|bc");
}

#[test]
fn ctrl_r_clipboard() {
    let host = crate::MemoryHost {
        clipboard: Some("clip".into()),
        ..Default::default()
    };
    let mut e = Editor::with_host("ab", Config::parity(), Box::new(host));
    e.handle_keys("i<C-r>+");
    assert_eq!(e.text(), "clipab");
}

#[test]
fn unbound_keys_in_insert_are_unhandled() {
    let mut e = ed("a|b^c");
    e.handle_keys("i");
    assert!(e.handle_keys("<C-z>").contains(&Effect::Unhandled));
    assert!(e.handle_keys("<F2>").contains(&Effect::Unhandled));
}
