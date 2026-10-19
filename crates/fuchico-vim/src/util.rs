//! cm-vim helpers: clipping, visual selections, marks, the jump list and
//! `clearInputState`.

use crate::editor::Editor;
use crate::pos::{self, Pos, before, first_non_ws, is_latin_word, is_ws_string, offset};
use crate::state::{InputState, JUMP_LIST_SIZE, LastSelection};

/// `makeCmSelection` result.
#[derive(Debug, Clone)]
pub(crate) struct CmSel {
    pub ranges: Vec<(Pos, Pos)>,
    pub primary: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelMode {
    Char,
    Line,
    Block,
}

impl Editor {
    /// `clipCursorToContent` (no surrogate pairs: positions are chars).
    pub(crate) fn clip_cursor_to_content(&self, cur: Pos) -> Pos {
        let include_line_break = self.vim.insert_mode || self.vim.visual_mode;
        let line = cur.line.max(self.first_line()).min(self.last_line());
        let len = self.line_len(line);
        let max_ch = len - 1 + i64::from(include_line_break);
        let ch = cur.ch.min(max_ch).max(0);
        Pos::new(line, ch)
    }

    /// `lineLength(cm, n)`.
    pub(crate) fn line_length(&self, n: i64) -> i64 {
        self.line_len(n)
    }

    /// `clearInputState(cm)` + the `vim-command-done` handlers (status
    /// reset, `<C-o>` return to insert mode).
    pub(crate) fn clear_input_state(&mut self) {
        self.vim.input_state = InputState::default();
        self.vim.expect_literal_next = false;
        self.replaying_last_edit = false;
        // plugin handler
        self.vim.status.clear();
        // oneNormalCommand handler
        if self.one_normal_pending {
            if self.vim.visual_mode {
                return;
            }
            self.one_normal_pending = false;
            if self.vim.insert_mode_return {
                self.vim.insert_mode_return = false;
                if !self.vim.insert_mode {
                    self.enter_insert_mode(&crate::state::Args::default());
                }
            }
        }
    }

    /// `CM.signal(cm, 'vim-mode-change', …)`.
    pub(crate) fn signal_mode(&mut self, mode: &'static str, sub: &str) {
        self.vim.mode = match (mode, sub) {
            ("visual", "linewise") => "visual line",
            ("visual", "blockwise") => "visual block",
            _ => mode,
        };
        self.vim.status.clear();
    }

    pub(crate) fn visual_sub_mode(&self) -> &'static str {
        if self.vim.visual_line {
            "linewise"
        } else if self.vim.visual_block {
            "blockwise"
        } else {
            ""
        }
    }

    // ---- visual selection ---------------------------------------------------

    /// `makeCmSelection`.
    pub(crate) fn make_cm_selection(
        &self,
        anchor: Pos,
        head: Pos,
        mode: SelMode,
        exclusive: bool,
    ) -> CmSel {
        match mode {
            SelMode::Char => {
                let head_off = if !exclusive && !before(head, anchor) { 1 } else { 0 };
                let anchor_off = if before(head, anchor) { 1 } else { 0 };
                CmSel {
                    ranges: vec![(offset(anchor, 0, anchor_off), offset(head, 0, head_off))],
                    primary: 0,
                }
            }
            SelMode::Line => {
                let (mut a, mut h) = (anchor, head);
                if !before(head, anchor) {
                    a.ch = 0;
                    let last = self.last_line();
                    if h.line > last {
                        h.line = last;
                    }
                    h.ch = self.line_length(h.line);
                } else {
                    h.ch = 0;
                    a.ch = self.line_length(a.line);
                }
                CmSel {
                    ranges: vec![(a, h)],
                    primary: 0,
                }
            }
            SelMode::Block => {
                let top = anchor.line.min(head.line);
                let bottom = anchor.line.max(head.line);
                let mut from_ch = anchor.ch;
                let mut to_ch = head.ch;
                if from_ch < to_ch {
                    to_ch = to_ch.saturating_add(1);
                } else {
                    from_ch = from_ch.saturating_add(1);
                }
                let height = bottom - top + 1;
                let primary = if head.line == top { 0 } else { (height - 1) as usize };
                let ranges = (0..height)
                    .map(|i| (Pos::new(top + i, from_ch), Pos::new(top + i, to_ch)))
                    .collect();
                CmSel { ranges, primary }
            }
        }
    }

    pub(crate) fn current_sel_mode(&self) -> SelMode {
        if self.vim.visual_line {
            SelMode::Line
        } else if self.vim.visual_block {
            SelMode::Block
        } else {
            SelMode::Char
        }
    }

    /// `updateCmSelection(cm)`.
    pub(crate) fn update_cm_selection(&mut self) {
        let mode = self.current_sel_mode();
        self.update_cm_selection_with(self.vim.sel_anchor, self.vim.sel_head, mode);
    }

    pub(crate) fn update_cm_selection_with(&mut self, anchor: Pos, head: Pos, mode: SelMode) {
        let s = self.make_cm_selection(anchor, head, mode, false);
        self.set_selections(&s.ranges, s.primary);
    }

    /// `exitVisualMode(cm, moveHead)`.
    pub(crate) fn exit_visual_mode(&mut self, move_head: bool) {
        if move_head {
            let h = self.clip_cursor_to_content(self.vim.sel_head);
            self.set_cursor(h);
        }
        self.update_last_selection();
        self.vim.visual_mode = false;
        self.vim.visual_line = false;
        self.vim.visual_block = false;
        if !self.vim.insert_mode {
            self.signal_mode("normal", "");
        }
    }

    /// `updateLastSelection`.
    pub(crate) fn update_last_selection(&mut self) {
        let anchor = self.vim.sel_anchor;
        let mut head = self.vim.sel_head;
        if let Some(t) = self.vim.last_pasted_text.take() {
            let i = self.index_from_pos(anchor) + pos::clen(&t) as usize;
            head = self.pos_from_index(i);
        }
        // cm-vim keeps the old bookmarks alive (`gv` in visual mode reads
        // the previous selection after this update); so do we
        let anchor_mark = self.set_bookmark(anchor, false);
        let head_mark = self.set_bookmark(head, false);
        self.vim.last_selection = Some(LastSelection {
            anchor_mark,
            head_mark,
            anchor,
            head,
            visual_mode: self.vim.visual_mode,
            visual_line: self.vim.visual_line,
            visual_block: self.vim.visual_block,
        });
    }

    /// `expandSelection(cm, start, end, move)`.
    pub(crate) fn expand_selection(&self, start: Pos, end: Pos, mv: bool) -> (Pos, Pos) {
        let (mut start, mut end) = (start, end);
        let mut head = if mv { start } else { self.vim.sel_head };
        let mut anchor = if mv { start } else { self.vim.sel_anchor };
        if before(end, start) {
            std::mem::swap(&mut start, &mut end);
        }
        if before(head, anchor) {
            head = pos::min(start, head);
            anchor = pos::max(anchor, end);
        } else {
            anchor = pos::min(start, anchor);
            head = pos::max(head, end);
            head = offset(head, 0, -1);
            if head.ch == -1 && head.line != self.first_line() {
                head = Pos::new(head.line - 1, self.line_length(head.line - 1));
            }
        }
        (anchor, head)
    }

    /// `getSelectedAreaRange`.
    pub(crate) fn get_selected_area_range(&self) -> (Pos, Pos) {
        let sels = self.list_selections();
        let start = sels[0];
        let end = sels[sels.len() - 1];
        let s = if before(start.0, start.1) { start.0 } else { start.1 };
        let e = if before(end.0, end.1) { end.1 } else { end.0 };
        (s, e)
    }

    /// `selectBlock(cm, selectionEnd)`: returns the new base and the
    /// adjusted selection end.
    pub(crate) fn select_block(&mut self, selection_end: Pos) -> (Pos, Pos) {
        let ranges = self.list_selections();
        let head = self.clip_pos(selection_end);
        let is_clipped = selection_end != head;
        let cur_head = self.head();
        let prim_index = get_index(&ranges, cur_head).max(0) as usize;
        let was_clipped = ranges[prim_index].1 == ranges[prim_index].0;
        let max = ranges.len() - 1;
        let index = if max - prim_index > prim_index { max } else { 0 };
        let mut base = ranges[index].0;
        let first = base.line.min(head.line);
        let last = base.line.max(head.line);
        let mut base_ch = base.ch;
        let mut head_ch = head.ch;
        let dir = ranges[index].1.ch - base_ch;
        let new_dir = head_ch - base_ch;
        if dir > 0 && new_dir <= 0 {
            base_ch += 1;
            if !is_clipped {
                head_ch -= 1;
            }
        } else if dir < 0 && new_dir >= 0 {
            base_ch -= 1;
            if !was_clipped {
                head_ch += 1;
            }
        } else if dir < 0 && new_dir == -1 {
            base_ch -= 1;
            head_ch += 1;
        }
        let sels: Vec<(Pos, Pos)> = (first..=last)
            .map(|l| (Pos::new(l, base_ch), Pos::new(l, head_ch)))
            .collect();
        self.set_selections(&sels, 0);
        base.ch = base_ch;
        (base, Pos::new(selection_end.line, head_ch))
    }

    /// `selectForInsert(cm, head, height)`.
    pub(crate) fn select_for_insert(&mut self, head: Pos, height: i64) {
        let sels: Vec<(Pos, Pos)> = (0..height.max(1))
            .map(|i| {
                let p = offset(head, i, 0);
                (p, p)
            })
            .collect();
        self.set_selections(&sels, 0);
    }

    /// `clipToLine(cm, curStart, curEnd)`: adjusts `cur_end`.
    pub(crate) fn clip_to_line(&self, cur_start: Pos, cur_end: &mut Pos) {
        let selection = self.get_range(cur_start, *cur_end);
        // /\n\s*$/
        let trimmed = selection.trim_end_matches(pos::is_ws);
        let tail = &selection[trimmed.len()..];
        if !tail.contains('\n') {
            return;
        }
        let mut lines: Vec<&str> = selection.split('\n').collect();
        lines.pop();
        let mut line = lines.pop();
        while !lines.is_empty() && line.is_some_and(|l| !l.is_empty() && is_ws_string(l)) {
            cur_end.line -= 1;
            cur_end.ch = 0;
            line = lines.pop();
        }
        if line.is_some_and(|l| !l.is_empty()) {
            cur_end.line -= 1;
            cur_end.ch = self.line_length(cur_end.line);
        } else {
            cur_end.ch = 0;
        }
    }

    // ---- marks ----------------------------------------------------------------

    /// `updateMark(cm, vim, name, pos)`.
    pub(crate) fn update_mark(&mut self, name: char, p: Pos) {
        if name != '<' && name != '>' && !is_latin_word(name) {
            return;
        }
        if let Some(old) = self.vim.marks.remove(&name) {
            self.clear_marker(old);
        }
        let id = self.set_bookmark(p, false);
        self.vim.marks.insert(name, id);
    }

    /// `getMarkPos(cm, vim, name)`.
    pub(crate) fn get_mark_pos(&mut self, name: char) -> Option<Pos> {
        if name == '\'' || name == '`' {
            return Some(self.jump_find(-1).unwrap_or(Pos::new(0, 0)));
        }
        if name == '.' {
            return Some(self.pos_from_index(self.last_change_end_offset));
        }
        let id = *self.vim.marks.get(&name)?;
        self.find_marker(id)
    }

    pub(crate) fn first_non_ws_of(&self, line: i64) -> i64 {
        first_non_ws(&self.get_line(line))
    }

    // ---- jump list --------------------------------------------------------------

    fn jl_slot(p: i64) -> Option<usize> {
        if p < 0 {
            None
        } else {
            Some((p % JUMP_LIST_SIZE) as usize)
        }
    }

    /// `recordJumpPosition`.
    pub(crate) fn record_jump_position(&mut self, old: Pos, new: Pos) {
        if old != new {
            self.jump_add(old, new);
        }
    }

    fn jump_use_next_slot(&mut self, cursor: Pos) {
        self.vim.jump_list.pointer += 1;
        let next = (self.vim.jump_list.pointer % JUMP_LIST_SIZE) as usize;
        if let Some(trash) = self.vim.jump_list.buffer[next].take() {
            self.clear_marker(trash);
        }
        let id = self.set_bookmark(cursor, false);
        self.vim.jump_list.buffer[next] = Some(id);
    }

    pub(crate) fn jump_add(&mut self, old: Pos, new: Pos) {
        let cur_mark = Self::jl_slot(self.vim.jump_list.pointer)
            .and_then(|i| self.vim.jump_list.buffer[i]);
        match cur_mark {
            Some(m) => {
                if let Some(mp) = self.find_marker(m)
                    && mp != old
                {
                    self.jump_use_next_slot(old);
                }
            }
            None => self.jump_use_next_slot(old),
        }
        self.jump_use_next_slot(new);
        let jl = &mut self.vim.jump_list;
        jl.head = jl.pointer;
        jl.tail = (jl.pointer - JUMP_LIST_SIZE + 1).max(0);
    }

    fn jl_mark(&self, p: i64) -> Option<usize> {
        let i = (JUMP_LIST_SIZE + p).rem_euclid(JUMP_LIST_SIZE) as usize;
        if JUMP_LIST_SIZE + p < 0 {
            return None;
        }
        self.vim.jump_list.buffer[i]
    }

    /// `jumpList.move(cm, offset)`: a marker id.
    pub(crate) fn jump_move(&mut self, off: i64) -> Option<usize> {
        {
            let jl = &mut self.vim.jump_list;
            jl.pointer += off;
            if jl.pointer > jl.head {
                jl.pointer = jl.head;
            } else if jl.pointer < jl.tail {
                jl.pointer = jl.tail;
            }
        }
        let mut mark = self.jl_mark(self.vim.jump_list.pointer);
        if let Some(m) = mark
            && self.find_marker(m).is_none()
        {
            let inc = if off > 0 { 1 } else { -1 };
            let old = self.head();
            loop {
                self.vim.jump_list.pointer += inc;
                mark = self.jl_mark(self.vim.jump_list.pointer);
                if let Some(m) = mark
                    && let Some(np) = self.find_marker(m)
                    && np != old
                {
                    break;
                }
                let jl = &self.vim.jump_list;
                if !(jl.pointer < jl.head && jl.pointer > jl.tail) {
                    break;
                }
            }
        }
        mark
    }

    /// `jumpList.find(cm, offset)`.
    pub(crate) fn jump_find(&mut self, off: i64) -> Option<Pos> {
        let old = self.vim.jump_list.pointer;
        let m = self.jump_move(off);
        self.vim.jump_list.pointer = old;
        m.and_then(|m| self.find_marker(m))
    }

    // ---- visible lines -------------------------------------------------------------

    /// `getUserVisibleLines` (0-based, inclusive).
    pub(crate) fn user_visible_lines(&mut self) -> (i64, i64) {
        match self.host.visible_lines() {
            Some((a, b)) => {
                let last = self.last_line();
                ((a as i64 - 1).clamp(0, last), (b as i64 - 1).clamp(0, last))
            }
            None => (0, self.last_line()),
        }
    }
}

/// `getIndex(ranges, cursor)`.
pub(crate) fn get_index(ranges: &[(Pos, Pos)], cursor: Pos) -> i64 {
    for (i, r) in ranges.iter().enumerate() {
        if r.0 == cursor || r.1 == cursor {
            return i as i64;
        }
    }
    -1
}
