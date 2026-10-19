mod common;
use common::*;
use fuchico_vim::{Effect, PromptKind};

#[test]
fn slash_search() {
    let mut e = ed("foo bar\nbaz bar");
    let effs = e.handle_keys("/");
    assert!(has(&effs, |f| matches!(f, Effect::OpenSearch { .. })));
    assert_eq!(e.prompt_kind(), Some(PromptKind::Search(fuchico_vim::SearchKind::Forward)));
    assert_eq!(e.prompt_prefix(), Some("/"));
    e.handle_keys("bar<CR>");
    assert_eq!(e.prompt_kind(), None);
    assert_eq!(e.line_col(), (1, 5));
    e.handle_keys("n");
    assert_eq!(e.line_col(), (2, 5));
    e.handle_keys("n");
    assert_eq!(e.line_col(), (1, 5));
    e.handle_keys("N");
    assert_eq!(e.line_col(), (2, 5));
}

#[test]
fn backward_search() {
    let mut e = ed("foo bar\nbaz bar");
    e.handle_keys("G$?ba<CR>");
    assert_eq!(e.line_col(), (2, 5));
    e.handle_keys("n");
    assert_eq!(e.line_col(), (2, 1));
    e.handle_keys("N");
    assert_eq!(e.line_col(), (2, 5));
}

#[test]
fn search_count_and_operator() {
    assert_eq!(cur("a x b x c x", "2/x<CR>"), (1, 7));
    assert_eq!(run("foo bar baz", "d/baz<CR>"), "baz");
    assert_eq!(run("foo bar baz", "dn"), "foo bar baz");
}

#[test]
fn search_smartcase_and_regex() {
    assert_eq!(cur("Foo foo", "/foo<CR>"), (1, 5));
    assert_eq!(cur("xx Foo foo", "/foo<CR>"), (1, 4));
    assert_eq!(cur("xx Foo foo", "/Foo<CR>n"), (1, 4));
    assert_eq!(cur("a1 b22 c333", "/\\d{3}<CR>"), (1, 9));
    assert_eq!(cur("a1 b22 c333", "/b\\d+<CR>"), (1, 4));
}

#[test]
fn search_escape_restores() {
    let mut e = ed("foo bar");
    e.handle_keys("/bar");
    assert_eq!(e.prompt_value(), Some("bar"));
    e.handle_keys("<Esc>");
    assert_eq!(e.prompt_kind(), None);
    assert_eq!(e.line_col(), (1, 1));
}

#[test]
fn incremental_highlight() {
    let mut e = ed("foo bar foo");
    e.handle_keys("/");
    e.prompt_input("foo");
    assert_eq!(e.search_highlight(0, 100), vec![(0, 3), (8, 11)]);
    e.prompt_close(true);
    assert_eq!(e.line_col(), (1, 9));
    e.execute_command("nohlsearch");
    assert!(e.search_highlight(0, 100).is_empty());
}

#[test]
fn search_history() {
    let mut e = ed("aa bb cc");
    e.handle_keys("/bb<CR>/cc<CR>/");
    assert_eq!(e.prompt_history(true).as_deref(), Some("cc"));
    assert_eq!(e.prompt_history(true).as_deref(), Some("bb"));
    assert_eq!(e.prompt_history(false).as_deref(), Some("cc"));
    e.prompt_close(false);
}

#[test]
fn search_not_found() {
    let mut e = ed("abc");
    e.handle_keys("/zzz<CR>");
    assert_eq!(e.line_col(), (1, 1));
    assert!(msg(&e).contains("No match"), "{}", msg(&e));
}

#[test]
fn search_shared_between_editors() {
    let mut a = ed("x foo");
    a.handle_keys("/foo<CR>");
    let g = a.take_globals();
    let mut b = ed("foo y foo");
    b.set_globals(g);
    b.handle_keys("n");
    assert_eq!(b.line_col(), (1, 7));
}

#[test]
fn search_in_visual() {
    let mut e = ed("foo bar baz");
    e.handle_keys("v/baz<CR>d");
    assert_eq!(e.text(), "az");
}

#[test]
fn search_dot_repeat_with_macro() {
    let mut e = ed("a x\nb x\nc x");
    e.handle_keys("qa/x<CR>rYq@a");
    assert_eq!(e.text(), "a Y\nb Y\nc x");
}
