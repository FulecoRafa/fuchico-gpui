mod common;
use common::*;
use fuchico_vim::{Effect, PromptKind};

fn ed_noapp(text: &str) -> fuchico_vim::Editor {
    let cfg = fuchico_vim::Config { app_commands: false, ..Default::default() };
    fuchico_vim::Editor::with_config(text, cfg)
}

fn ex(text: &str, cmd: &str) -> String {
    let mut e = ed(text);
    e.execute_command(cmd);
    e.text()
}

#[test]
fn colon_prompt() {
    let mut e = ed("a\nb\nc");
    let effs = e.handle_keys(":");
    assert!(has(&effs, |f| matches!(f, Effect::OpenCommandLine { .. })));
    assert_eq!(e.prompt_kind(), Some(PromptKind::Command));
    assert_eq!(e.prompt_prefix(), Some(":"));
    e.handle_keys("3<CR>");
    assert_eq!(e.prompt_kind(), None);
    assert_eq!(e.line_col(), (3, 1));
}

#[test]
fn colon_in_visual_has_range() {
    let mut e = ed("a\nb\nc");
    e.handle_keys("Vj:");
    assert_eq!(e.prompt_value(), Some("'<,'>"));
    e.handle_keys("d<CR>");
    assert_eq!(e.text(), "c");
}

#[test]
fn write_and_quit() {
    let mut e = ed_noapp("x");
    let effs = e.execute_command("w");
    assert!(has(&effs, |f| matches!(f, Effect::Save)));
    let effs = e.execute_command("write");
    assert!(has(&effs, |f| matches!(f, Effect::Save)));
    // extras are off by default
    let effs = e.execute_command("q");
    assert!(!has(&effs, |f| matches!(f, Effect::Quit)));
    assert_eq!(msg(&e), "Not an editor command \":q\"");
    let mut e = ed_app("x");
    let effs = e.execute_command("q");
    assert!(has(&effs, |f| matches!(f, Effect::Quit)));
    let effs = e.execute_command("wq");
    assert!(has(&effs, |f| matches!(f, Effect::Save)) && has(&effs, |f| matches!(f, Effect::Quit)));
    let effs = e.execute_command("x");
    assert!(has(&effs, |f| matches!(f, Effect::Save)) && has(&effs, |f| matches!(f, Effect::Quit)));
}

#[test]
fn unknown_command() {
    let mut e = ed("x");
    e.execute_command("frobnicate");
    assert_eq!(msg(&e), "Not an editor command \":frobnicate\"");
    e.execute_command("");
    assert_eq!(msg(&e), "Not an editor command \":\"");
}

#[test]
fn version() {
    let mut e = ed("x");
    e.execute_command("ve");
    assert_eq!(msg(&e), "Codemirror-vim version: 6.3.0");
    e.execute_command("version");
    assert_eq!(msg(&e), "Codemirror-vim version: 6.3.0");
}

#[test]
fn line_numbers() {
    let mut e = ed("a\nb\nc\nd");
    e.execute_command("$");
    assert_eq!(e.line_col(), (4, 1));
    e.execute_command("2");
    assert_eq!(e.line_col(), (2, 1));
    e.execute_command(".+1");
    assert_eq!(e.line_col(), (3, 1));
    e.execute_command("-2");
    assert_eq!(e.line_col(), (1, 1));
    e.execute_command("'z");
    assert_eq!(msg(&e), "Error: Mark not set");
}

#[test]
fn substitute() {
    assert_eq!(ex("foo foo\nfoo", "s/foo/bar/"), "bar foo\nfoo");
    assert_eq!(ex("foo foo\nfoo", "s/foo/bar/g"), "bar bar\nfoo");
    assert_eq!(ex("foo foo\nfoo", "%s/foo/bar/"), "bar foo\nbar");
    assert_eq!(ex("foo foo\nfoo", "%s/foo/bar/g"), "bar bar\nbar");
    assert_eq!(ex("a\nb\nc\nd", "2,3s/$/!/"), "a\nb!\nc!\nd");
    assert_eq!(ex("abc", "s/(b)/[$1]/"), "a[b]c");
    assert_eq!(ex("abc", "s/b/[&]/"), "a[b]c");
    assert_eq!(ex("abc", "s/b/[\\&]/"), "a[&]c");
    assert_eq!(ex("a/b", "s#/#-#"), "a-b");
    assert_eq!(ex("a.b", "s/\\./,/"), "a,b");
    assert_eq!(ex("a b", "s/ /\\n/"), "a\nb");
    assert_eq!(ex("Foo", "s/foo/x/i"), "x");
    // no \U case conversion in pcre mode: the backslash stays
    assert_eq!(ex("ab ab", "s/ab/\\U&/g"), "\\Uab \\Uab");
}

#[test]
fn substitute_count_and_repeat() {
    assert_eq!(ex("x\nx\nx\nx", "s/x/y/ 2"), "y\ny\nx\nx");
    let mut e = ed("a a\na a");
    e.execute_command("s/a/b/");
    e.handle_keys("j");
    e.execute_command("s");
    assert_eq!(e.text(), "b a\nb a");
}

#[test]
fn substitute_messages() {
    let mut e = ed("abc");
    e.execute_command("s/zzz/y/");
    // smartcase: an all-lowercase pattern ignores case
    assert_eq!(msg(&e), "No matches for /zzz/im (set nopcre to use vim regexps)");
    let mut e = ed("abc");
    e.execute_command("s");
    assert_eq!(msg(&e), "No previous substitute regular expression");
    let mut e = ed("abc");
    e.execute_command("s/(/x/");
    assert!(msg(&e).starts_with("SyntaxError: Invalid regular expression"), "{}", msg(&e));
}

#[test]
fn substitute_undo_is_one_step() {
    let mut e = ed("a a a\na");
    e.execute_command("%s/a/b/g");
    assert_eq!(e.text(), "b b b\nb");
    e.handle_keys("u");
    assert_eq!(e.text(), "a a a\na");
}

#[test]
fn substitute_confirm() {
    let mut e = ed("x x x");
    e.execute_command("s/x/y/gc");
    assert_eq!(e.confirm_prompt(), Some("replace with y (y/n/a/q/l)"));
    assert_eq!(e.prompt_kind(), None);
    e.handle_keys("y");
    assert_eq!(e.text(), "y x x");
    e.handle_keys("n");
    assert_eq!(e.text(), "y x x");
    e.handle_keys("y");
    assert_eq!(e.text(), "y x y");
    assert_eq!(e.confirm_prompt(), None);
    let mut e = ed("x x x");
    e.execute_command("s/x/y/gc");
    e.handle_keys("a");
    assert_eq!(e.text(), "y y y");
    assert_eq!(e.confirm_prompt(), None);
    let mut e = ed("x x x");
    e.execute_command("s/x/y/gc");
    e.handle_keys("nl");
    assert_eq!(e.text(), "x y x");
    assert_eq!(e.confirm_prompt(), None);
    let mut e = ed("x x x");
    e.execute_command("s/x/y/gc");
    e.handle_keys("q");
    assert_eq!(e.text(), "x x x");
    assert_eq!(e.confirm_prompt(), None);
}

#[test]
fn global() {
    assert_eq!(ex("a1\nb\na2\nc", "g/a/d"), "b\nc");
    assert_eq!(ex("a1\nb\na2\nc", "v/a/d"), "a1\na2");
    assert_eq!(ex("a1\nb\na2\nc", "g!/a/d"), "a1\na2");
    assert_eq!(ex("a\nb\na", "g/a/s/a/x/"), "x\nb\nx");
    assert_eq!(ex("a\nb\na", "g/a/normal Ay"), "ay\nb\nay");
    let mut e = ed("a1\nb\na2");
    e.execute_command("g/a");
    assert_eq!(msg(&e), "a1\na2");
    e.execute_command("g");
    assert_eq!(msg(&e), "Regular Expression missing from global");
}

#[test]
fn global_with_substitute_confirm() {
    let mut e = ed("a\nb\na");
    e.execute_command("g/a/s/a/x/c");
    assert!(e.confirm_prompt().is_some());
    e.handle_keys("y");
    assert!(e.confirm_prompt().is_some());
    e.handle_keys("y");
    assert_eq!(e.text(), "x\nb\nx");
    assert_eq!(e.confirm_prompt(), None);
}

#[test]
fn sort() {
    assert_eq!(ex("c\na\nb", "sort"), "a\nb\nc");
    assert_eq!(ex("c\na\nb", "sort!"), "c\nb\na");
    assert_eq!(ex("B\na\nC", "sort i"), "a\nB\nC");
    assert_eq!(ex("B\na\nC", "sort"), "B\nC\na");
    assert_eq!(ex("x10\nx9\nx100", "sort n"), "x9\nx10\nx100");
    assert_eq!(ex("b\na\nb\na", "sort u"), "a\nb");
    assert_eq!(ex("0x1F\n0xA\n0x2", "sort x"), "0x2\n0xA\n0x1F");
    assert_eq!(ex("d\nc\nb\na", "2,3sort"), "d\nb\nc\na");
    assert_eq!(ex("a2 z\nb1 y", "sort /\\d/"), "b1 y\na2 z");
    let mut e = ed("a\nb");
    e.execute_command("sort q");
    // the message keeps argString's leading space
    assert_eq!(msg(&e), "Invalid arguments:  q");
}

#[test]
fn normal_command() {
    assert_eq!(ex("a\nb\nc", "%normal Ax"), "ax\nbx\ncx");
    assert_eq!(ex("a\nb\nc", "2,3norm Ix"), "a\nxb\nxc");
    assert_eq!(ex("abc", "normal x"), "bc");
    let mut e = ed("abc");
    e.execute_command("normal");
    assert_eq!(msg(&e), "Argument is required.");
}

#[test]
fn normal_with_mapping() {
    let mut e = ed("abc");
    e.execute_command("nmap X dd");
    e.execute_command("normal X");
    assert_eq!(e.text(), "");
    let mut e = ed("abc\ndef");
    e.execute_command("nmap x dd");
    e.execute_command("normal! x");
    assert_eq!(e.text(), "bc\ndef");
}

#[test]
fn delete_yank_join() {
    assert_eq!(ex("a\nb\nc", "2d"), "a\nc");
    assert_eq!(ex("a\nb\nc", "1,2d"), "c");
    // cm-vim passes `lineEnd - line` as the count, so `:%j` joins one line less
    assert_eq!(ex("a\nb\nc", "%j"), "a b\nc");
    assert_eq!(ex("a\nb\nc\nd", "1,4j"), "a b c\nd");
    assert_eq!(ex("a\nb\nc", "j"), "a b\nc");
    let mut e = ed("a\nb");
    e.execute_command("2");
    e.execute_command("y");
    assert_eq!(e.register('0').as_deref(), Some("b\n"));
}

#[test]
fn undo_redo_commands() {
    let mut e = ed("abc");
    e.handle_keys("x");
    e.execute_command("u");
    assert_eq!(e.text(), "abc");
    e.execute_command("red");
    assert_eq!(e.text(), "bc");
}

#[test]
fn maps() {
    let mut e = ed("foo bar");
    e.execute_command("map Q dw");
    e.handle_keys("Q");
    assert_eq!(e.text(), "bar");
    e.execute_command("unmap Q");
    e.handle_keys("Q");
    assert_eq!(e.text(), "bar");
    e.execute_command("unmap Q");
    assert_eq!(msg(&e), "No such mapping: unmap Q");
    e.execute_command("map Q");
    assert_eq!(msg(&e), "Invalid mapping: map Q");
}

#[test]
fn noremap_vs_map() {
    let mut e = ed("abc def");
    e.execute_command("nmap x w");
    e.execute_command("nnoremap Z x");
    e.handle_keys("Z");
    assert_eq!(e.text(), "bc def");
    e.execute_command("nmap Y x");
    e.handle_keys("Y");
    assert_eq!(e.line_col(), (1, 4));
}

#[test]
fn mode_specific_maps() {
    let mut e = ed("abc");
    e.execute_command("vmap Q d");
    e.handle_keys("Q");
    assert_eq!(e.text(), "abc");
    e.handle_keys("vlQ");
    assert_eq!(e.text(), "c");
}

#[test]
fn mapclear() {
    let mut e = ed("foo bar");
    e.execute_command("nmap Q dw");
    e.execute_command("imap jj <Esc>");
    e.execute_command("nmapclear");
    e.handle_keys("Q");
    assert_eq!(e.text(), "foo bar");
    e.handle_keys("ijj");
    assert_eq!(e.mode_label(), "NORMAL");
    e.execute_command("mapclear");
    e.handle_keys("ijj<Esc>");
    assert_eq!(e.text(), "jjfoo bar");
}

#[test]
fn ex_mappings() {
    let mut e = ed("a\nb");
    e.execute_command("map :foo :2");
    e.execute_command("foo");
    assert_eq!(e.line_col(), (2, 1));
    e.execute_command("map :bar dd");
    e.execute_command("bar");
    assert_eq!(e.text(), "a");
    e.execute_command("nmap :baz x");
    assert_eq!(msg(&e), "Error: Mode not supported for ex mappings");
    e.execute_command("unmap :bar");
    e.execute_command("bar");
    assert_eq!(msg(&e), "Not an editor command \":bar\"");
}

#[test]
fn key_to_ex_mapping() {
    let mut e = ed("a\nb\nc");
    e.execute_command("nmap Q :3<CR>");
    e.handle_keys("Q");
    assert_eq!(e.line_col(), (3, 1));
}

#[test]
fn set_options() {
    let mut e = ed("x");
    e.execute_command("set pcre?");
    assert_eq!(msg(&e), " pcre");
    e.execute_command("set nopcre");
    e.execute_command("set pcre?");
    assert_eq!(msg(&e), " nopcre");
    e.execute_command("set pcre!");
    e.execute_command("set pcre?");
    assert_eq!(msg(&e), " pcre");
    e.execute_command("set tw=40");
    e.execute_command("set tw");
    assert_eq!(msg(&e), "  tw=40");
    e.execute_command("set textwidth");
    assert_eq!(msg(&e), "  textwidth=40");
    e.execute_command("set bogus");
    assert_eq!(msg(&e), "Unknown option: bogus");
    e.execute_command("set");
    assert_eq!(msg(&e), "Invalid mapping: set");
    e.execute_command("set pcre?=1");
    assert_eq!(msg(&e), "Error: Trailing characters:  pcre?=1");
    e.execute_command("setlocal tw=10");
    // textwidth is a buffer-local callback option: no global value
    e.execute_command("setglobal tw?");
    assert_eq!(msg(&e), "  tw=undefined");
    e.execute_command("setlocal tw?");
    assert_eq!(msg(&e), "  tw=10");
    e.execute_command("setlocal nopcre");
    e.execute_command("setglobal pcre?");
    assert_eq!(msg(&e), " pcre");
    e.execute_command("setlocal pcre?");
    assert_eq!(msg(&e), " nopcre");
}

#[test]
fn nopcre_vim_regex() {
    let mut e = ed("foo(bar)");
    e.execute_command("set nopcre");
    e.execute_command("s/\\(bar\\)/[\\1]/");
    assert_eq!(e.text(), "foo([bar])");
}

#[test]
fn registers_and_marks_listing() {
    let mut e = ed("foo\nbar");
    e.handle_keys("\"ayiwjma");
    e.execute_command("registers a");
    assert_eq!(
        msg(&e),
        "----------Registers----------\n\n\"a    foo\n\nPress ENTER or type command to continue"
    );
    // Enter dismisses the notification
    e.handle_keys("<CR>");
    assert_eq!(e.line_col(), (2, 1));
    e.execute_command("marks a");
    assert_eq!(
        msg(&e),
        "-----------Marks-----------\nmark\tline\tcol\n\na\t1\t0\n\nPress ENTER or type command to continue"
    );
}

#[test]
fn delmarks() {
    let mut e = ed("a\nb\nc");
    e.handle_keys("majmbjmc");
    e.execute_command("delmarks a-b");
    e.handle_keys("`a");
    assert_eq!(e.line_col(), (3, 1));
    e.handle_keys("`c");
    assert_eq!(e.line_col(), (3, 1));
    e.execute_command("delm c");
    e.handle_keys("gg`c");
    assert_eq!(e.line_col(), (1, 1));
    e.execute_command("delm");
    assert_eq!(msg(&e), "Argument required");
    e.execute_command("delm b-a");
    assert_eq!(msg(&e), "Invalid argument:  b-a");
    e.execute_command("delm a-B");
    assert_eq!(msg(&e), "Invalid argument: a-");
}

#[test]
fn colorscheme() {
    let mut e = ed("x");
    e.execute_command("colo");
    assert_eq!(msg(&e), "default");
    let effs = e.execute_command("colorscheme nord");
    assert!(has(&effs, |f| matches!(f, Effect::ChangeTheme(t) if t == "nord")));
    e.execute_command("colo");
    assert_eq!(msg(&e), "nord");
}

#[test]
fn startinsert() {
    let mut e = ed("abc");
    e.execute_command("startinsert");
    assert_eq!(e.mode_label(), "INSERT");
    assert_eq!(e.line_col(), (1, 1));
    let mut e = ed("abc");
    e.execute_command("startinsert!");
    assert_eq!(e.line_col(), (1, 4));
}

#[test]
fn ex_history_and_register() {
    let mut e = ed("a\nb\nc");
    e.handle_keys(":2<CR>:3<CR>");
    assert_eq!(e.register(':').as_deref(), Some("3"));
    e.handle_keys(":");
    assert_eq!(e.prompt_history(true).as_deref(), Some("3"));
    assert_eq!(e.prompt_history(true).as_deref(), Some("2"));
    e.prompt_close(false);
    e.handle_keys("gg@:");
    assert_eq!(e.line_col(), (3, 1));
}

#[test]
fn completions() {
    let mut e = ed_noapp("x");
    e.handle_keys(":");
    let c = e.prompt_completions("nohl");
    assert_eq!(c.options, vec!["nohlsearch".to_string()]);
    let c = e.prompt_completions("set tw");
    assert!(c.options.is_empty());
    let c = e.prompt_completions("set pc");
    assert_eq!(c.options, vec!["pcre".to_string()]);
    assert_eq!(e.prompt_accept_completion("%subs", "substitute"), "%substitute");
    // extras hidden without app_commands
    assert!(!e.prompt_completions("qu").options.contains(&"quit".to_string()));
    e.prompt_close(false);
}

#[test]
fn substitute_highlight_while_typing() {
    let mut e = ed("foo bar foo");
    e.handle_keys(":");
    e.prompt_input("%s/foo");
    assert_eq!(e.search_highlight(0, 100), vec![(0, 3), (8, 11)]);
    e.prompt_input("2");
    assert!(e.search_highlight(0, 100).is_empty());
    e.prompt_close(false);
}

#[test]
fn commands_markdown_lists_everything() {
    let md = fuchico_vim::commands_markdown();
    assert!(md.contains("### normal"));
    assert!(md.contains("### ex commands"));
    assert!(md.contains("| `:s[ubstitute]` | `substitute` |"));
    assert!(md.contains("| `:q[uit]` | `quit` | extension |"));
    assert!(md.contains("`moveByWords`"));
}
