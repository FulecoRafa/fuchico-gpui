//! The CodeMirror 5 API cm-vim's CodeMirror 6 adapter (`cm_adapter.ts`)
//! provides, over [`Text`] / [`Selection`]: positions, selections, ranges,
//! bookmarks, operations and change/cursor-activity events, history
//! commands, and the handful of `@codemirror/commands` commands reachable
//! from Vim (`newlineAndIndent`, `indentMore`, `deleteCharBackward` …).

use fuchico_helix::text::{
    CharCategory, categorize, char_len, count_column, find_cluster_break, indent_string,
    leading_ws_len, step_char,
};
use fuchico_helix::{Change, ChangeSet, Effect, Range, Selection, Text};

use crate::editor::{CurOp, Editor, Marker};
use crate::pos::{Pos, chars};

/// Transaction user events (history grouping).
pub(crate) const COMPOSE_START: &str = "input.type.compose.start";
pub(crate) const COMPOSE: &str = "input.type.compose";

impl Editor {
    // ---- positions -----------------------------------------------------------

    pub(crate) fn line_count(&self) -> i64 {
        self.doc.lines() as i64
    }
    pub(crate) fn first_line(&self) -> i64 {
        0
    }
    pub(crate) fn last_line(&self) -> i64 {
        self.doc.lines() as i64 - 1
    }

    /// `cm.getLine(n)` (`""` out of range… CM6 adapter returns undefined;
    /// callers treat it as empty).
    pub(crate) fn get_line(&self, n: i64) -> String {
        if n < 0 || n > self.last_line() {
            return String::new();
        }
        let l = self.doc.line(n as usize + 1);
        self.doc.line_text(&l)
    }

    pub(crate) fn line_chars(&self, n: i64) -> Vec<char> {
        chars(&self.get_line(n))
    }

    pub(crate) fn line_len(&self, n: i64) -> i64 {
        if n < 0 || n > self.last_line() {
            return 0;
        }
        self.doc.line(n as usize + 1).len() as i64
    }

    /// `cm.indexFromPos` (clipped).
    pub(crate) fn index_from_pos(&self, p: Pos) -> usize {
        let (line, ch) = if p.line < 0 {
            (0, 0)
        } else if p.line > self.last_line() {
            (self.last_line(), i64::MAX)
        } else {
            (p.line, p.ch)
        };
        let l = self.doc.line(line as usize + 1);
        let ch = ch.clamp(0, l.len() as i64) as usize;
        l.from + ch
    }

    pub(crate) fn pos_from_index(&self, i: usize) -> Pos {
        let i = i.min(self.doc.len());
        let l = self.doc.line_at(i);
        Pos::new(l.number as i64 - 1, (i - l.from) as i64)
    }

    /// `cm.clipPos`.
    pub(crate) fn clip_pos(&self, p: Pos) -> Pos {
        self.pos_from_index(self.index_from_pos(p))
    }

    // ---- selections ------------------------------------------------------------

    pub(crate) fn get_cursor(&self, which: &str) -> Pos {
        let r = self.sel.main();
        let i = match which {
            "anchor" => r.anchor,
            "start" | "from" => r.from(),
            "end" | "to" => r.to(),
            _ => r.head,
        };
        self.pos_from_index(i)
    }

    pub(crate) fn head(&self) -> Pos {
        self.get_cursor("head")
    }

    /// `cm.listSelections()` as (anchor, head).
    pub(crate) fn list_selections(&self) -> Vec<(Pos, Pos)> {
        self.sel
            .ranges()
            .iter()
            .map(|r| (self.pos_from_index(r.anchor), self.pos_from_index(r.head)))
            .collect()
    }

    pub(crate) fn something_selected(&self) -> bool {
        self.sel.ranges().iter().any(|r| !r.is_empty())
    }

    /// `cm.setCursor(pos)`.
    pub(crate) fn set_cursor(&mut self, p: Pos) {
        let i = self.index_from_pos(p);
        self.dispatch_tr(None, Some(Selection::single(i, i)), Some("select"));
        if self.cur_op.as_ref().is_some_and(|o| !o.is_vim_op) {
            self.on_before_end_operation();
        }
    }

    /// `cm.setSelection(anchor, head)`.
    pub(crate) fn set_selection_pos(&mut self, anchor: Pos, head: Pos) {
        let a = self.index_from_pos(anchor);
        let h = self.index_from_pos(head);
        self.dispatch_tr(None, Some(Selection::single(a, h)), Some("select"));
    }

    /// `cm.setSelections(ranges, primary)`.
    pub(crate) fn set_selections(&mut self, ranges: &[(Pos, Pos)], primary: usize) {
        if ranges.is_empty() {
            return;
        }
        let rs: Vec<Range> = ranges
            .iter()
            .map(|&(a, h)| Range::new(self.index_from_pos(a), self.index_from_pos(h)))
            .collect();
        let primary = primary.min(rs.len() - 1);
        let sel = selection_unsorted(rs, primary);
        self.dispatch_tr(None, Some(sel), Some("select"));
    }

    /// `cm.getRange(from, to)`.
    pub(crate) fn get_range(&self, a: Pos, b: Pos) -> String {
        let (a, b) = (self.index_from_pos(a), self.index_from_pos(b));
        self.doc.slice(a.min(b), a.max(b))
    }

    /// `cm.getSelection()`: every range joined with `"\n"`.
    pub(crate) fn get_selection(&self) -> String {
        self.get_selections().join("\n")
    }

    pub(crate) fn get_selections(&self) -> Vec<String> {
        self.sel
            .ranges()
            .iter()
            .map(|r| self.doc.slice(r.from(), r.to()))
            .collect()
    }

    /// `cm.replaceRange(text, from, to)`.
    pub(crate) fn replace_range(&mut self, text: &str, from: Pos, to: Option<Pos>) {
        let a = self.index_from_pos(from);
        let b = to.map(|t| self.index_from_pos(t)).unwrap_or(a);
        let (a, b) = (a.min(b), a.max(b));
        self.dispatch_change(ChangeSet::new(vec![Change::new(a, b, text)]), None);
    }

    /// `cm.replaceSelection(text)` (cursor after the text).
    pub(crate) fn replace_selection(&mut self, text: &str) {
        let n = self.sel.len();
        self.replace_selections(&vec![text.to_string(); n]);
    }

    /// `cm.replaceSelections(texts)`.
    pub(crate) fn replace_selections(&mut self, texts: &[String]) {
        let ranges: Vec<Range> = self.sel.ranges().to_vec();
        let mut changes = Vec::new();
        for (i, r) in ranges.iter().enumerate() {
            let t = texts.get(i).cloned().unwrap_or_default();
            changes.push(Change::new(r.from(), r.to(), t));
        }
        let cs = ChangeSet::new(changes.clone());
        // every range becomes a cursor after its inserted text
        let new_ranges: Vec<Range> = ranges
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let t = texts.get(i).map(|s| char_len(s)).unwrap_or(0);
                let start = cs.map_pos(r.from(), -1);
                Range::cursor(start + t)
            })
            .collect();
        let sel = selection_unsorted(new_ranges, self.sel.main_index());
        self.dispatch_change(cs, Some(sel));
    }

    /// `dispatchChange`: user event `input.type.compose(.start)`.
    pub(crate) fn dispatch_change(&mut self, cs: ChangeSet, sel: Option<Selection>) {
        let ev = match &self.cur_op {
            Some(op) if op.changes.is_empty() && op.change_start.is_none() => COMPOSE_START,
            _ => COMPOSE,
        };
        self.dispatch_tr(Some(cs), sel, Some(ev));
    }

    // ---- bookmarks -------------------------------------------------------------

    /// `cm.setBookmark(pos, {insertLeft})`.
    pub(crate) fn set_bookmark(&mut self, p: Pos, insert_left: bool) -> usize {
        let id = self.next_marker;
        self.next_marker += 1;
        let offset = self.index_from_pos(p);
        self.markers.insert(
            id,
            Marker {
                offset: Some(offset),
                assoc: if insert_left { 1 } else { -1 },
                line_handle: false,
            },
        );
        id
    }

    /// A line handle (`cm.getLineHandle`), tracked with `TrackAfter`.
    pub(crate) fn line_handle(&mut self, line: i64) -> usize {
        let id = self.next_marker;
        self.next_marker += 1;
        let offset = self.index_from_pos(Pos::new(line, 0));
        self.markers.insert(
            id,
            Marker {
                offset: Some(offset),
                assoc: -1,
                line_handle: true,
            },
        );
        id
    }

    /// `marker.find()`.
    pub(crate) fn find_marker(&self, id: usize) -> Option<Pos> {
        let m = self.markers.get(&id)?;
        m.offset.map(|o| self.pos_from_index(o))
    }

    /// `marker.clear()`.
    pub(crate) fn clear_marker(&mut self, id: usize) {
        self.markers.remove(&id);
    }

    fn map_markers(&mut self, cs: &ChangeSet) {
        for m in self.markers.values_mut() {
            let Some(pos) = m.offset else { continue };
            let deleted = cs.changes().iter().any(|c| {
                if m.line_handle {
                    c.from <= pos && pos < c.to
                } else {
                    c.from < pos && pos < c.to
                }
            });
            m.offset = if deleted {
                None
            } else {
                Some(cs.map_pos(pos, m.assoc))
            };
        }
    }

    // ---- transactions ---------------------------------------------------------

    pub(crate) fn now_ms(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0 + self.clock_offset
    }

    /// Dispatch a transaction: the CodeMirror 6 state update plus the
    /// adapter's `onChange` / `onSelectionChange` bookkeeping.
    pub(crate) fn dispatch_tr(
        &mut self,
        changes: Option<ChangeSet>,
        sel: Option<Selection>,
        user_event: Option<&str>,
    ) {
        let changes = changes.filter(|c| !c.is_empty());
        if changes.is_none() && sel.is_none() {
            return;
        }
        let old_doc = self.doc.clone();
        let start_sel = self.sel.clone();
        let explicit_sel = sel.is_some();
        if let Some(cs) = &changes {
            self.edits.push(Effect::DocChanged(cs.describe(&self.doc)));
            cs.apply(&mut self.doc);
            self.map_markers(cs);
        }
        let new_sel = match sel {
            Some(s) => s.clamp(self.doc.len()),
            None => match &changes {
                Some(cs) => self.sel.map(cs, -1),
                None => self.sel.clone(),
            },
        };
        self.sel = new_sel;
        let time = self.now_ms();
        self.history.add(
            &old_doc,
            changes.as_ref(),
            &start_sel,
            explicit_sel,
            user_event,
            time,
        );
        if let Some(cs) = &changes {
            // adapter onChange
            let listener = self.change_listener;
            let op = self.cur_op.get_or_insert_with(CurOp::default);
            if op.change_handlers.is_none() {
                op.change_handlers = Some(listener);
            }
            let mut delta: isize = 0;
            for c in cs.changes() {
                let from_b = (c.from as isize + delta) as usize;
                op.change_start = Some(op.change_start.map_or(from_b, |s| s.min(from_b)));
                let ins = char_len(&c.insert);
                self.last_change_end_offset = from_b + ins;
                op.changes.push(c.insert.clone());
                delta += ins as isize - (c.to - c.from) as isize;
            }
        }
        if explicit_sel {
            let op = self.cur_op.get_or_insert_with(CurOp::default);
            op.cursor_activity = true;
        }
        // ViewPlugin.update: end non-vim operations right away
        if self.cur_op.as_ref().is_some_and(|o| !o.is_vim_op && o.depth <= 0) {
            self.on_before_end_operation();
        }
    }

    /// `cm.operation(fn, isVimOp)`.
    pub(crate) fn operation<R>(&mut self, is_vim_op: bool, f: impl FnOnce(&mut Self) -> R) -> R {
        {
            let op = self.cur_op.get_or_insert_with(CurOp::default);
            op.depth += 1;
            if is_vim_op {
                op.is_vim_op = true;
            }
        }
        let r = f(self);
        let end = match self.cur_op.as_mut() {
            Some(op) => {
                op.depth -= 1;
                op.depth <= 0
            }
            None => false,
        };
        if end {
            self.on_before_end_operation();
        }
        r
    }

    pub(crate) fn is_vim_op(&self) -> bool {
        self.cur_op.as_ref().is_some_and(|o| o.is_vim_op)
    }

    /// `onBeforeEndOperation`: fire change and cursor activity handlers.
    pub(crate) fn on_before_end_operation(&mut self) {
        let Some(op) = self.cur_op.take() else { return };
        // handlers run "inside" the finished op: keep isVimOp visible
        let marker = CurOp {
            is_vim_op: op.is_vim_op,
            depth: 1,
            ..CurOp::default()
        };
        self.cur_op = Some(marker);
        if op.change_handlers == Some(true) && !op.changes.is_empty() {
            self.vim_on_change(&op.changes);
        }
        if op.cursor_activity {
            self.on_cursor_activity();
        }
        if op.is_vim_op {
            self.want_scroll_into_view = true;
        }
        // anything dispatched by the handlers belongs to a fresh op
        let leftover = self.cur_op.take();
        if let Some(l) = leftover
            && (!l.changes.is_empty() || l.cursor_activity)
            && !l.is_vim_op
        {
            self.cur_op = Some(CurOp { depth: 0, ..l });
            self.on_before_end_operation();
        }
    }

    // ---- history ---------------------------------------------------------------

    /// `runHistoryCommand(cm, revert)`.
    pub(crate) fn run_history_command(&mut self, redo: bool) {
        if let Some(op) = self.cur_op.as_mut() {
            op.change_start = None;
            op.changes.clear();
        }
        let current = self.sel.clone();
        let Some(p) = self.history.pop(redo, &current) else {
            return;
        };
        let mut applied: Vec<(ChangeSet, Text)> = Vec::new();
        let mut change_start: Option<usize> = None;
        for cs in &p.steps {
            let old = self.doc.clone();
            self.edits.push(Effect::DocChanged(cs.describe(&self.doc)));
            let mut delta: isize = 0;
            for c in cs.changes() {
                let from_b = (c.from as isize + delta) as usize;
                change_start = Some(change_start.map_or(from_b, |s: usize| s.min(from_b)));
                delta += char_len(&c.insert) as isize - (c.to - c.from) as isize;
            }
            cs.apply(&mut self.doc);
            self.map_markers(cs);
            self.sel = self.sel.map(cs, -1);
            applied.push((cs.clone(), old));
        }
        if let Some(s) = p.selection {
            self.sel = s.clamp(self.doc.len());
        }
        self.history.push_other(redo, &applied, current);
        if let Some(op) = self.cur_op.as_mut() {
            op.cursor_activity = true;
        }
        if let Some(cs) = change_start {
            let p = self.pos_from_index(cs);
            self.set_cursor(p);
        }
    }

    // ---- CodeMirror commands -----------------------------------------------------

    /// `cm.moveH(increment, 'char')`.
    pub(crate) fn move_h(&mut self, inc: i64) {
        let c = self.head();
        self.set_cursor(Pos::new(c.line, c.ch + inc));
    }

    /// `overWriteSelection(key)` (replace mode typing).
    pub(crate) fn overwrite_selection(&mut self, text: &str) {
        let ranges: Vec<Range> = self
            .sel
            .ranges()
            .iter()
            .map(|r| {
                if r.is_empty() && self.doc.char_at(r.head).is_some_and(|c| c != '\n') {
                    Range::new(r.head, r.head + 1)
                } else {
                    *r
                }
            })
            .collect();
        self.sel = selection_unsorted(ranges, self.sel.main_index());
        self.replace_selection(text);
    }

    /// CodeMirror `changeByRange`.
    pub(crate) fn change_by_range(
        &self,
        mut f: impl FnMut(&Self, Range) -> (Vec<Change>, Range),
    ) -> (ChangeSet, Selection) {
        let results: Vec<(Vec<Change>, Range)> =
            self.sel.ranges().iter().map(|r| f(self, *r)).collect();
        let all: Vec<Change> = results.iter().flat_map(|(c, _)| c.clone()).collect();
        let cs = ChangeSet::new(all);
        let mut ranges = Vec::new();
        for (i, (own, range)) in results.iter().enumerate() {
            let own_set = ChangeSet::new(own.clone());
            let mut others = Vec::new();
            for (j, (theirs, _)) in results.iter().enumerate() {
                if i == j {
                    continue;
                }
                let assoc = if j < i { -1 } else { 1 };
                for c in theirs {
                    let from = own_set.map_pos(c.from, assoc);
                    let to = own_set.map_pos(c.to, assoc).max(from);
                    others.push(Change::new(from, to, c.insert.clone()));
                }
            }
            ranges.push(if others.is_empty() {
                *range
            } else {
                range.map(&ChangeSet::new(others), -1)
            });
        }
        (cs, selection_unsorted(ranges, self.sel.main_index()))
    }

    fn delete_by(&mut self, ev: &str, by: impl Fn(&Editor, Range) -> usize) {
        let (cs, sel) = self.change_by_range(|ed, r| {
            let (mut from, mut to) = (r.from(), r.to());
            if from == to {
                let t = by(ed, r);
                from = from.min(t);
                to = to.max(t);
            }
            if from == to {
                (Vec::new(), r)
            } else {
                (vec![Change::delete(from, to)], Range::cursor(from))
            }
        });
        if cs.is_empty() {
            return;
        }
        self.dispatch_tr(Some(cs), Some(sel), Some(ev));
    }

    /// `deleteCharBackward` / `deleteCharForward`.
    pub(crate) fn cmd_delete_char(&mut self, forward: bool) {
        let tab = self.config.tab_size;
        let unit = self.config.indent_unit_cols();
        let ev = if forward { "delete.forward" } else { "delete.backward" };
        self.delete_by(ev, |ed, r| {
            let doc = &ed.doc;
            let mut pos = r.from();
            let line = doc.line_at(pos);
            let text = doc.line_text(&line);
            let cs: Vec<char> = text.chars().collect();
            if !forward && pos > line.from && pos < line.from + 200 {
                let before = &cs[..pos - line.from];
                if before.iter().all(|c| *c == ' ' || *c == '\t') {
                    if before.last() == Some(&'\t') {
                        return pos - 1;
                    }
                    let b: String = before.iter().collect();
                    let col = count_column(&b, tab, usize::MAX);
                    let drop = if col.is_multiple_of(unit) { unit } else { col % unit };
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
            }
            target
        });
    }

    /// `deleteGroupBackward` / `deleteGroupForward`.
    pub(crate) fn cmd_delete_group(&mut self, forward: bool) {
        let ev = if forward { "delete.forward" } else { "delete.backward" };
        self.delete_by(ev, |ed, r| {
            let doc = &ed.doc;
            let mut pos = r.head;
            let line = doc.line_at(pos);
            let text = doc.line_text(&line);
            let cs: Vec<char> = text.chars().collect();
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
                let s: String = cs[a..b].iter().collect();
                let nc = categorize(&s);
                if cat.is_some_and(|c| c != nc) {
                    break;
                }
                if s != " " || pos != r.head {
                    cat = Some(nc);
                }
                pos = next;
            }
            pos
        });
    }

    /// `deleteLineBoundaryBackward/Forward` (Cmd-Backspace / Cmd-Delete).
    pub(crate) fn cmd_delete_to_line_boundary(&mut self, forward: bool) {
        let ev = if forward { "delete.forward" } else { "delete.backward" };
        self.delete_by(ev, |ed, r| {
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

    fn move_sel(&mut self, f: impl FnMut(Range) -> Range) {
        let next = selection_unsorted(
            self.sel.ranges().iter().copied().map(f).collect(),
            self.sel.main_index(),
        );
        if next.same(&self.sel) {
            return;
        }
        self.dispatch_tr(None, Some(next), Some("select"));
    }

    /// `cursorCharLeft` / `cursorCharRight`.
    pub(crate) fn cmd_cursor_char(&mut self, forward: bool) {
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

    /// `cursorGroupLeft` / `cursorGroupRight`.
    pub(crate) fn cmd_cursor_group(&mut self, forward: bool) {
        let doc = self.doc.clone();
        self.move_sel(|r| {
            if !r.is_empty() {
                return Range::cursor(if forward { r.to() } else { r.from() });
            }
            Range::cursor(fuchico_helix::text::move_by_group(&doc, r.head, forward))
        });
    }

    /// `cursorLineUp` / `cursorLineDown`.
    pub(crate) fn cmd_cursor_line(&mut self, forward: bool) {
        let tab = self.config.tab_size;
        let ranges: Vec<Range> = self.sel.ranges().to_vec();
        let mut out = Vec::new();
        for r in ranges {
            if !r.is_empty() {
                out.push(Range::cursor(if forward { r.to() } else { r.from() }));
                continue;
            }
            let (p, g) = self
                .host
                .move_vertically(&self.doc, r.head, r.goal, forward, 1, tab);
            if p != r.head {
                out.push(Range::with_goal(p, p, g));
            } else {
                let line = self.doc.line_at(r.head);
                out.push(Range::cursor(if forward { line.to } else { line.from }));
            }
        }
        let next = selection_unsorted(out, self.sel.main_index());
        if !next.same(&self.sel) {
            self.dispatch_tr(None, Some(next), Some("select"));
        }
    }

    /// `cursorLineBoundaryBackward` (smart home) / `…Forward`.
    pub(crate) fn cmd_cursor_line_boundary(&mut self, forward: bool) {
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

    /// `insertNewlineAndIndent` (Markdown: keeps the line's indentation).
    pub(crate) fn cmd_newline_and_indent(&mut self, markup: bool) {
        let tab = self.config.tab_size;
        let unit_tab = self.config.indent_is_tab();
        let markup = markup && self.config.continue_markup;
        let (cs, sel) = self.change_by_range(|ed, r| {
            let doc = &ed.doc;
            let (mut from, mut to) = (r.from(), r.to());
            let line = doc.line_at(from);
            let text = doc.line_text(&line);
            let cs: Vec<char> = text.chars().collect();
            if markup && from == to {
                let prefix = fuchico_helix::markup::parse_prefix(&text);
                if prefix.has_markup() && from - line.from >= prefix.len {
                    if prefix.empty {
                        let cleared = prefix.cleared();
                        let len = char_len(&cleared);
                        return (
                            vec![Change::new(line.from, line.to, cleared)],
                            Range::cursor(line.from + len),
                        );
                    }
                    while to < line.to && cs[to - line.from].is_whitespace() {
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
            let ws: String = cs.iter().take_while(|c| c.is_whitespace()).collect();
            let indent = count_column(&ws, tab, usize::MAX);
            while to < line.to && cs[to - line.from].is_whitespace() {
                to += 1;
            }
            if explode {
                to = from;
            } else if from > line.from && from < line.from + 100 {
                let n = from.min(cs.len());
                if cs[..n].iter().all(|c| c.is_whitespace()) {
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
        self.dispatch_tr(Some(cs), Some(sel), Some("input"));
    }

    fn change_by_selected_line(&mut self, mut f: impl FnMut(&Text, usize, usize, &mut Vec<Change>)) {
        let mut at_line = 0usize;
        let (cs, sel) = self.change_by_range(|ed, r| {
            let doc = &ed.doc;
            let mut changes = Vec::new();
            let mut pos = r.from();
            while pos <= r.to() {
                let line = doc.line_at(pos);
                if line.number > at_line && (r.is_empty() || r.to() > line.from) {
                    f(doc, line.from, line.to, &mut changes);
                    at_line = line.number;
                }
                pos = line.to + 1;
            }
            let set = ChangeSet::new(changes.clone());
            let range = Range::new(set.map_pos(r.anchor, 1), set.map_pos(r.head, 1));
            (changes, range)
        });
        self.dispatch_tr(Some(cs), Some(sel), Some("input.indent"));
    }

    /// `indentMore` (Tab).
    pub(crate) fn cmd_indent_more(&mut self) {
        let unit = self.config.indent_unit.clone();
        self.change_by_selected_line(|_, from, _, changes| {
            changes.push(Change::insert(from, unit.clone()))
        });
    }

    /// `indentLess` (Shift-Tab).
    pub(crate) fn cmd_indent_less(&mut self) {
        let tab = self.config.tab_size;
        let unit = self.config.indent_unit_cols();
        let unit_tab = self.config.indent_is_tab();
        self.change_by_selected_line(|doc, from, to, changes| {
            let text = doc.slice(from, to);
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
                from + keep,
                from + space.len(),
                insert[keep..].iter().collect::<String>(),
            ));
        });
    }

    /// Typed text in insert mode (CodeMirror's input handling).
    pub(crate) fn type_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.overwrite && !text.contains('\n') {
            self.overwrite_selection_typed(text);
            return;
        }
        let n = char_len(text);
        let (cs, sel) = self.change_by_range(|_, r| {
            (
                vec![Change::new(r.from(), r.to(), text)],
                Range::cursor(r.from() + n),
            )
        });
        self.dispatch_tr(Some(cs), Some(sel), Some("input.type"));
    }

    fn overwrite_selection_typed(&mut self, text: &str) {
        let n = char_len(text);
        let (cs, sel) = self.change_by_range(|ed, r| {
            let to = if r.is_empty() && ed.doc.char_at(r.head).is_some_and(|c| c != '\n') {
                r.head + n.min(ed.doc.line_at(r.head).to - r.head)
            } else {
                r.to()
            };
            (
                vec![Change::new(r.from(), to, text)],
                Range::cursor(r.from() + n),
            )
        });
        self.dispatch_tr(Some(cs), Some(sel), Some("input.type"));
    }

    /// `cm.findPosV(pos, amount, 'line'|'page')` over the host's visual
    /// lines (`hitSide` when it cannot move the whole amount).
    pub(crate) fn find_pos_v_line(&mut self, p: Pos, amount: i64, goal_col: Option<i64>) -> (Pos, bool) {
        let tab = self.config.tab_size;
        let mut pos = self.index_from_pos(p);
        let mut goal = goal_col.map(|g| g.max(0) as usize);
        let forward = amount > 0;
        let n = amount.unsigned_abs();
        let mut hit = false;
        for _ in 0..n {
            let (np, g) = self
                .host
                .move_vertically(&self.doc, pos, goal, forward, 1, tab);
            goal = g;
            if np == pos || (forward && np == self.doc.len() && self.doc.line_at(np).number == self.doc.line_at(pos).number) {
                hit = true;
                break;
            }
            pos = np;
        }
        (self.pos_from_index(pos), hit)
    }
}

/// `Selection::create` sorts and merges; CodeMirror keeps the order of
/// non-overlapping ranges for the primary index but sorts them too, which
/// is what `create` does.
pub(crate) fn selection_unsorted(ranges: Vec<Range>, main: usize) -> Selection {
    if ranges.len() == 1 {
        return Selection::from_range(ranges[0]);
    }
    Selection::create(ranges, main)
}
