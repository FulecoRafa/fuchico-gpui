//! The `@codemirror/commands` commands cm-helix binds (or that CodeMirror's
//! default keymap provides in insert mode), over logical lines.

use crate::change::{Change, ChangeSet};
use crate::editor::{Editor, Tr};
use crate::markup::parse_prefix;
use crate::selection::{Range, Selection};
use crate::text::{
    CharCategory, Line, Text, categorize, char_len, count_column, find_cluster_break,
    indent_string, leading_ws_len, step_char,
};

fn line_chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

impl Editor {
    /// CodeMirror `deleteBy`: `by` gives the target of an empty range.
    fn delete_by(&mut self, by: impl Fn(&Editor, Range) -> usize) {
        let tr = self.change_by_range(|ed, r| {
            let (mut from, mut to) = (r.from(), r.to());
            if from == to {
                let towards = by(ed, r);
                from = from.min(towards);
                to = to.max(towards);
            }
            if from == to {
                (Vec::new(), r)
            } else {
                (vec![Change::delete(from, to)], Range::cursor(from))
            }
        });
        if tr.changes.as_ref().is_none_or(|c| c.is_empty()) {
            return;
        }
        self.dispatch(tr.scrolled());
    }

    /// `deleteCharBackward` (by indent unit) / `deleteCharForward`.
    pub(crate) fn delete_char(&mut self, forward: bool) {
        let tab = self.config.tab_size;
        let unit = self.config.indent_unit_cols();
        self.delete_by(|ed, r| {
            let doc = &ed.doc;
            let mut pos = r.from();
            let line = doc.line_at(pos);
            let text = doc.line_text(&line);
            let chars = line_chars(&text);
            if !forward && pos > line.from && pos < line.from + 200 {
                let before = &chars[..pos - line.from];
                if before.iter().all(|c| *c == ' ' || *c == '\t') {
                    if before.last() == Some(&'\t') {
                        return pos - 1;
                    }
                    let b: String = before.iter().collect();
                    let col = count_column(&b, tab, usize::MAX);
                    let drop = if col.is_multiple_of(unit) {
                        unit
                    } else {
                        col % unit
                    };
                    let mut i = 0;
                    while i < drop && before.len() > i && before[before.len() - 1 - i] == ' ' {
                        pos -= 1;
                        i += 1;
                    }
                    return pos;
                }
            }
            let mut target = find_cluster_break(&text, pos - line.from, forward) + line.from;
            if target == pos && line.number != if forward { doc.lines() } else { 1 } {
                target = if forward { target + 1 } else { target - 1 };
            } else if !forward
                && chars[target - line.from..pos - line.from]
                    .iter()
                    .any(|c| ('\u{fe00}'..='\u{fe0f}').contains(c))
            {
                target = find_cluster_break(&text, target - line.from, false) + line.from;
            }
            target
        });
    }

    /// `deleteGroupBackward` / `deleteGroupForward`.
    pub(crate) fn delete_group(&mut self, forward: bool) {
        self.delete_by(|ed, r| {
            let doc = &ed.doc;
            let mut pos = r.head;
            let line = doc.line_at(pos);
            let text = doc.line_text(&line);
            let chars = line_chars(&text);
            let mut cat: Option<CharCategory> = None;
            loop {
                if pos == if forward { line.to } else { line.from } {
                    if pos == r.head && line.number != if forward { doc.lines() } else { 1 } {
                        pos = if forward { pos + 1 } else { pos - 1 };
                    }
                    break;
                }
                let next = find_cluster_break(&text, pos - line.from, forward) + line.from;
                let (a, b) = (pos.min(next) - line.from, pos.max(next) - line.from);
                let next_char: String = chars[a..b].iter().collect();
                let next_cat = categorize(&next_char);
                if cat.is_some_and(|c| c != next_cat) {
                    break;
                }
                if next_char != " " || pos != r.head {
                    cat = Some(next_cat);
                }
                pos = next;
            }
            pos
        });
    }

    /// `deleteToLineStart` / `deleteToLineEnd` (logical lines).
    pub(crate) fn delete_to_line_boundary(&mut self, forward: bool) {
        self.delete_by(|ed, r| {
            let line = ed.doc.line_at(r.head);
            if forward {
                if r.head < line.to {
                    line.to
                } else {
                    ed.doc.len().min(r.head + 1)
                }
            } else if r.head > line.from {
                line.from
            } else {
                r.head.saturating_sub(1)
            }
        });
    }

    /// CodeMirror `moveSel`: no-op (and no dispatch) when unchanged.
    fn move_sel(&mut self, f: impl FnMut(Range) -> Range) {
        let next = Selection::create(
            self.sel.ranges().iter().copied().map(f).collect(),
            self.sel.main_index(),
        );
        if next.same(&self.sel) {
            return;
        }
        self.dispatch(Tr::sel(next).scrolled());
    }

    /// `cursorCharLeft` / `cursorCharRight` (LTR).
    pub(crate) fn cursor_char(&mut self, forward: bool) {
        let doc = self.doc.clone();
        self.move_sel(|r| {
            if !r.is_empty() {
                return Range::cursor(if forward { r.to() } else { r.from() });
            }
            match step_char(&doc, r.head, forward) {
                Some((p, _)) => Range::cursor(p),
                None => r,
            }
        });
    }

    /// `cursorLineUp` / `cursorLineDown`.
    pub(crate) fn cursor_line(&mut self, forward: bool) {
        let tab = self.config.tab_size;
        let next = {
            let doc = &self.doc;
            let host = &mut self.host;
            let ranges = self.sel.ranges().iter().map(|&r| {
                if !r.is_empty() {
                    return Range::cursor(if forward { r.to() } else { r.from() });
                }
                let (p, g) = host.move_vertically(doc, r.head, r.goal, forward, 1, tab);
                if p != r.head {
                    Range::with_goal(p, p, g)
                } else {
                    let line = doc.line_at(r.head);
                    Range::cursor(if forward { line.to } else { line.from })
                }
            });
            Selection::create(ranges.collect(), self.sel.main_index())
        };
        if next.same(&self.sel) {
            return;
        }
        self.dispatch(Tr::sel(next).scrolled());
    }

    /// `cursorLineBoundaryBackward` (smart home) / `cursorLineBoundaryForward`.
    pub(crate) fn cursor_line_boundary(&mut self, forward: bool) {
        let doc = self.doc.clone();
        self.move_sel(|r| {
            let line = doc.line_at(r.head);
            if forward {
                return Range::cursor(line.to);
            }
            let text = doc.line_text(&line);
            let head100: String = text.chars().take(100).collect();
            let space = leading_ws_len(&head100);
            if !line.is_empty() && space > 0 && r.head != line.from + space {
                Range::cursor(line.from + space)
            } else {
                Range::cursor(line.from)
            }
        });
    }

    /// `insertNewlineAndIndent` (indentation = the line's own, as Markdown
    /// has no indent service), plus list/quote continuation when
    /// [`crate::Config::continue_markup`] is on.
    pub(crate) fn insert_newline_and_indent(&mut self) {
        let tab = self.config.tab_size;
        let unit_tab = self.config.indent_is_tab();
        let markup = self.config.continue_markup;
        let tr = self.change_by_range(|ed, r| {
            let doc = &ed.doc;
            let (mut from, mut to) = (r.from(), r.to());
            let line = doc.line_at(from);
            let text = doc.line_text(&line);
            let chars = line_chars(&text);

            if markup && from == to {
                let prefix = parse_prefix(&text);
                if prefix.has_markup() && from - line.from >= prefix.len {
                    if prefix.empty {
                        // Enter on an empty item: drop the marker instead.
                        let cleared = prefix.cleared();
                        let len = char_len(&cleared);
                        return (
                            vec![Change::new(line.from, line.to, cleared)],
                            Range::cursor(line.from + len),
                        );
                    }
                    while to < line.to && chars[to - line.from].is_whitespace() {
                        to += 1;
                    }
                    let cont = prefix.continuation(true);
                    let len = char_len(&cont);
                    return (
                        vec![Change::new(from, to, format!("\n{cont}"))],
                        Range::cursor(from + 1 + len),
                    );
                }
            }

            let explode = from == to && {
                let s = doc.slice(from.saturating_sub(1), (from + 1).min(doc.len()));
                s == "()" || s == "[]" || s == "{}"
            };
            let ws: String = chars.iter().take_while(|c| c.is_whitespace()).collect();
            let indent = count_column(&ws, tab, usize::MAX);
            while to < line.to && chars[to - line.from].is_whitespace() {
                to += 1;
            }
            if explode {
                to = from;
            } else if from > line.from && from < line.from + 100 {
                // CodeMirror slices `line.text` with the *document* offset
                // `from` here (a quirk we keep).
                let n = from.min(chars.len());
                if chars[..n].iter().all(|c| c.is_whitespace()) {
                    from = line.from;
                }
            }
            let ind = indent_string(indent, tab, unit_tab);
            let mut insert = format!("\n{ind}");
            if explode {
                insert.push('\n');
                insert.push_str(&ind);
            }
            let cursor = from + 1 + char_len(&ind);
            (vec![Change::new(from, to, insert)], Range::cursor(cursor))
        });
        self.dispatch(tr.scrolled());
    }

    /// CodeMirror `changeBySelectedLine`.
    fn change_by_selected_line(&mut self, mut f: impl FnMut(&Editor, &Line, &mut Vec<Change>)) {
        let mut at_line = 0usize;
        let tr = self.change_by_range(|ed, r| {
            let doc = &ed.doc;
            let mut changes = Vec::new();
            let mut pos = r.from();
            while pos <= r.to() {
                let line = doc.line_at(pos);
                if line.number > at_line && (r.is_empty() || r.to() > line.from) {
                    f(ed, &line, &mut changes);
                    at_line = line.number;
                }
                pos = line.to + 1;
            }
            let set = ChangeSet::new(changes.clone());
            let range = Range::new(set.map_pos(r.anchor, 1), set.map_pos(r.head, 1));
            (changes, range)
        });
        self.dispatch(tr);
    }

    /// `indentMore`.
    pub(crate) fn indent_more(&mut self) {
        let unit = self.config.indent_unit.clone();
        self.change_by_selected_line(|_, line, changes| {
            changes.push(Change::insert(line.from, unit.clone()))
        });
    }

    /// `indentLess`.
    pub(crate) fn indent_less(&mut self) {
        let tab = self.config.tab_size;
        let unit = self.config.indent_unit_cols();
        let unit_tab = self.config.indent_is_tab();
        self.change_by_selected_line(|ed, line, changes| {
            let text = ed.doc.line_text(line);
            let space: Vec<char> = text.chars().take_while(|c| c.is_whitespace()).collect();
            if space.is_empty() {
                return;
            }
            let s: String = space.iter().collect();
            let col = count_column(&s, tab, usize::MAX);
            let insert: Vec<char> = indent_string(col.saturating_sub(unit), tab, unit_tab)
                .chars()
                .collect();
            let mut keep = 0;
            while keep < space.len() && keep < insert.len() && space[keep] == insert[keep] {
                keep += 1;
            }
            changes.push(Change::new(
                line.from + keep,
                line.from + space.len(),
                insert[keep..].iter().collect::<String>(),
            ));
        });
    }

    /// `toggleComment`: line comments when the language has them, else block
    /// comments around each selected line range (`toggleBlockCommentByLine`).
    pub(crate) fn toggle_comment(&mut self) {
        let tokens = self.config.comment_tokens.clone();
        if let Some(token) = tokens.line {
            self.toggle_line_comment(&token);
        } else if let Some((open, close)) = tokens.block {
            self.toggle_block_comment_by_line(&open, &close);
        }
    }

    fn toggle_line_comment(&mut self, token: &str) {
        struct L {
            line: Line,
            comment: Option<usize>,
            indent: usize,
            empty: bool,
            single: bool,
        }
        let doc = &self.doc;
        let tlen = char_len(token);
        let mut lines: Vec<L> = Vec::new();
        let mut prev_line: Option<usize> = None;
        for r in self.sel.ranges() {
            let (from, to) = (r.from(), r.to());
            let start = lines.len();
            let mut min_indent = usize::MAX;
            let mut pos = from;
            while pos <= to {
                let line = doc.line_at(pos);
                if prev_line.is_none_or(|p| line.from > p) && (from == to || to > line.from) {
                    prev_line = Some(line.from);
                    let text = doc.line_text(&line);
                    let chars = line_chars(&text);
                    let indent = leading_ws_len(&text);
                    let empty = indent == chars.len();
                    let comment = if chars.len() >= indent + tlen
                        && chars[indent..indent + tlen].iter().collect::<String>() == token
                    {
                        Some(indent)
                    } else {
                        None
                    };
                    if indent < chars.len() && indent < min_indent {
                        min_indent = indent;
                    }
                    lines.push(L {
                        line,
                        comment,
                        indent,
                        empty,
                        single: false,
                    });
                }
                pos = line.to + 1;
            }
            if min_indent < usize::MAX {
                for l in &mut lines[start..] {
                    if l.indent < l.line.len() {
                        l.indent = min_indent;
                    }
                }
            }
            if lines.len() == start + 1 {
                lines[start].single = true;
            }
        }
        if lines
            .iter()
            .any(|l| l.comment.is_none() && (!l.empty || l.single))
        {
            let changes: Vec<Change> = lines
                .iter()
                .filter(|l| l.single || !l.empty)
                .map(|l| Change::insert(l.line.from + l.indent, format!("{token} ")))
                .collect();
            let set = ChangeSet::new(changes);
            let sel = self.sel.map(&set, 1);
            self.dispatch(Tr::changes(set).with_sel(sel));
        } else if lines.iter().any(|l| l.comment.is_some()) {
            let mut changes = Vec::new();
            for l in &lines {
                if let Some(c) = l.comment {
                    let from = l.line.from + c;
                    let mut to = from + tlen;
                    if doc.char_at(to) == Some(' ') && to < l.line.to {
                        to += 1;
                    }
                    changes.push(Change::delete(from, to));
                }
            }
            self.dispatch(Tr::changes(ChangeSet::new(changes)));
        }
    }

    fn toggle_block_comment_by_line(&mut self, open: &str, close: &str) {
        let doc = &self.doc;
        // selectedLineRanges
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for r in self.sel.ranges() {
            let from_line = doc.line_at(r.from());
            let mut to_line = if r.to() <= from_line.to {
                from_line
            } else {
                doc.line_at(r.to())
            };
            if to_line.from > from_line.from && to_line.from == r.to() {
                to_line = if r.to() == from_line.to + 1 {
                    from_line
                } else {
                    doc.line_at(r.to() - 1)
                };
            }
            match ranges.last_mut() {
                Some(last) if last.1 > from_line.from => last.1 = to_line.to,
                _ => ranges.push((
                    from_line.from + leading_ws_len(&doc.line_text(&from_line)),
                    to_line.to,
                )),
            }
        }
        let comments: Vec<Option<BlockComment>> = ranges
            .iter()
            .map(|&(f, t)| find_block_comment(doc, open, close, f, t))
            .collect();
        if !comments.iter().all(|c| c.is_some()) {
            let mut changes = Vec::new();
            for (i, &(f, t)) in ranges.iter().enumerate() {
                if comments[i].is_none() {
                    changes.push(Change::insert(f, format!("{open} ")));
                    changes.push(Change::insert(t, format!(" {close}")));
                }
            }
            self.dispatch(Tr::changes(ChangeSet::new(changes)));
        } else {
            let (ol, cl) = (char_len(open), char_len(close));
            let mut changes = Vec::new();
            for c in comments.into_iter().flatten() {
                changes.push(Change::delete(c.open_pos - ol, c.open_pos + c.open_margin));
                changes.push(Change::delete(
                    c.close_pos - c.close_margin,
                    c.close_pos + cl,
                ));
            }
            self.dispatch(Tr::changes(ChangeSet::new(changes)));
        }
    }
}

struct BlockComment {
    open_pos: usize,
    open_margin: usize,
    close_pos: usize,
    close_margin: usize,
}

const SEARCH_MARGIN: usize = 50;

/// CodeMirror `findBlockComment`.
fn find_block_comment(
    doc: &Text,
    open: &str,
    close: &str,
    from: usize,
    to: usize,
) -> Option<BlockComment> {
    let ol = char_len(open);
    let cl = char_len(close);
    let before: Vec<char> = doc
        .slice(from.saturating_sub(SEARCH_MARGIN), from)
        .chars()
        .collect();
    let after: Vec<char> = doc
        .slice(to, (to + SEARCH_MARGIN).min(doc.len()))
        .chars()
        .collect();
    let space_before = before
        .iter()
        .rev()
        .take_while(|c| c.is_whitespace())
        .count();
    let space_after = after.iter().take_while(|c| c.is_whitespace()).count();
    let before_off = before.len() - space_before;
    let s = |v: &[char], a: usize, b: usize| -> String {
        v[a.min(v.len())..b.min(v.len())].iter().collect()
    };
    if before_off >= ol
        && s(&before, before_off - ol, before_off) == open
        && s(&after, space_after, space_after + cl) == close
    {
        return Some(BlockComment {
            open_pos: from - space_before,
            open_margin: usize::from(space_before > 0),
            close_pos: to + space_after,
            close_margin: usize::from(space_after > 0),
        });
    }
    let (start, end): (Vec<char>, Vec<char>) = if to - from <= 2 * SEARCH_MARGIN {
        let t: Vec<char> = doc.slice(from, to).chars().collect();
        (t.clone(), t)
    } else {
        (
            doc.slice(from, from + SEARCH_MARGIN).chars().collect(),
            doc.slice(to - SEARCH_MARGIN, to).chars().collect(),
        )
    };
    let start_space = start.iter().take_while(|c| c.is_whitespace()).count();
    let end_space = end.iter().rev().take_while(|c| c.is_whitespace()).count();
    if end.len() < end_space + cl {
        return None;
    }
    let end_off = end.len() - end_space - cl;
    if s(&start, start_space, start_space + ol) == open && s(&end, end_off, end_off + cl) == close {
        let open_margin = usize::from(
            start
                .get(start_space + ol)
                .is_some_and(|c| c.is_whitespace()),
        );
        let close_margin = usize::from(end_off > 0 && end[end_off - 1].is_whitespace());
        return Some(BlockComment {
            open_pos: from + start_space + ol,
            open_margin,
            close_pos: to - end_space - cl,
            close_margin,
        });
    }
    None
}
