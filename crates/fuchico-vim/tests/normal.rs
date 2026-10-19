mod common;
use common::*;

// ---- motions -------------------------------------------------------------

#[test]
fn hjkl() {
    assert_eq!(cur("abc\ndef", "l"), (1, 2));
    assert_eq!(cur("abc\ndef", "lllll"), (1, 3));
    assert_eq!(cur("abc\ndef", "j"), (2, 1));
    assert_eq!(cur("abc\ndef", "jk"), (1, 1));
    assert_eq!(cur("abc\ndef", "llh"), (1, 2));
    assert_eq!(cur("abc\ndef", "2l"), (1, 3));
}

#[test]
fn vertical_keeps_column() {
    assert_eq!(cur("abcdef\nab\nabcdef", "$jj"), (3, 6));
    assert_eq!(cur("abcdef\nab\nabcdef", "4lj"), (2, 2));
    assert_eq!(cur("abcdef\nab\nabcdef", "4ljj"), (3, 5));
}

#[test]
fn words() {
    assert_eq!(cur("foo bar.baz qux", "w"), (1, 5));
    assert_eq!(cur("foo bar.baz qux", "ww"), (1, 8));
    assert_eq!(cur("foo bar.baz qux", "W"), (1, 5));
    assert_eq!(cur("foo bar.baz qux", "WW"), (1, 13));
    assert_eq!(cur("foo bar.baz qux", "e"), (1, 3));
    assert_eq!(cur("foo bar.baz qux", "ee"), (1, 7));
    assert_eq!(cur("foo bar.baz qux", "E"), (1, 3));
    assert_eq!(cur("foo bar.baz qux", "$b"), (1, 13));
    assert_eq!(cur("foo bar.baz qux", "$bb"), (1, 9));
    assert_eq!(cur("foo bar.baz qux", "$B"), (1, 13));
    assert_eq!(cur("foo bar.baz qux", "$BB"), (1, 5));
    assert_eq!(cur("foo bar\nbaz", "ww"), (2, 1));
    assert_eq!(cur("foo bar baz", "3w"), (1, 11));
    assert_eq!(cur("foo bar baz", "$ge"), (1, 7));
}

#[test]
fn line_motions() {
    assert_eq!(cur("  abc def", "$"), (1, 9));
    assert_eq!(cur("  abc def", "$0"), (1, 1));
    assert_eq!(cur("  abc def", "$^"), (1, 3));
    assert_eq!(cur("  abc def", "_"), (1, 3));
    assert_eq!(cur("a\n  b\nc", "+"), (2, 3));
    assert_eq!(cur("a\n  b\nc", "G-"), (2, 3));
    assert_eq!(cur("abcdef", "4|"), (1, 4));
}

#[test]
fn document_motions() {
    assert_eq!(cur("a\nb\nc\nd", "G"), (4, 1));
    assert_eq!(cur("a\nb\nc\nd", "Ggg"), (1, 1));
    assert_eq!(cur("a\nb\nc\nd", "2G"), (2, 1));
    assert_eq!(cur("a\nb\nc\nd", "3gg"), (3, 1));
    assert_eq!(cur("a\n  b\nc", "G2gg"), (2, 3));
}

#[test]
fn find_char() {
    assert_eq!(cur("a,b,c,d", "f,"), (1, 2));
    assert_eq!(cur("a,b,c,d", "2f,"), (1, 4));
    assert_eq!(cur("a,b,c,d", "f,;"), (1, 4));
    assert_eq!(cur("a,b,c,d", "f,;,"), (1, 2));
    assert_eq!(cur("a,b,c,d", "t,"), (1, 1));
    assert_eq!(cur("a,b,c,d", "lt,"), (1, 3));
    assert_eq!(cur("a,b,c,d", "$F,"), (1, 6));
    assert_eq!(cur("a,b,c,d", "$T,"), (1, 7));
    assert_eq!(cur("a,b,c,d", "fz"), (1, 1));
}

#[test]
fn paragraphs() {
    assert_eq!(cur("a\nb\n\nc\nd\n\ne", "}"), (3, 1));
    assert_eq!(cur("a\nb\n\nc\nd\n\ne", "}}"), (6, 1));
    assert_eq!(cur("a\nb\n\nc\nd\n\ne", "G{"), (6, 1));
    assert_eq!(cur("a\nb\n\nc\nd\n\ne", "2}"), (6, 1));
}

#[test]
fn matched_symbol() {
    assert_eq!(cur("(a [b] c)", "%"), (1, 9));
    assert_eq!(cur("(a [b] c)", "%%"), (1, 1));
    assert_eq!(cur("x (a [b] c)", "%"), (1, 11));
    assert_eq!(cur("(a [b] c)", "3l%"), (1, 6));
}

#[test]
fn star_and_hash() {
    assert_eq!(cur("foo bar foo foobar", "*"), (1, 9));
    assert_eq!(cur("foo bar foo foobar", "**"), (1, 1));
    assert_eq!(cur("foo bar foo foobar", "g*"), (1, 9));
    assert_eq!(cur("foo bar foo foobar", "g*g*"), (1, 13));
    assert_eq!(cur("foo bar foo foobar", "#"), (1, 9));
}

#[test]
fn screen_lines() {
    let text = "a\nb\nc\nd\ne";
    assert_eq!(cur(text, "L"), (5, 1));
    assert_eq!(cur(text, "GH"), (1, 1));
    assert_eq!(cur(text, "M"), (3, 1));
}

// ---- operators -------------------------------------------------------------

#[test]
fn delete_ops() {
    assert_eq!(run("foo bar baz", "dw"), "bar baz");
    assert_eq!(run("foo bar baz", "2dw"), "baz");
    assert_eq!(run("foo bar baz", "d2w"), "baz");
    assert_eq!(run("foo bar baz", "de"), " bar baz");
    assert_eq!(run("foo bar baz", "wd$"), "foo ");
    assert_eq!(run("foo bar baz", "wD"), "foo ");
    assert_eq!(run("foo bar baz", "$d0"), "z");
    assert_eq!(run("foo bar baz", "x"), "oo bar baz");
    assert_eq!(run("foo bar baz", "3x"), " bar baz");
    assert_eq!(run("foo bar baz", "$X"), "foo bar bz");
    assert_eq!(run("a,b,c", "dt,"), ",b,c");
    assert_eq!(run("a,b,c", "df,"), "b,c");
    assert_eq!(run("foo bar", "wdb"), "bar");
}

#[test]
fn delete_lines() {
    assert_eq!(run("a\nb\nc", "dd"), "b\nc");
    assert_eq!(run("a\nb\nc", "jdd"), "a\nc");
    assert_eq!(run("a\nb\nc", "Gdd"), "a\nb");
    assert_eq!(run("a\nb\nc", "2dd"), "c");
    assert_eq!(run("a\nb\nc", "dj"), "c");
    // cm-vim keeps the trailing newline when the range starts above the last line
    assert_eq!(run("a\nb\nc", "Gdk"), "a\n");
    assert_eq!(run("a\nb\nc\nd", "jdG"), "a\n");
    assert_eq!(run("a\nb\nc\nd", "jjdgg"), "d");
    assert_eq!(run("a\nb\nc", "5dd"), "");
}

#[test]
fn delete_line_cursor() {
    assert_eq!(run_c("a\n  b\nc", "dd"), ("  b\nc".into(), (1, 3)));
}

#[test]
fn change_ops() {
    let mut e = ed("foo bar");
    e.handle_keys("cwxyz<Esc>");
    assert_eq!(e.text(), "xyz bar");
    assert_eq!(e.mode_label(), "NORMAL");
    assert_eq!(e.line_col(), (1, 3));
    assert_eq!(run("foo bar", "ccnew<Esc>"), "new");
    assert_eq!(run("  foo bar", "ccnew<Esc>"), "  new");
    assert_eq!(run("foo bar", "wCx<Esc>"), "foo x");
    assert_eq!(run("foo bar", "sx<Esc>"), "xoo bar");
    assert_eq!(run("foo\nbar", "Sx<Esc>"), "x\nbar");
    // cw on a word acts like ce
    assert_eq!(run("foo  bar", "cwx<Esc>"), "x  bar");
}

#[test]
fn yank_and_paste() {
    assert_eq!(run("foo bar", "ywP"), "foo foo bar");
    assert_eq!(run("foo bar", "yw$p"), "foo barfoo ");
    assert_eq!(run("a\nb", "yyp"), "a\na\nb");
    assert_eq!(run("a\nb", "yyP"), "a\na\nb");
    assert_eq!(run("a\nb", "Yjp"), "a\nb\na");
    assert_eq!(run("a\nb", "yy3p"), "a\na\na\na\nb");
    assert_eq!(run("abc", "xp"), "bac");
    assert_eq!(run("a\nb\nc", "ddp"), "b\na\nc");
    assert_eq!(run("abc", "yl3p"), "aaaabc");
}

#[test]
fn yank_cursor() {
    let (t, c) = run_c("foo bar", "wyb");
    assert_eq!(t, "foo bar");
    // cm-vim's yank returns the old anchor
    assert_eq!(c, (1, 5));
}

#[test]
fn case_ops() {
    assert_eq!(run("hello world", "~"), "Hello world");
    assert_eq!(run("hello world", "3~"), "HELlo world");
    assert_eq!(run("hello world", "gUw"), "HELLO world");
    assert_eq!(run("HELLO world", "guw"), "hello world");
    assert_eq!(run("Hello world", "g~w"), "hELLO world");
    assert_eq!(run("hello world", "gUU"), "HELLO WORLD");
    assert_eq!(run("hello", "g?w"), "uryyb");
}

#[test]
fn indent_ops() {
    assert_eq!(run("a\nb", ">>"), "  a\nb");
    assert_eq!(run("a\nb", ">j"), "  a\n  b");
    assert_eq!(run("    a\nb", "<<"), "  a\nb");
    assert_eq!(run("a\nb\nc", "2>>"), "  a\n  b\nc");
}

#[test]
fn join_lines() {
    assert_eq!(run("a\nb\nc", "J"), "a b\nc");
    assert_eq!(run("a\nb\nc", "3J"), "a b c");
    assert_eq!(run("a\n   b", "J"), "a b");
    assert_eq!(run("a\nb", "gJ"), "ab");
    assert_eq!(cur("abc\ndef", "J"), (1, 4));
}

#[test]
fn replace_char() {
    assert_eq!(run("abc", "rx"), "xbc");
    assert_eq!(run("abc", "2rx"), "xxc");
    // cm-vim clamps the count to the line
    assert_eq!(run("abc", "5rx"), "xxx");
}

#[test]
fn replace_mode() {
    let mut e = ed("abcdef");
    e.handle_keys("Rxy");
    assert_eq!(e.mode_label(), "REPLACE");
    e.handle_keys("<Esc>");
    assert_eq!(e.text(), "xycdef");
    assert_eq!(e.mode_label(), "NORMAL");
}

#[test]
fn increment() {
    assert_eq!(run("x 9 y", "<C-a>"), "x 10 y");
    assert_eq!(run("x 9 y", "5<C-x>"), "x 4 y");
    assert_eq!(run("x -1 y", "<C-a>"), "x 0 y");
    assert_eq!(run("0x0f", "<C-a>"), "0x10");
}

// ---- text objects ------------------------------------------------------------

#[test]
fn word_objects() {
    assert_eq!(run("foo bar baz", "wdiw"), "foo  baz");
    assert_eq!(run("foo bar baz", "wdaw"), "foo baz");
    assert_eq!(run("foo bar.x baz", "wdiW"), "foo  baz");
    assert_eq!(run("foo bar baz", "wciwX<Esc>"), "foo X baz");
}

#[test]
fn bracket_objects() {
    assert_eq!(run("f(a, b)", "fadi("), "f()");
    assert_eq!(run("f(a, b)", "fada("), "f");
    assert_eq!(run("f(a, (b))", "fbdib"), "f(a, ())");
    assert_eq!(run("x [1, 2] y", "f1di["), "x [] y");
    assert_eq!(run("x {1} y", "f1di{"), "x {} y");
    assert_eq!(run("x {1} y", "f1diB"), "x {} y");
    assert_eq!(run("x <a> y", "fadi<"), "x <> y");
}

#[test]
fn quote_objects() {
    assert_eq!(run("say \"hi there\" ok", "fhdi\""), "say \"\" ok");
    // cm-vim does not take the trailing space with a"
    assert_eq!(run("say \"hi there\" ok", "fhda\""), "say  ok");
    assert_eq!(run("say 'hi' ok", "fhci'yo<Esc>"), "say 'yo' ok");
    assert_eq!(run("a `b` c", "fbdi`"), "a `` c");
}

#[test]
fn paragraph_objects() {
    assert_eq!(run("a\nb\n\nc", "dip"), "\nc");
    assert_eq!(run("a\nb\n\nc", "dap"), "c");
}

// ---- counts / registers ---------------------------------------------------------

#[test]
fn registers() {
    let mut e = ed("foo bar");
    e.handle_keys("\"ayw");
    assert_eq!(e.register('a').as_deref(), Some("foo "));
    e.handle_keys("\"Ayw");
    assert_eq!(e.register('a').as_deref(), Some("foo foo "));
    e.handle_keys("$\"ap");
    assert_eq!(e.text(), "foo barfoo foo ");
}

#[test]
fn numbered_registers() {
    let mut e = ed("a\nb\nc");
    e.handle_keys("dddd");
    assert_eq!(e.register('1').as_deref(), Some("b\n"));
    assert_eq!(e.register('2').as_deref(), Some("a\n"));
    e.handle_keys("\"2p");
    assert_eq!(e.text(), "c\na");
}

#[test]
fn yank_register_zero() {
    let mut e = ed("foo bar");
    e.handle_keys("ywwdw");
    assert_eq!(e.register('0').as_deref(), Some("foo "));
    assert_eq!(e.register('-').as_deref(), Some("bar"));
}

#[test]
fn black_hole() {
    let mut e = ed("foo bar");
    e.handle_keys("yw\"_dwP");
    assert_eq!(e.text(), "foo bar");
}

#[test]
fn insert_register() {
    let mut e = ed("foo");
    e.handle_keys("yiwA <C-r>\"<Esc>");
    assert_eq!(e.text(), "foo foo");
}

// ---- dot repeat -----------------------------------------------------------------

#[test]
fn dot_repeat() {
    assert_eq!(run("a b c d", "dw."), "c d");
    assert_eq!(run("a b c d e", "dw3."), "e");
    assert_eq!(run("a\nb\nc\nd", "dd."), "c\nd");
    assert_eq!(run("abc", "x.."), "");
    assert_eq!(run("foo foo", "cwbar<Esc>w."), "bar bar");
    assert_eq!(run("a\nb", "Ax<Esc>j."), "ax\nbx");
    assert_eq!(run("a\nb", "ix<Esc>j0."), "xa\nxb");
    assert_eq!(run("a", "3ix<Esc>"), "xxxa");
    assert_eq!(run("a\nb", "oz<Esc>j."), "a\nz\nb\nz");
    assert_eq!(run("hello", "~."), "HEllo");
}

// ---- undo / redo ------------------------------------------------------------------

#[test]
fn undo_redo() {
    let mut e = ed("foo bar");
    e.handle_keys("dw");
    assert_eq!(e.text(), "bar");
    e.handle_keys("u");
    assert_eq!(e.text(), "foo bar");
    e.handle_keys("<C-r>");
    assert_eq!(e.text(), "bar");
    e.handle_keys("u");
    e.handle_keys("ixy<Esc>");
    assert_eq!(e.text(), "xyfoo bar");
    e.handle_keys("u");
    assert_eq!(e.text(), "foo bar");
}

#[test]
fn undo_multiple() {
    let mut e = ed("a b c");
    e.handle_keys("xxx");
    assert_eq!(e.text(), " c");
    e.handle_keys("2u");
    assert_eq!(e.text(), " b c");
    e.handle_keys("u");
    assert_eq!(e.text(), "a b c");
}

// ---- marks --------------------------------------------------------------------------

#[test]
fn marks() {
    assert_eq!(cur("abc\ndef\nghi", "jlmaG`a"), (2, 2));
    assert_eq!(cur("abc\n  def\nghi", "jllmaG'a"), (2, 3));
    assert_eq!(run("a\nb\nc\nd", "jmaGd'a"), "a\n");
    assert_eq!(cur("abc\ndef", "G``"), (1, 1));
}

#[test]
fn marks_follow_edits() {
    assert_eq!(cur("abc\ndef", "jmaggOnew<Esc>`a"), (3, 1));
}

// ---- macros ---------------------------------------------------------------------------

#[test]
fn macros() {
    let mut e = ed("1\n2\n3\n4");
    e.handle_keys("qaA!<Esc>jq");
    assert_eq!(e.recording(), None);
    // insert-mode text is kept as insert-mode changes, not keys
    assert_eq!(e.register('a').as_deref(), Some("Aj"));
    e.handle_keys("@a");
    assert_eq!(e.text(), "1!\n2!\n3\n4");
    e.handle_keys("@@");
    assert_eq!(e.text(), "1!\n2!\n3!\n4");
}

#[test]
fn macro_count() {
    let mut e = ed("a a a a a");
    e.handle_keys("qqrxwq3@q");
    assert_eq!(e.text(), "x x x x a");
}

#[test]
fn recording_indicator() {
    let mut e = ed("x");
    e.handle_keys("qz");
    assert_eq!(e.recording(), Some('z'));
    e.handle_keys("q");
    assert_eq!(e.recording(), None);
}

// ---- jump list ------------------------------------------------------------------------

#[test]
fn jumplist() {
    assert_eq!(cur("a\nb\nc\nd", "G<C-o>"), (1, 1));
    assert_eq!(cur("a\nb\nc\nd", "G<C-o><C-i>"), (4, 1));
}
