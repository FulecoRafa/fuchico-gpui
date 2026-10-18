//! Behaviour tests. Selections are written inline in the text:
//!
//! * `|` primary head, `^` primary anchor (omit `^` for an empty cursor);
//! * `¦` secondary head, `°` secondary anchor (the n-th `¦` pairs with the
//!   n-th `°`), `¡` an empty secondary cursor.
//!
//! Positions are CodeMirror ones, so a normal-mode block cursor on `l` in
//! `hello` is `he|l^lo` (anchor after the char, head before it).

mod editing;
mod insert;
mod keymap_table;
mod modes;
mod motion;
mod prompt;
mod registers;
mod search;
mod selection_ops;
mod textobjects;
mod undo;

use crate::{Config, Editor, Effect, MemoryHost, Range, Selection};

pub(crate) fn parse(s: &str) -> (String, Selection) {
    let mut text = String::new();
    let mut pos = 0;
    let (mut ph, mut pa) = (None, None);
    let mut sec_heads = Vec::new();
    let mut sec_anchors = Vec::new();
    let mut empties = Vec::new();
    for c in s.chars() {
        match c {
            '|' => ph = Some(pos),
            '^' => pa = Some(pos),
            '¦' => sec_heads.push(pos),
            '°' => sec_anchors.push(pos),
            '¡' => empties.push(pos),
            c => {
                text.push(c);
                pos += 1;
            }
        }
    }
    let ph = ph.expect("primary head `|`");
    let primary = Range::new(pa.unwrap_or(ph), ph);
    assert_eq!(
        sec_heads.len(),
        sec_anchors.len(),
        "unpaired secondary markers in {s:?}"
    );
    let mut ranges: Vec<Range> = sec_heads
        .iter()
        .zip(&sec_anchors)
        .map(|(&h, &a)| Range::new(a, h))
        .collect();
    ranges.extend(empties.iter().map(|&p| Range::cursor(p)));
    ranges.push(primary);
    ranges.sort_by_key(|r| (r.from(), r.to()));
    let main = ranges.iter().position(|r| *r == primary).unwrap();
    (text, Selection::create(ranges, main))
}

pub(crate) fn render(ed: &Editor) -> String {
    let sel = ed.selection();
    // (pos, order, mark): range ends, then empty cursors, then range starts
    let mut marks: Vec<(usize, usize, char)> = Vec::new();
    for (i, r) in sel.ranges().iter().enumerate() {
        let main = i == sel.main_index();
        if r.is_empty() {
            marks.push((r.head, 1, if main { '|' } else { '¡' }));
        } else {
            let (h, a) = if main { ('|', '^') } else { ('¦', '°') };
            marks.push((r.head, if r.head < r.anchor { 2 } else { 0 }, h));
            marks.push((r.anchor, if r.anchor < r.head { 2 } else { 0 }, a));
        }
    }
    marks.sort();
    let mut out = String::new();
    let mut mi = 0;
    for (i, c) in ed.text().chars().enumerate() {
        while mi < marks.len() && marks[mi].0 == i {
            out.push(marks[mi].2);
            mi += 1;
        }
        out.push(c);
    }
    while mi < marks.len() {
        out.push(marks[mi].2);
        mi += 1;
    }
    out
}

/// An editor with exactly cm-helix's behaviour (no extensions).
pub(crate) fn ed(s: &str) -> Editor {
    ed_with(s, Config::parity())
}

/// An editor with Fuchico's defaults (extensions + markup continuation).
pub(crate) fn ed_ext(s: &str) -> Editor {
    ed_with(s, Config::default())
}

pub(crate) fn ed_with(s: &str, config: Config) -> Editor {
    let (text, sel) = parse(s);
    let mut e = Editor::with_host(&text, config, Box::new(MemoryHost::default()));
    e.set_selection(sel);
    e
}

/// Run `keys` from `before` (parity config) and compare with `after`.
#[track_caller]
pub(crate) fn check(before: &str, keys: &str, after: &str) {
    let mut e = ed(before);
    e.handle_keys(keys);
    assert_eq!(render(&e), after, "keys {keys:?} from {before:?}");
}

/// Like [`check`] with the default (extensions) config.
#[track_caller]
pub(crate) fn check_ext(before: &str, keys: &str, after: &str) {
    let mut e = ed_ext(before);
    e.handle_keys(keys);
    assert_eq!(render(&e), after, "keys {keys:?} from {before:?}");
}

pub(crate) fn messages(effects: &[Effect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Message { text, error } => Some((text.clone(), *error)),
            _ => None,
        })
        .collect()
}

#[test]
fn notation_roundtrip() {
    for s in [
        "he|l^lo",
        "|",
        "a|b",
        "^ab|c",
        "|ab^c",
        "°a¦b |c^d",
        "x¡y|z",
        "|a^°b¦",
    ] {
        let e = ed(s);
        assert_eq!(render(&e), s);
    }
}
