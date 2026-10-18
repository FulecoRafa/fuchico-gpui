//! `/` search prompt, `n`/`N`, `*`, `s` select-regex, S/K/Alt-K, global
//! search.

use super::*;
use crate::effect::{External, Scroll, SearchKind};
use crate::prompt::PromptKind;
use crate::{ExternalCommands, Mode};

fn with_search(s: &str, pattern: &str) -> Editor {
    let mut e = ed(s);
    let mut regs = e.registers().clone();
    regs.write('/', vec![pattern.to_string()]);
    e.set_registers(regs);
    e
}

#[test]
fn slash_opens_prompt_with_register_placeholder() {
    let mut e = with_search("|h^ello", "ab");
    let fx = e.handle_keys("/");
    assert!(fx.contains(&Effect::OpenSearch {
        kind: SearchKind::Forward,
        placeholder: Some("ab".into())
    }));
    assert_eq!(
        e.prompt_kind(),
        Some(PromptKind::Search(SearchKind::Forward))
    );
    let mut e = ed("|h^ello");
    let fx = e.handle_keys("/");
    assert!(fx.contains(&Effect::OpenSearch {
        kind: SearchKind::Forward,
        placeholder: None
    }));
}

#[test]
fn search_preview_and_accept() {
    let mut e = ed("|h^ello hello");
    e.handle_keys("/");
    let fx = e.prompt_input("llo");
    assert_eq!(render(&e), "he^llo| hello");
    assert!(
        fx.iter()
            .any(|f| matches!(f, Effect::Scroll(Scroll::To { .. })))
    );
    e.prompt_input("llo h");
    assert_eq!(render(&e), "he^llo h|ello");
    e.prompt_close(true);
    assert_eq!(e.prompt_kind(), None);
    assert_eq!(e.registers().get('/').unwrap(), &vec!["llo h".to_string()]);
    assert_eq!(render(&e), "he^llo h|ello");
}

#[test]
fn search_preview_wraps_to_start() {
    let mut e = ed("abc ab|c^");
    e.handle_keys("/");
    e.prompt_input("ab");
    assert_eq!(render(&e), "^ab|c abc");
}

#[test]
fn search_preview_no_match_restores_selection() {
    let mut e = ed("|h^ello");
    e.handle_keys("/");
    e.prompt_input("ll");
    assert_eq!(render(&e), "he^ll|o");
    let fx = e.prompt_input("zz");
    assert_eq!(render(&e), "|h^ello");
    assert!(fx.contains(&Effect::Scroll(Scroll::RestorePromptStart)));
}

#[test]
fn search_invalid_regex_keeps_preview() {
    let mut e = ed("|h^ello");
    e.handle_keys("/");
    e.prompt_input("l");
    let before = render(&e);
    e.prompt_input("l(");
    assert_eq!(render(&e), before);
}

#[test]
fn search_cancel_restores() {
    let mut e = ed("|h^ello");
    e.handle_keys("/");
    e.prompt_input("ll");
    let fx = e.prompt_close(false);
    assert_eq!(render(&e), "|h^ello");
    assert!(fx.contains(&Effect::Scroll(Scroll::RestorePromptStart)));
    assert!(e.registers().get('/').is_none());
}

#[test]
fn search_accept_empty_yanks_empty_pattern() {
    // cm-helix starts the prompt with `input = ""` and yanks it on accept.
    let mut e = with_search("|h^ello", "ll");
    e.handle_keys("/");
    e.prompt_close(true);
    assert_eq!(e.registers().get('/').unwrap(), &vec![String::new()]);
    // An empty pattern means no search facet: `n` does nothing.
    let fx = e.handle_keys("n");
    assert_eq!(render(&e), "|h^ello");
    assert!(messages(&fx).is_empty());
}

#[test]
fn n_next_match_and_wrap() {
    let mut e = with_search("|a^b ab ab", "ab");
    e.handle_keys("n");
    assert_eq!(render(&e), "ab ^ab| ab");
    e.handle_keys("n");
    assert_eq!(render(&e), "ab ab ^ab|");
    let fx = e.handle_keys("n");
    assert_eq!(render(&e), "^ab| ab ab");
    assert_eq!(
        messages(&fx),
        vec![("Wrapped around document".to_string(), false)]
    );
}

#[test]
fn n_with_count() {
    let mut e = with_search("|x^ ab ab ab", "ab");
    e.handle_keys("2n");
    assert_eq!(render(&e), "x ab ^ab| ab");
}

#[test]
fn n_no_matches() {
    let mut e = with_search("|x^yz", "ab");
    let fx = e.handle_keys("n");
    assert_eq!(messages(&fx), vec![("No more matches".to_string(), true)]);
    assert_eq!(render(&e), "|x^yz");
}

#[test]
fn n_invalid_regex() {
    let mut e = with_search("|x^yz", "a(");
    let fx = e.handle_keys("n");
    let m = messages(&fx);
    assert_eq!(m.len(), 1);
    assert!(m[0].0.starts_with("Invalid regex /a(/: "), "{m:?}");
    assert!(m[0].1);
}

#[test]
fn n_in_select_mode_adds_range() {
    let mut e = with_search("|a^b ab", "ab");
    e.handle_keys("vn");
    assert_eq!(render(&e), "¦a°b ^ab|");
    assert_eq!(e.selection().len(), 2);
}

#[test]
fn shift_n_previous_match() {
    let mut e = with_search("ab ab a|b^", "ab");
    e.handle_keys("N");
    assert_eq!(render(&e), "ab ^ab| ab");
    e.handle_keys("N");
    assert_eq!(render(&e), "^ab| ab ab");
    let fx = e.handle_keys("N");
    assert_eq!(render(&e), "ab ab ^ab|");
    assert_eq!(
        messages(&fx),
        vec![("Wrapped around document".to_string(), false)]
    );
}

#[test]
fn shift_n_with_count_ring_quirk() {
    // cm-helix's Ring.start is off by one once full: `2N` lands on the
    // closest match instead of the second closest.
    let mut e = with_search("ab ab ab |x^", "ab");
    e.handle_keys("2N");
    assert_eq!(render(&e), "ab ab ^ab| x");
    let mut e = with_search("ab ab ab ab |x^", "ab");
    e.handle_keys("3N");
    assert_eq!(render(&e), "ab ab ^ab| ab x");
    assert_eq!(e.mode(), Mode::NORMAL);
}

#[test]
fn shift_n_no_matches() {
    let mut e = with_search("|x^yz", "ab");
    let fx = e.handle_keys("N");
    assert_eq!(messages(&fx), vec![("No more matches".to_string(), true)]);
}

#[test]
fn star_sets_register() {
    let mut e = ed("^foo| bar");
    let fx = e.handle_keys("*");
    assert_eq!(e.registers().get('/').unwrap(), &vec!["foo".to_string()]);
    assert_eq!(
        messages(&fx),
        vec![("register '/' set to 'foo'".to_string(), false)]
    );
    e.handle_keys("n");
    assert_eq!(render(&e), "^foo| bar");
}

#[test]
fn star_escapes_regex() {
    let mut e = ed("^a.b| axb a.b");
    e.handle_keys("*n");
    assert_eq!(render(&e), "a.b axb ^a.b|");
}

#[test]
fn select_regex() {
    let mut e = ed("^one two one|");
    let fx = e.handle_keys("s");
    assert!(fx.iter().any(|f| matches!(
        f,
        Effect::OpenSearch {
            kind: SearchKind::Selection,
            ..
        }
    )));
    e.prompt_input("one");
    assert_eq!(render(&e), "^one| two °one¦");
    e.prompt_close(true);
    assert_eq!(e.selection().len(), 2);
    assert_eq!(e.registers().get('/').unwrap(), &vec!["one".to_string()]);
}

#[test]
fn select_regex_no_match_keeps_initial() {
    let mut e = ed("^one two|");
    e.handle_keys("s");
    e.prompt_input("zz");
    assert_eq!(render(&e), "^one two|");
    e.prompt_input("o");
    e.prompt_close(false);
    assert_eq!(render(&e), "^one two|");
}

#[test]
fn split_keep_remove_are_extensions() {
    let mut e = ed("^a,b|");
    let fx = e.handle_keys("S");
    assert!(!fx.iter().any(|f| matches!(f, Effect::OpenSearch { .. })));

    let mut e = ed_ext("^a,b,c|");
    e.handle_keys("S");
    e.prompt_input(",");
    e.prompt_close(true);
    // primary is the first range; check the pieces
    let parts: Vec<(usize, usize)> = e
        .selection()
        .ranges()
        .iter()
        .map(|r| (r.from(), r.to()))
        .collect();
    assert_eq!(parts, vec![(0, 1), (2, 3), (4, 5)]);
}

#[test]
fn keep_and_remove_selections() {
    let mut e2 = ed_ext("ab cd ae|");
    let sel = Selection::create(
        vec![Range::new(0, 2), Range::new(3, 5), Range::new(6, 8)],
        0,
    );
    e2.set_selection(sel.clone());
    e2.handle_keys("K");
    e2.prompt_input("a");
    e2.prompt_close(true);
    let kept: Vec<(usize, usize)> = e2
        .selection()
        .ranges()
        .iter()
        .map(|r| (r.from(), r.to()))
        .collect();
    assert_eq!(kept, vec![(0, 2), (6, 8)]);

    e2.set_selection(sel);
    e2.handle_keys("<A-K>");
    e2.prompt_input("a");
    e2.prompt_close(true);
    let kept: Vec<(usize, usize)> = e2
        .selection()
        .ranges()
        .iter()
        .map(|r| (r.from(), r.to()))
        .collect();
    assert_eq!(kept, vec![(3, 5)]);
}

#[test]
fn global_search_needs_external() {
    let mut e = ed("|a^b");
    e.handle_keys(" /");
    e.prompt_input("ab");
    let fx = e.prompt_close(true);
    assert!(!fx.iter().any(|f| matches!(f, Effect::External(_))));
    assert_eq!(render(&e), "|a^b", "global search does not move the cursor");

    let mut cfg = Config::parity();
    cfg.externals = ExternalCommands {
        global_search: true,
        ..Default::default()
    };
    let mut e = ed_with("|a^b", cfg);
    let fx = e.handle_keys(" /");
    assert!(fx.iter().any(|f| matches!(
        f,
        Effect::OpenSearch {
            kind: SearchKind::Global,
            ..
        }
    )));
    e.prompt_input("needle");
    let fx = e.prompt_close(true);
    assert!(fx.contains(&Effect::External(External::GlobalSearch("needle".into()))));
    // Empty input falls back to the `/` register (just set to "needle").
    e.handle_keys(" /");
    let fx = e.prompt_close(true);
    assert!(
        !fx.iter().any(|f| matches!(f, Effect::External(_))),
        "register was overwritten with \"\""
    );
}

#[test]
fn search_history_and_completions() {
    let mut e = ed("|a^b");
    for p in ["foo", "bar", "fob"] {
        e.handle_keys("/");
        e.prompt_input(p);
        e.prompt_close(true);
    }
    e.handle_keys("/");
    assert_eq!(
        e.prompt_completions("fo").options,
        vec!["fob".to_string(), "foo".to_string()]
    );
    assert_eq!(e.prompt_history(true).as_deref(), Some("fob"));
    assert_eq!(e.prompt_history(true).as_deref(), Some("bar"));
    assert_eq!(e.prompt_history(true).as_deref(), Some("foo"));
    assert_eq!(e.prompt_history(true), None);
    assert_eq!(e.prompt_history(false).as_deref(), Some("bar"));
    assert_eq!(e.prompt_accept_completion("fo", "foo"), "foo");
}
