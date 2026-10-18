//! Normal-mode editing commands: d/c/y/p/P/R/r, case, indent, join,
//! increment, open line, add newline, copy selection, comments, surround.

use super::*;
use crate::{Mode, ModeType};

pub(crate) fn with_reg(s: &str, reg: char, value: &[&str]) -> Editor {
    let mut e = ed(s);
    let mut regs = e.registers().clone();
    regs.write(reg, value.iter().map(|v| v.to_string()).collect());
    e.set_registers(regs);
    e
}

#[track_caller]
fn check_reg(before: &str, value: &[&str], keys: &str, after: &str) {
    let mut e = with_reg(before, '"', value);
    e.handle_keys(keys);
    assert_eq!(render(&e), after, "keys {keys:?} from {before:?}");
}

// ---- delete / change -----------------------------------------------------------

#[test]
fn delete_block_cursor() {
    let mut e = ed("he|l^lo");
    e.handle_keys("d");
    assert_eq!(render(&e), "he|l^o");
    assert_eq!(e.registers().get('"').unwrap(), &vec!["l".to_string()]);
    assert_eq!(e.mode(), Mode::NORMAL);
}

#[test]
fn delete_selection() {
    check("^hel|lo", "d", "|l^o");
    check("|hel^lo", "d", "|l^o");
    check("hell|o^", "d", "hell|");
    check("^hello|", "d", "|");
}

#[test]
fn delete_multiple_selections() {
    let mut e = ed("°a¦b ^c|d");
    e.handle_keys("d");
    assert_eq!(render(&e), "¦b° |d^");
    assert_eq!(
        e.registers().get('"').unwrap(),
        &vec!["a".to_string(), "c".to_string()]
    );
}

#[test]
fn delete_line() {
    check("a|b^c\ndef", "xd", "|d^ef");
}

#[test]
fn delete_noyank() {
    let mut e = ed("^hel|lo");
    e.handle_keys("<A-d>");
    assert_eq!(render(&e), "|l^o");
    assert!(e.registers().get('"').is_none());
}

#[test]
fn delete_ignores_register_and_keeps_it_pending_quirk() {
    let mut e = ed("^hel|lo");
    e.handle_keys("\"ad");
    assert_eq!(e.text(), "lo");
    assert_eq!(e.registers().get('"').unwrap(), &vec!["hel".to_string()]);
    assert!(e.registers().get('a').is_none());
    assert_eq!(e.pending_register(), Some('a'));
}

#[test]
fn change_enters_insert_with_empty_cursor() {
    let mut e = ed("he|l^lo");
    e.handle_keys("c");
    assert_eq!(render(&e), "he|lo");
    assert_eq!(e.mode().ty, ModeType::Insert);
    assert_eq!(e.registers().get('"').unwrap(), &vec!["l".to_string()]);
    e.handle_keys("XY<esc>");
    // Escape turns the bar cursor into a block on the next char
    assert_eq!(render(&e), "heXY|l^o");
}

#[test]
fn change_noyank() {
    let mut e = ed("he|l^lo");
    e.handle_keys("<A-c>");
    assert_eq!(render(&e), "he|lo");
    assert_eq!(e.mode().ty, ModeType::Insert);
    assert!(e.registers().get('"').is_none());
}

// ---- yank / paste --------------------------------------------------------------

#[test]
fn yank_message_and_register() {
    let mut e = ed("^hel|lo");
    let fx = e.handle_keys("y");
    assert_eq!(
        messages(&fx),
        vec![("yanked 1 selection to register \"".to_string(), false)]
    );
    assert_eq!(e.registers().get('"').unwrap(), &vec!["hel".to_string()]);
    let mut e = ed("°a¦b ^c|d");
    let fx = e.handle_keys("\"zy");
    assert_eq!(
        messages(&fx),
        vec![("yanked 2 selections to register z".to_string(), false)]
    );
    assert_eq!(
        e.registers().get('z').unwrap(),
        &vec!["a".to_string(), "c".to_string()]
    );
}

#[test]
fn yank_leaves_select_mode_quirk() {
    let mut e = ed("^hel|lo");
    e.handle_keys("vy");
    assert_eq!(e.mode(), Mode::NORMAL);
}

#[test]
fn paste_after_and_before() {
    check_reg("he|l^lo", &["XY"], "p", "hel^XY|lo");
    check_reg("he|l^lo", &["XY"], "P", "he^XY|llo");
    check_reg("^hel|lo", &["XY"], "p", "hel^XY|lo");
    check_reg("^hel|lo", &["XY"], "P", "^XY|hello");
}

#[test]
fn paste_count_selects_one_copy_quirk() {
    check_reg("he|l^lo", &["XY"], "2p", "hel^XY|XYlo");
}

#[test]
fn paste_into_empty_doc() {
    check_reg("|", &["XY"], "p", "^XY|");
}

#[test]
fn paste_multiple_values() {
    check_reg("°a¦b ^c|d", &["1", "2"], "p", "a°1¦b c^2|d");
    // fewer values than selections: the last one repeats
    check_reg("°a¦b ^c|d", &["1"], "p", "a°1¦b c^1|d");
    // more values than selections: extra ones are dropped
    check_reg("^a|b", &["1", "2"], "p", "a^1|b");
}

#[test]
fn paste_empty_register_leaves_cursor_quirk() {
    // an unset register pastes [""] and leaves an empty cursor
    check("he|l^lo", "p", "hel|lo");
    check("he|l^lo", "\"_p", "hel|lo");
}

#[test]
fn paste_resets_mode() {
    let mut e = with_reg("he|l^lo", '"', &["X"]);
    e.handle_keys("vp");
    assert_eq!(e.mode(), Mode::NORMAL);
}

#[test]
fn replace_with_yanked() {
    check_reg("^hel|lo", &["X"], "R", "^X|lo");
    check_reg("^hel|lo", &["X"], "3R", "^XXX|lo");
    check_reg("°a¦b ^c|d", &["1", "2"], "R", "°1¦b ^2|d");
    // nothing to replace with: no-op
    check("^hel|lo", "R", "^hel|lo");
}

#[test]
fn replace_char() {
    check("^hel|lo", "rx", "^xxx|lo");
    check("he|l^lo", "rx", "he|x^lo");
    // newlines are replaced too (cm-helix quirk)
    check("^a\nb|c", "rx", "^xxx|c");
    let mut e = ed("he|l^lo");
    e.handle_keys("r");
    assert_eq!(e.pending_status(), "r");
    e.handle_keys("<esc>");
    assert_eq!(render(&e), "he|l^lo");
    assert_eq!(e.pending_status(), "");
}

#[test]
fn replace_char_only_primary_quirk() {
    check("°a¦b ^c|d", "rx", "°a¦b ^x|d");
}

// ---- case ---------------------------------------------------------------------

#[test]
fn change_case() {
    check("^hEl|lo", "~", "^HeL|lo");
    check("^hEl|lo", "`", "^hel|lo");
    check("^hEl|lo", "<A-`>", "^HEL|lo");
}

// ---- indent -------------------------------------------------------------------

#[test]
fn indent_and_dedent() {
    check("|a^bc\ndef", ">", "  |a^bc\ndef");
    check("  |a^bc", "<", "|a^bc");
    check("^abc\nde|f", ">", "  ^abc\n  de|f");
    check("|a^bc", "<", "|a^bc");
    check("    |a^bc", "<", "  |a^bc");
    check("   |a^bc", "<", " |a^bc");
}

#[test]
fn indent_includes_empty_lines_like_codemirror() {
    check("^abc\n\nde|f", ">", "  ^abc\n  \n  de|f");
}

#[test]
fn indent_with_tab_unit() {
    let mut c = Config::parity();
    c.indent_unit = "\t".into();
    let mut e = ed_with("|a^bc", c);
    e.handle_keys(">");
    assert_eq!(e.text(), "\tabc");
    e.handle_keys("<");
    assert_eq!(e.text(), "abc");
}

// ---- join ---------------------------------------------------------------------

#[test]
fn join_lines() {
    check("a|b^c\n  def", "J", "a|b^c def");
    check("^abc\n  de|f", "J", "^abc de|f");
    check("^abc\n  def|", "J", "^abc def|");
    check("a|b^c", "J", "a|b^c");
    check("^a\n b\n  c|", "J", "^a b c|");
}

#[test]
fn join_primary_only_and_drops_others() {
    check("¦a°\nb\n|c^\nd", "J", "a\nb\n|c^ d");
}

// ---- increment ----------------------------------------------------------------

#[test]
fn increment_and_decrement() {
    check("|1^0", "<C-a>", "|2^0");
    check("^10|", "<C-a>", "^11|");
    check("^-1|", "<C-a>", "^0|");
    check("^0|", "<C-x>", "^-1|");
    check("^9|", "<C-a>", "^10|");
    check("^ab|", "<C-a>", "^ab|");
}

#[test]
fn increment_uses_js_number_quirk() {
    check("^1.5|", "<C-a>", "^2.5|");
    check("^a1|", "<C-a>", "^NaN|");
    check("^0x1|", "<C-a>", "^2|");
}

#[test]
fn increment_every_selection() {
    check("°1¦ ^5|", "<C-a>", "°2¦ ^6|");
}

// ---- open line ------------------------------------------------------------------

#[test]
fn open_below_and_above() {
    let mut e = ed("a|b^c\ndef");
    e.handle_keys("o");
    assert_eq!(render(&e), "abc\n|\ndef");
    assert_eq!(e.mode().ty, ModeType::Insert);
    check("abc\nd|e^f", "O", "abc\n|\ndef");
    check("a|b^c", "O", "|\nabc");
    check("a|b^c", "o", "abc\n|");
}

#[test]
fn open_uses_primary_only() {
    check("¦a°\n|b^", "o", "a\nb\n|");
}

#[test]
fn open_parity_does_not_indent() {
    check("  a|b^c", "o", "  abc\n|");
}

#[test]
fn open_keeps_indent_with_continue_markup() {
    check_ext("  a|b^c", "o", "  abc\n  |");
    check_ext("  a|b^c", "O", "  |\n  abc");
    check_ext("\ta|b^c", "o", "\tabc\n\t|");
}

#[test]
fn open_continues_lists_and_quotes() {
    check_ext("- it|e^m", "o", "- item\n- |");
    check_ext("* it|e^m", "o", "* item\n* |");
    check_ext("  - it|e^m", "o", "  - item\n  - |");
    check_ext("1. it|e^m", "o", "1. item\n2. |");
    check_ext("> qu|o^te", "o", "> quote\n> |");
    check_ext("- [x] do|n^e", "o", "- [x] done\n- [ ] |");
    check_ext("- it|e^m", "O", "- |\n- item");
}

#[test]
fn open_then_type_and_escape() {
    let mut e = ed("a|b^c");
    e.handle_keys("oxy<esc>");
    assert_eq!(render(&e), "abc\nxy|");
    assert_eq!(e.mode(), Mode::NORMAL);
}

// ---- add newline ----------------------------------------------------------------

#[test]
fn add_newline_above_and_below() {
    check("a|b^c", "[<space>", "\na|b^c");
    check("a|b^c", "]<space>", "a|b^c\n");
    check("a|b^c", "2]<space>", "a|b^c\n\n");
    check("a|b^c", "]3<space>", "a|b^c\n\n\n");
    let mut e = ed("a|b^c");
    e.handle_keys("]<space>");
    assert_eq!(e.mode(), Mode::NORMAL);
}

// ---- copy selection to next line ----------------------------------------------

#[test]
fn copy_selection_down_flips_direction_quirk() {
    check("a|b^c\ndef", "C", "a¦b°c\nd^e|f");
    check("^a|bc\ndef", "C", "°a¦bc\n^d|ef");
}

#[test]
fn copy_selection_skips_short_lines() {
    check("ab|c^\nd\nefg", "C", "ab¦c°\nd\nef^g|");
    check("abc\nd|e^f", "C", "abc\nd|e^f");
}

#[test]
fn copy_selection_up_extension() {
    let mut e = ed_ext("abc\nd|e^f");
    e.handle_keys("<A-C>");
    assert_eq!(e.selection().ranges().len(), 2);
    let main = e.selection().main();
    assert_eq!((main.from(), main.to()), (1, 2));
}

// ---- comments ---------------------------------------------------------------------

#[test]
fn toggle_block_comment_by_line() {
    check("a|b^c", "<C-c>", "<!-- a|b^c -->");
    check("<!-- a|b^c -->", "<C-c>", "a|b^c");
    check("a|b^c", "<space>c", "<!-- a|b^c -->");
}

#[test]
fn toggle_comment_multiple_lines() {
    let mut e = ed("^abc\nde|f");
    e.handle_keys("<C-c>");
    assert_eq!(e.text(), "<!-- abc\ndef -->");
    e.handle_keys("<C-c>");
    assert_eq!(e.text(), "abc\ndef");
}

#[test]
fn toggle_line_comment() {
    let mut c = Config::parity();
    c.comment_tokens = crate::CommentTokens {
        line: Some("//".into()),
        block: None,
    };
    let mut e = ed_with("^abc\n  de|f", c);
    e.handle_keys("<C-c>");
    assert_eq!(e.text(), "// abc\n//   def");
    e.handle_keys("<C-c>");
    assert_eq!(e.text(), "abc\n  def");
}

#[test]
fn toggle_comment_returns_to_normal() {
    let mut e = ed("a|b^c");
    e.handle_keys("v<C-c>");
    assert_eq!(e.mode(), Mode::NORMAL);
}

// ---- surround -------------------------------------------------------------------

#[test]
fn surround_add() {
    check("^hel|lo", "ms(", "^(hel)|lo");
    check("^hel|lo", "ms)", "^(hel)|lo");
    check("he|l^lo", "ms\"", "he|\"l\"^lo");
    check("he|l^lo", "ms[", "he|[l]^lo");
    check("he|l^lo", "ms*", "he|*l*^lo");
    let mut e = ed("he|l^lo");
    e.handle_keys("ms");
    assert_eq!(e.pending_status(), "ms");
    e.handle_keys("{");
    assert_eq!(e.mode(), Mode::NORMAL);
    assert_eq!(e.text(), "he{l}lo");
}

#[test]
fn surround_multiple() {
    check("°a¦b ^c|d", "ms(", "°(a)¦b ^(c)|d");
}

#[test]
fn surround_delete_and_replace_extensions() {
    check_ext("(he|l^lo)", "md(", "he|l^lo");
    check_ext("(he|l^lo)", "mr([", "[he|l^lo]");
    check_ext("\"he|l^lo\"", "md\"", "he|l^lo");
}
