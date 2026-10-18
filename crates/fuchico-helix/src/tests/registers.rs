//! Registers: named, special (`# . _ % +`), clipboard, sharing, history.

use super::editing::with_reg;
use super::*;
use crate::Registers;
use crate::registers::is_read_only;

#[test]
fn named_register_yank_and_paste() {
    let mut e = ed("^ab|c");
    e.handle_keys("\"ay");
    assert_eq!(e.registers().get('a').unwrap(), &vec!["ab".to_string()]);
    assert!(e.registers().get('"').is_none());
    e.handle_keys("gl\"ap");
    assert_eq!(render(&e), "abc^ab|");
}

#[test]
fn register_is_consumed_by_yank() {
    let mut e = ed("^ab|c");
    e.handle_keys("\"a");
    assert_eq!(e.pending_register(), Some('a'));
    e.handle_keys("y");
    assert_eq!(e.pending_register(), None);
    e.handle_keys("y");
    assert_eq!(e.registers().get('"').unwrap(), &vec!["ab".to_string()]);
}

#[test]
fn paste_from_named_register_per_selection() {
    let mut e = with_reg("^a|¦b°", 'q', &["1", "2"]);
    e.handle_keys("\"qp");
    assert_eq!(e.text(), "a1b2");
}

#[test]
fn paste_reuses_last_value_for_extra_selections() {
    let mut e = with_reg("^a|¦b°¦c°", 'q', &["1", "2"]);
    e.handle_keys("\"qp");
    assert_eq!(e.text(), "a1b2c2");
}

#[test]
fn index_register() {
    let mut e = ed("^a|¦b°¦c°");
    e.handle_keys("\"#R");
    assert_eq!(e.text(), "123");
}

#[test]
fn selection_register() {
    let mut e = ed("^ab| cd");
    e.handle_keys("\".P");
    assert_eq!(e.text(), "abab cd");
}

#[test]
fn black_hole_register() {
    let mut e = ed("^ab|c");
    e.handle_keys("\"_y");
    assert!(e.registers().get('_').is_none());
    e.handle_keys("\"_p");
    assert_eq!(e.text(), "abc");
}

#[test]
fn path_register() {
    let mut e = Editor::with_host(
        "x",
        Config::parity(),
        Box::new(MemoryHost {
            path: Some("notes/a.md".into()),
            ..Default::default()
        }),
    );
    e.set_selection(Selection::single(1, 0));
    e.handle_keys("\"%P");
    assert_eq!(e.text(), "notes/a.mdx");
    // no path: empty paste
    let mut e = ed("|x^");
    e.handle_keys("\"%P");
    assert_eq!(e.text(), "x");
}

#[test]
fn read_only_registers() {
    for r in ['_', '%', '.', '#'] {
        assert!(is_read_only(r));
        let mut regs = Registers::new();
        regs.write(r, vec!["x".into()]);
        assert!(regs.get(r).is_none());
    }
    assert!(!is_read_only('a'));
}

#[test]
fn clipboard_register_writes_host() {
    let mut e = ed("^ab|¦cd°");
    e.handle_keys("\"+y");
    assert_eq!(e.host_mut().clipboard_read().unwrap(), "ab\ncd");
    assert_eq!(
        e.registers().get('+').unwrap(),
        &vec!["ab".to_string(), "cd".to_string()]
    );
}

#[test]
fn clipboard_read_prefers_matching_yank() {
    // The yanked value is reused (keeping multiple selections) when the
    // clipboard still holds its joined text...
    let mut e = ed("^ab|¦cd°");
    e.handle_keys("\"+y\"+R");
    assert_eq!(e.text(), "abcd");
    // ...otherwise the clipboard text is one value.
    let mut e = ed("^ab|¦cd°");
    e.handle_keys("\"+y");
    e.host_mut().clipboard_write("Z");
    e.handle_keys("\"+R");
    assert_eq!(e.text(), "ZZ");
}

#[test]
fn space_clipboard_commands() {
    let mut e = ed("^ab|c");
    e.handle_keys(" y");
    assert_eq!(e.host_mut().clipboard_read().unwrap(), "ab");
    e.handle_keys("gl p");
    assert_eq!(e.text(), "abcab");
    e.handle_keys("gh P");
    assert_eq!(e.text(), "ababcab");
}

#[test]
fn clipboard_error_is_reported() {
    struct NoClip;
    impl crate::VisualLines for NoClip {}
    impl crate::Host for NoClip {}
    let mut e = Editor::with_host("abc", Config::parity(), Box::new(NoClip));
    e.set_selection(Selection::single(1, 0));
    let fx = e.handle_keys(" p");
    assert_eq!(
        messages(&fx),
        vec![("clipboard unavailable".to_string(), true)]
    );
    assert_eq!(e.text(), "abc");
}

#[test]
fn default_register_config() {
    let mut cfg = Config::parity();
    cfg.default_register = 'z';
    let mut e = ed_with("^ab|c", cfg);
    let fx = e.handle_keys("y");
    assert_eq!(
        messages(&fx),
        vec![("yanked 1 selection to register z".to_string(), false)]
    );
    assert_eq!(e.registers().get('z').unwrap(), &vec!["ab".to_string()]);
}

#[test]
fn share_registers_between_editors() {
    let mut a = ed("^ab|c");
    a.handle_keys("y");
    let mut b = ed("|x^");
    b.set_registers(a.registers().clone());
    b.handle_keys("p");
    assert_eq!(b.text(), "xab");
}

#[test]
fn command_register_keeps_one_value_and_history() {
    let mut regs = Registers::new();
    regs.write(':', vec!["a".into(), "b".into()]);
    assert_eq!(regs.get(':').unwrap(), &vec!["a".to_string()]);
    regs.write(':', vec!["c".into()]);
    assert_eq!(regs.history(':'), &["a".to_string(), "c".to_string()]);
    regs.write('/', vec!["x".into(), "y".into()]);
    assert_eq!(regs.get('/').unwrap().len(), 2);
    assert_eq!(regs.history('/'), &["x".to_string()]);
    assert!(regs.history('a').is_empty());
    regs.write('a', vec!["1".into()]);
    assert_eq!(regs.names(), vec!['/', ':', 'a']);
}

#[test]
fn insert_mode_ctrl_r_named() {
    let mut e = with_reg("|a^", 'k', &["XY"]);
    e.handle_keys("i<C-r>k");
    assert_eq!(e.text(), "XYa");
    assert_eq!(e.mode(), crate::Mode::INSERT);
}
