//! Pure text commands behind the editor shortcuts (port of
//! `src/modules/editor/lib/shortcuts.ts` + `taskHelpers.ts` pickers).
//!
//! Everything works on **byte** offsets of a `&str`; the view converts to
//! and from the engine's char offsets. A [`CommandEdit`] is a set of
//! non-overlapping replacements in the old document plus the selection
//! (in the new document) to install afterwards.

use std::ops::Range;

use chrono::NaiveDateTime;
use fuchico_core::task_helpers::{self, DUE_RE, RECUR_RE};
use regex::Regex;
use std::sync::LazyLock;

/// Replacements (old-document byte ranges, sorted, non-overlapping) and the
/// new selection (`(anchor, head)` byte offsets in the new document; empty =
/// keep the engine's mapped selection).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandEdit {
    pub changes: Vec<(Range<usize>, String)>,
    pub selection: Vec<(usize, usize)>,
}

impl CommandEdit {
    /// Apply to `doc` (for tests and the host).
    pub fn apply(&self, doc: &str) -> String {
        let mut out = doc.to_string();
        for (r, s) in self.changes.iter().rev() {
            out.replace_range(r.clone(), s);
        }
        out
    }
}

static CHECKBOX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([ xX])\]").unwrap());

fn line_bounds(doc: &str, pos: usize) -> Range<usize> {
    let pos = pos.min(doc.len());
    let start = doc[..pos].rfind('\n').map_or(0, |i| i + 1);
    let end = doc[pos..].find('\n').map_or(doc.len(), |i| pos + i);
    start..end
}

/// `toggleCheckboxAtCursor` (Mod-Enter): toggles the first `[ ]`/`[x]` on
/// the line of every head. `None` when no line has a checkbox.
pub fn toggle_checkbox(doc: &str, heads: &[usize]) -> Option<CommandEdit> {
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    for &h in heads {
        let line = line_bounds(doc, h);
        let Some(c) = CHECKBOX_RE.captures(&doc[line.clone()]) else { continue };
        let m = c.get(0).unwrap();
        let from = line.start + m.start();
        let checked = matches!(c.get(1).unwrap().as_str(), "x" | "X");
        if changes.iter().any(|(r, _)| r.start == from) {
            continue;
        }
        changes.push((from..from + 3, if checked { "[ ]" } else { "[x]" }.into()));
    }
    if changes.is_empty() {
        return None;
    }
    changes.sort_by_key(|(r, _)| r.start);
    Some(CommandEdit { changes, selection: Vec::new() })
}

/// `formatDate`: `YYYY-MM-DD`.
pub fn format_date(now: &NaiveDateTime) -> String {
    now.format("%Y-%m-%d").to_string()
}

/// `formatDateTime`: `YYYY-MM-DD HH:MM`.
pub fn format_date_time(now: &NaiveDateTime) -> String {
    now.format("%Y-%m-%d %H:%M").to_string()
}

/// `insertAtCursor`: replace every selection with `text`, cursors after it.
pub fn insert_at_cursor(ranges: &[Range<usize>], text: &str) -> CommandEdit {
    let mut sorted: Vec<Range<usize>> = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);
    let mut delta: isize = 0;
    let mut selection = Vec::new();
    let mut changes = Vec::new();
    for r in sorted {
        let at = (r.start as isize + delta) as usize + text.len();
        selection.push((at, at));
        delta += text.len() as isize - r.len() as isize;
        changes.push((r, text.to_string()));
    }
    CommandEdit { changes, selection }
}

/// `insertRegion` (Mod-Shift-r): an empty range inserts
/// `<start> Region\n<end>\n` with the cursor after the start marker; a
/// non-empty range wraps its lines and stays selected.
pub fn insert_region(doc: &str, ranges: &[Range<usize>], start: &str, end: &str) -> CommandEdit {
    let mut sorted: Vec<Range<usize>> = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    let mut selection = Vec::new();
    let mut delta: isize = 0;
    for r in sorted {
        let shift = |p: usize, d: isize| (p as isize + d) as usize;
        if r.is_empty() {
            let insert = format!("{start} Region\n{end}\n");
            selection.push((shift(r.start, delta) + start.len() + 1, shift(r.start, delta) + start.len() + 1));
            delta += insert.len() as isize;
            changes.push((r.start..r.start, insert));
        } else {
            let sl = line_bounds(doc, r.start).start;
            let el = line_bounds(doc, r.end).end;
            let ins_start = format!("{start} Region\n");
            let ins_end = format!("\n{end}");
            let a = shift(sl, delta);
            let h = shift(el, delta) + ins_start.len() + ins_end.len();
            selection.push((a, h));
            delta += (ins_start.len() + ins_end.len()) as isize;
            changes.push((sl..sl, ins_start));
            changes.push((el..el, ins_end));
        }
    }
    changes.sort_by_key(|(r, _)| r.start);
    CommandEdit { changes, selection }
}

/// `insertTable` (Mod-Alt-t): the default table replacing the main range
/// (line breaks added around it as needed). Returns the edit and the table
/// start offset in the new document (the cursor is parked at `start + 1`).
pub fn insert_table(doc: &str, range: Range<usize>) -> (CommandEdit, usize) {
    let lead = range.start > 0 && doc.as_bytes()[range.start - 1] != b'\n';
    let trail = range.end < doc.len() && doc.as_bytes()[range.end] != b'\n';
    let insert = format!(
        "{}{}{}",
        if lead { "\n" } else { "" },
        fuchico_markdown::table::default_table_markdown(),
        if trail { "\n" } else { "" }
    );
    let table_from = range.start + usize::from(lead);
    (CommandEdit { changes: vec![(range, insert)], selection: vec![(table_from + 1, table_from + 1)] }, table_from)
}

/// `toggleTaskLine` (Mod-Shift-Enter).
pub fn toggle_task_line(doc: &str, ranges: &[Range<usize>]) -> Option<CommandEdit> {
    let sel: Vec<(usize, usize)> = ranges.iter().map(|r| (r.start, r.end)).collect();
    let changes = task_helpers::toggle_task_line(doc, &sel)?;
    let mut changes: Vec<(Range<usize>, String)> = changes.into_iter().map(|c| (c.from..c.to, c.insert)).collect();
    changes.sort_by_key(|(r, _)| r.start);
    Some(CommandEdit { changes, selection: Vec::new() })
}

/// Which picker [`pick_token`] opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Due,
    Recurrence,
}

/// `pickToken` (Mod-Shift-. / Mod-Shift-,): put the cursor after an
/// existing `📅 value` / `🔁 rule`, or append `" 📅 "` / `" 🔁 "` at the end
/// of the line. The caller then opens the completion popup.
pub fn pick_token(doc: &str, head: usize, kind: TokenKind) -> CommandEdit {
    let (re, emoji) = match kind {
        TokenKind::Due => (&*DUE_RE, "📅"),
        TokenKind::Recurrence => (&*RECUR_RE, "🔁"),
    };
    let line = line_bounds(doc, head);
    let text = &doc[line.clone()];
    if let Some(c) = re.captures(text) {
        let v = c.get(1).unwrap();
        let at = line.start + v.end();
        return CommandEdit { changes: Vec::new(), selection: vec![(at, at)] };
    }
    let trimmed = text.trim_end().len();
    let insert = format!("{}{emoji} ", if trimmed > 0 { " " } else { "" });
    let at = line.start + trimmed + insert.len();
    CommandEdit { changes: vec![(line.start + trimmed..line.end, insert)], selection: vec![(at, at)] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, 27).unwrap().and_hms_opt(9, 5, 7).unwrap()
    }

    #[test]
    fn checkbox_toggles_first_box_per_line() {
        let doc = "- [ ] a [x]\n- [x] b\nplain";
        let e = toggle_checkbox(doc, &[2, 14]).unwrap();
        assert_eq!(e.apply(doc), "- [x] a [x]\n- [ ] b\nplain");
        assert!(toggle_checkbox(doc, &[doc.len()]).is_none());
    }

    #[test]
    fn dates() {
        assert_eq!(format_date(&now()), "2026-08-27");
        assert_eq!(format_date_time(&now()), "2026-08-27 09:05");
        let e = insert_at_cursor(&[1..1, 3..4], "X");
        assert_eq!(e.apply("abcd"), "aXbcX");
        assert_eq!(e.selection, vec![(2, 2), (5, 5)]);
    }

    #[test]
    fn region_empty_and_wrapping() {
        let e = insert_region("", &[0..0], ":::fold", ":::endfold");
        assert_eq!(e.apply(""), ":::fold Region\n:::endfold\n");
        assert_eq!(e.selection, vec![(8, 8)]);
        let doc = "a\nb\nc";
        let e = insert_region(doc, &[2..3], ":::fold", ":::endfold");
        let out = e.apply(doc);
        assert_eq!(out, "a\n:::fold Region\nb\n:::endfold\nc");
        assert_eq!(e.selection, vec![(2, 2 + 1 + 15 + 11)]);
    }

    #[test]
    fn table_insert_adds_breaks() {
        let (e, from) = insert_table("ab", 1..1);
        let out = e.apply("ab");
        assert!(out.starts_with("a\n| Column 1"), "{out}");
        assert!(out.ends_with("|\nb"));
        assert_eq!(from, 2);
        assert_eq!(e.selection, vec![(3, 3)]);
    }

    #[test]
    fn pick_tokens() {
        let doc = "- [ ] task  ";
        let e = pick_token(doc, 0, TokenKind::Due);
        assert_eq!(e.apply(doc), "- [ ] task 📅 ");
        assert_eq!(e.selection[0].0, "- [ ] task 📅 ".len());
        let doc = "- [ ] task 📅 2026-01-02 x";
        let e = pick_token(doc, 0, TokenKind::Due);
        assert!(e.changes.is_empty());
        assert_eq!(e.selection[0].0, doc.find(" x").unwrap());
        let e = pick_token("", 0, TokenKind::Recurrence);
        assert_eq!(e.apply(""), "🔁 ");
    }

    #[test]
    fn task_line() {
        let e = toggle_task_line("hello", &[0..0]).unwrap();
        assert_eq!(e.apply("hello"), "- [ ] hello");
    }
}
