//! The original's `@codemirror/search` UI and the selection commands that
//! ship with it: the Mod-f find/replace panel (docked on top), F3 / Mod-g
//! `findNext` / `findPrevious` (also with the panel closed), the
//! Mod-Alt-g `gotoLine` dialog (docked at the bottom), Mod-d
//! `selectNextOccurrence`, Mod-Shift-l `selectSelectionMatches`,
//! `highlightSelectionMatches`, `undoSelection` / `redoSelection` and
//! Alt-drag `rectangularSelection`. The pure matching logic lives in
//! [`crate::search`]; this file holds the view state, input routing and the
//! panel / dialog / outline rendering.

use std::ops::Range;

use fuchico_helix::text::{count_column, find_column};
use fuchico_helix::{Change, ModeType, Range as HxRange, Scroll, ScrollAlign, Selection};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, ClickEvent, ClipboardItem, Context, HighlightStyle, InteractiveElement,
    IntoElement, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels,
    Point, ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled, StyledText, Window,
    div, px, relative,
};

use crate::element::Frame;
use crate::search::{self, NextOccurrence, SearchQuery};
use crate::style::mix;
use crate::view::{EditorView, EngineKind, OutlineState, Overlay, to_helix, typed_char};

/// User-visible strings of the editor's own UI (the host translates them;
/// the defaults are the original's English labels).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorStrings {
    /// Search field placeholder (`Find`).
    pub find: String,
    /// Replace field placeholder (`Replace`).
    pub replace: String,
    pub next: String,
    pub previous: String,
    pub all: String,
    pub match_case: String,
    pub regexp: String,
    pub by_word: String,
    /// The `replace` button.
    pub replace_one: String,
    pub replace_all: String,
    /// `gotoLine` dialog label and submit button.
    pub go_to_line: String,
    pub go: String,
    /// Outline overlay placeholder and empty state.
    pub go_to_header: String,
    pub no_headers: String,
}

impl Default for EditorStrings {
    fn default() -> Self {
        EditorStrings {
            find: "Find".into(),
            replace: "Replace".into(),
            next: "next".into(),
            previous: "previous".into(),
            all: "all".into(),
            match_case: "match case".into(),
            regexp: "regexp".into(),
            by_word: "by word".into(),
            replace_one: "replace".into(),
            replace_all: "replace all".into(),
            go_to_line: "Go to line".into(),
            go: "go".into(),
            go_to_header: "Go to header…".into(),
            no_headers: "No headers found".into(),
        }
    }
}

/// A text field of the search panel / go-to-line dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindField {
    Search,
    Replace,
    Goto,
}

#[derive(Debug, Clone)]
enum SelEntry {
    /// A selection-only step: the selection before it.
    Sel(Selection),
    /// A document change (undone through the engine history).
    Change,
}

#[derive(Debug, Clone)]
pub(crate) struct RectDrag {
    line: usize,
    col: usize,
    start: Selection,
    multiple: bool,
}

/// Search panel, go-to-line dialog, selection history and rectangular
/// drag state.
#[derive(Debug, Default)]
pub(crate) struct FindModel {
    pub(crate) query: SearchQuery,
    /// The search state exists (the panel was opened at least once).
    started: bool,
    /// The search panel is shown.
    pub(crate) open: bool,
    /// The go-to-line dialog's input, when shown.
    pub(crate) goto: Option<String>,
    /// The panel field that has the keyboard (the editor has it otherwise).
    pub(crate) focus: Option<FindField>,
    /// The focused field's text is selected (typing replaces it).
    pub(crate) all_selected: bool,
    done: Vec<SelEntry>,
    undone: Vec<SelEntry>,
    last_sel: Option<Selection>,
    restoring: bool,
    drag_recorded: bool,
    pub(crate) rect: Option<RectDrag>,
}

impl FindModel {
    /// Starts the selection history at `sel` (a new document).
    pub(crate) fn reset_history(&mut self, sel: Selection) {
        self.done.clear();
        self.undone.clear();
        self.last_sel = Some(sel);
    }
    /// A pointer gesture ended: the next one is a new history step.
    pub(crate) fn end_drag(&mut self) {
        self.drag_recorded = false;
    }
}

/// Selection history depth (CodeMirror's `minDepth` is 100).
const SEL_HISTORY: usize = 200;

impl EditorView {
    // ---- public accessors ------------------------------------------------------

    /// The search query while the panel is open.
    pub fn search_panel(&self) -> Option<SearchQuery> {
        self.find.open.then(|| self.find.query.clone())
    }
    /// The (persisted) search query, also after the panel closed.
    pub fn search_query(&self) -> &SearchQuery {
        &self.find.query
    }
    /// The focused panel field, if a panel has the keyboard.
    pub fn focused_field(&self) -> Option<FindField> {
        self.find.focus
    }
    /// The go-to-line dialog's input, if open.
    pub fn goto_dialog(&self) -> Option<String> {
        self.find.goto.clone()
    }
    /// `(query, index of the selected match, match count)` while the
    /// search panel is open.
    pub fn find_query(&self) -> Option<(String, usize, usize)> {
        if !self.find.open {
            return None;
        }
        let q = self.find.query.search.clone();
        let Some(m) = self.find.query.matcher() else {
            return Some((q, 0, 0));
        };
        let text = self.model.text();
        let all: Vec<Range<usize>> = m.matches_in(text, 0, text.len()).collect();
        let r = self.engine.selection().main();
        let cur = self.byte_of(r.from())..self.byte_of(r.to());
        let i = all.iter().position(|m| *m == cur).unwrap_or(0);
        Some((q, i, all.len()))
    }
    /// Char ranges painted as search matches in the last frame (`true` =
    /// the match is selected).
    pub fn debug_search_highlights(&self) -> Vec<(Range<usize>, bool)> {
        let Some(vis) = self.visible_bytes() else {
            return Vec::new();
        };
        self.search_highlights(vis)
            .into_iter()
            .map(|(r, s)| (self.char_of(r.start)..self.char_of(r.end), s))
            .collect()
    }
    /// Char ranges painted as `.cm-selectionMatch` in the last frame.
    pub fn debug_selection_matches(&self) -> Vec<Range<usize>> {
        let Some(vis) = self.visible_bytes() else {
            return Vec::new();
        };
        let found = self.search_highlights(vis.clone());
        self.selection_match_ranges(vis, &found)
            .into_iter()
            .map(|r| self.char_of(r.start)..self.char_of(r.end))
            .collect()
    }
    /// Active row of the outline overlay.
    pub fn outline_active(&self) -> Option<usize> {
        match &self.overlay {
            Overlay::Outline(o) => Some(o.selected),
            _ => None,
        }
    }

    // ---- helpers ------------------------------------------------------------------

    fn visible_bytes(&self) -> Option<Range<usize>> {
        let f = self.frame.as_ref()?;
        Some(f.lines.first()?.disp.src.start..f.lines.last()?.disp.src.end)
    }

    fn sel_bytes(&self) -> Vec<Range<usize>> {
        self.engine
            .selection()
            .ranges()
            .iter()
            .map(|r| self.byte_of(r.from())..self.byte_of(r.to()))
            .collect()
    }

    fn select_bytes(&mut self, ranges: &[Range<usize>], main: usize, cx: &mut Context<Self>) {
        if ranges.is_empty() {
            return;
        }
        let rs = ranges
            .iter()
            .map(|r| HxRange::new(self.char_of(r.start), self.char_of(r.end)))
            .collect();
        let fx = self.engine.set_selection(Selection::create(rs, main));
        self.apply_effects(fx, cx);
    }

    /// Search matches to paint in `visible` (bytes), with "selected".
    pub(crate) fn search_highlights(&self, visible: Range<usize>) -> Vec<(Range<usize>, bool)> {
        if !self.find.open {
            return Vec::new();
        }
        let Some(m) = self.find.query.matcher() else {
            return Vec::new();
        };
        let text = self.model.text();
        let sel = self.sel_bytes();
        let to = (visible.end + 1000).min(text.len());
        m.matches_in(text, visible.start, to)
            .filter(|r| r.start <= visible.end)
            .take(10_000)
            .map(|r| {
                let s = sel.contains(&r);
                (r, s)
            })
            .collect()
    }

    /// `highlightSelectionMatches` ranges in `visible` (bytes), minus the
    /// ones inside a search match (`.cm-searchMatch .cm-selectionMatch`
    /// is transparent).
    pub(crate) fn selection_match_ranges(
        &self,
        visible: Range<usize>,
        found: &[(Range<usize>, bool)],
    ) -> Vec<Range<usize>> {
        let mut out = search::selection_matches(self.model.text(), &self.sel_bytes(), visible);
        out.retain(|r| {
            !found
                .iter()
                .any(|(f, _)| f.start <= r.start && r.end <= f.end)
        });
        out
    }

    fn focus_field(&mut self, f: FindField) {
        self.find.focus = Some(f);
        self.find.all_selected = true;
        self.completion = None;
    }

    // ---- search panel ---------------------------------------------------------------

    /// Mod-f (`openSearchPanel`).
    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            return;
        }
        let r = self.engine.selection().main();
        let seeded = if r.to() - r.from() > 100 {
            None
        } else {
            search::default_search(&self.engine.doc().slice(r.from(), r.to()))
        };
        let seeded = seeded.map(|s| SearchQuery {
            search: s,
            replace: String::new(),
            ..self.find.query.clone()
        });
        if self.find.open {
            if self.find.focus != Some(FindField::Search) {
                if let Some(q) = seeded.filter(SearchQuery::valid) {
                    self.find.query = q;
                }
                self.focus_field(FindField::Search);
            }
        } else {
            if let Some(q) = seeded {
                self.find.query = q;
            } else if !self.find.started {
                self.find.query = SearchQuery::default();
            }
            self.find.started = true;
            self.find.open = true;
            self.focus_field(FindField::Search);
        }
        cx.notify();
    }

    /// `closeSearchPanel` (Escape in the panel, the `×` button).
    pub(crate) fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find.open = false;
        if matches!(
            self.find.focus,
            Some(FindField::Search | FindField::Replace)
        ) {
            self.find.focus = None;
        }
        cx.notify();
    }

    /// `findNext` / `findPrevious`: open the panel when there is no valid
    /// query yet.
    pub fn find_next(&mut self, prev: bool, cx: &mut Context<Self>) {
        if !self.find.started || !self.find.query.valid() {
            self.open_find(cx);
            return;
        }
        let Some(m) = self.find.query.matcher() else {
            return;
        };
        let r = self.engine.selection().main();
        let (from, to) = (self.byte_of(r.from()), self.byte_of(r.to()));
        let text = self.model.text();
        let hit = if prev {
            m.prev_match(text, from, to)
        } else {
            m.next_match(text, to, to)
        };
        let Some(h) = hit else { return };
        self.select_bytes(&[h], 0, cx);
        if self.find.focus == Some(FindField::Search) {
            self.find.all_selected = true;
        }
        cx.notify();
    }

    /// The panel's `all` button (`selectMatches`).
    pub(crate) fn select_all_matches(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.find.query.matcher() else {
            return;
        };
        let Some(all) = m.match_all(self.model.text(), 1000) else {
            return;
        };
        self.select_bytes(&all, 0, cx);
    }

    /// `replaceNext`: replace the selected match and select the next one,
    /// or just select the next match.
    pub(crate) fn replace_next(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.find.query.matcher() else {
            return;
        };
        let r = self.engine.selection().main();
        let (from, to) = (self.byte_of(r.from()), self.byte_of(r.to()));
        let text = self.model.text();
        let Some(next) = m.next_match(text, from, from) else {
            return;
        };
        let mut target = Some(next.clone());
        let mut change = None;
        if next == (from..to) {
            change = Some((next.clone(), m.replacement(text, &next)));
            target = m.next_match(text, next.start, next.end);
        }
        let target = target.map(|t| (self.char_of(t.start), self.char_of(t.end)));
        let mut edit: Option<(usize, usize, usize)> = None;
        if let Some((rg, rep)) = change {
            let (ca, cb) = (self.char_of(rg.start), self.char_of(rg.end));
            edit = Some((ca, cb, rep.chars().count()));
            let fx = self.engine.apply_changes(vec![Change::new(ca, cb, rep)]);
            self.apply_effects(fx, cx);
        }
        let shift = |p: usize| match edit {
            Some((ca, cb, n)) if p >= cb => p - (cb - ca) + n,
            _ => p,
        };
        if let Some((a, b)) = target {
            let fx = self
                .engine
                .set_selection(Selection::single(shift(a), shift(b)));
            self.apply_effects(fx, cx);
        }
        cx.notify();
    }

    /// `replaceAll`.
    pub(crate) fn replace_all(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.find.query.matcher() else {
            return;
        };
        let text = self.model.text();
        let Some(all) = m.match_all(text, usize::MAX) else {
            return;
        };
        let changes: Vec<Change> = all
            .iter()
            .map(|r| {
                Change::new(
                    self.char_of(r.start),
                    self.char_of(r.end),
                    m.replacement(text, r),
                )
            })
            .collect();
        if changes.is_empty() {
            return;
        }
        let fx = self.engine.apply_changes(changes);
        self.apply_effects(fx, cx);
        cx.notify();
    }

    fn toggle(&mut self, which: u8, cx: &mut Context<Self>) {
        let q = &mut self.find.query;
        match which {
            0 => q.case_sensitive = !q.case_sensitive,
            1 => q.regexp = !q.regexp,
            _ => q.whole_word = !q.whole_word,
        }
        cx.notify();
    }

    // ---- go to line -------------------------------------------------------------------

    /// Mod-Alt-g (`gotoLine`): the dialog, prefilled with the cursor line.
    pub fn open_goto(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            return;
        }
        let head = self.engine.selection().main().head;
        let line = self.engine.doc().line_at(head).number;
        self.find.goto = Some(line.to_string());
        self.focus_field(FindField::Goto);
        cx.notify();
    }

    fn close_goto(&mut self, cx: &mut Context<Self>) {
        self.find.goto = None;
        if self.find.focus == Some(FindField::Goto) {
            self.find.focus = None;
        }
        cx.notify();
    }

    fn submit_goto(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.find.goto.clone() else {
            return;
        };
        self.close_goto(cx);
        let doc = self.engine.doc();
        let cur = doc.line_at(self.engine.selection().main().head).number;
        let Some((line, col)) = search::parse_goto(&input, cur, doc.lines()) else {
            return;
        };
        let l = doc.line(line);
        let pos = l.from + col.min(l.len());
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::single(pos, pos));
        self.apply_effects(fx, cx);
        self.pending_scroll = Some(Scroll::To {
            pos,
            align: ScrollAlign::Center,
        });
        cx.notify();
    }

    // ---- panel keyboard --------------------------------------------------------------

    /// Keys while a panel field has the keyboard. Returns whether it
    /// handled (everything but platform shortcuts, which are actions).
    pub(crate) fn panel_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(f) = self.find.focus else {
            return false;
        };
        if !matches!(self.overlay, Overlay::None) {
            return false;
        }
        let m = ks.modifiers;
        match ks.key.as_str() {
            "escape" => match f {
                FindField::Goto => self.close_goto(cx),
                _ => self.close_find(cx),
            },
            "enter" => match f {
                FindField::Search => self.find_next(m.shift, cx),
                FindField::Replace => self.replace_next(cx),
                FindField::Goto => self.submit_goto(cx),
            },
            "tab" if f != FindField::Goto => {
                let to = if f == FindField::Search {
                    FindField::Replace
                } else {
                    FindField::Search
                };
                self.focus_field(to);
            }
            "backspace" => {
                let all = std::mem::take(&mut self.find.all_selected);
                if let Some(t) = self.field_mut(f) {
                    if all {
                        t.clear();
                    } else {
                        t.pop();
                    }
                }
            }
            "delete" => {
                if std::mem::take(&mut self.find.all_selected)
                    && let Some(t) = self.field_mut(f)
                {
                    t.clear();
                }
            }
            "left" | "right" | "home" | "end" | "up" | "down" => self.find.all_selected = false,
            _ => {
                if let Some(s) = typed_char(ks) {
                    self.field_input(&s, cx);
                }
            }
        }
        cx.notify();
        true
    }

    fn field_mut(&mut self, f: FindField) -> Option<&mut String> {
        match f {
            FindField::Search => Some(&mut self.find.query.search),
            FindField::Replace => Some(&mut self.find.query.replace),
            FindField::Goto => self.find.goto.as_mut(),
        }
    }

    /// Text typed (or pasted / IME-committed) into the focused field.
    /// Returns whether a field took it.
    pub(crate) fn field_input(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(f) = self.find.focus else {
            return false;
        };
        if !matches!(self.overlay, Overlay::None) {
            return false;
        }
        let all = std::mem::take(&mut self.find.all_selected);
        if let Some(t) = self.field_mut(f) {
            if all {
                t.clear();
            }
            t.push_str(text);
        }
        cx.notify();
        true
    }

    /// Mod-c / Mod-x / Mod-v / Mod-a / Mod-z with a field focused: act on
    /// the field (`op` = `c`, `x`, `v`, `a`, `z`). Returns whether it did.
    pub(crate) fn field_clipboard(&mut self, op: char, cx: &mut Context<Self>) -> bool {
        let Some(f) = self.find.focus else {
            return false;
        };
        if !matches!(self.overlay, Overlay::None) {
            return false;
        }
        let all = self.find.all_selected;
        match op {
            'c' | 'x' => {
                if all && let Some(t) = self.field_mut(f) {
                    let s = t.clone();
                    if op == 'x' {
                        t.clear();
                        self.find.all_selected = false;
                    }
                    cx.write_to_clipboard(ClipboardItem::new_string(s));
                }
            }
            'v' => {
                if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    self.field_input(&t.replace('\n', " "), cx);
                }
            }
            'a' => self.find.all_selected = true,
            _ => {}
        }
        cx.notify();
        true
    }

    /// Escape in the editor with the non-modal engine closes the panel
    /// (`searchKeymap`'s Escape has scope `editor search-panel`).
    pub(crate) fn editor_escape_closes_panel(
        &mut self,
        ks: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let m = ks.modifiers;
        if self.options.engine == EngineKind::Normal
            && self.find.open
            && ks.key == "escape"
            && !m.control
            && !m.alt
            && !m.shift
        {
            self.close_find(cx);
            return true;
        }
        false
    }

    // ---- selection commands -------------------------------------------------------------

    /// On non-mac Mod-* is Ctrl-*: the Helix normal/select keymap owns
    /// Ctrl-d / Ctrl-u (half-page moves) and wins over CodeMirror's keymap.
    fn helix_owns_ctrl(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if cfg!(target_os = "macos")
            || self.options.engine != EngineKind::Helix
            || self.mode() == ModeType::Insert
        {
            return false;
        }
        if let Ok(ks) = Keystroke::parse(key)
            && let Some(ev) = to_helix(&ks)
        {
            self.feed_key(ev, cx);
        }
        true
    }

    /// Mod-d (`selectNextOccurrence`).
    pub fn select_next_occurrence(&mut self, cx: &mut Context<Self>) {
        if self.helix_owns_ctrl("ctrl-d", cx) {
            return;
        }
        let sel = self.engine.selection().clone();
        let ranges = self.sel_bytes();
        let heads: Vec<usize> = sel.ranges().iter().map(|r| self.byte_of(r.head)).collect();
        let text = self.model.text();
        match search::select_next_occurrence(text, &ranges, &heads, sel.main_index()) {
            None => {}
            Some(NextOccurrence::Words(ws)) => self.select_bytes(&ws, sel.main_index(), cx),
            Some(NextOccurrence::Add(r)) => {
                let (a, b) = (self.char_of(r.start), self.char_of(r.end));
                let fx = self
                    .engine
                    .set_selection(sel.add_range(HxRange::new(a, b), false));
                self.apply_effects(fx, cx);
                self.pending_scroll = Some(Scroll::To {
                    pos: b,
                    align: ScrollAlign::Nearest,
                });
            }
        }
        cx.notify();
    }

    /// Mod-Shift-l (`selectSelectionMatches`).
    pub fn select_selection_matches(&mut self, cx: &mut Context<Self>) {
        let ranges = self.sel_bytes();
        if let Some((all, main)) = search::select_selection_matches(self.model.text(), &ranges) {
            self.select_bytes(&all, main, cx);
        }
    }

    // ---- selection history ----------------------------------------------------------------

    /// Called after every engine update: records selection-only steps
    /// (one per mouse drag) and document changes for `undoSelection`.
    pub(crate) fn record_selection(&mut self, doc_changed: bool) {
        let cur = self.engine.selection().clone();
        let prev = self.find.last_sel.replace(cur.clone());
        if self.find.restoring {
            return;
        }
        let dragging = self.drag_anchor.is_some() || self.find.rect.is_some();
        if !dragging {
            self.find.drag_recorded = false;
        }
        let entry = if doc_changed {
            if matches!(self.find.done.last(), Some(SelEntry::Change)) {
                self.find.undone.clear();
                return;
            }
            SelEntry::Change
        } else {
            let Some(prev) = prev else { return };
            if prev == cur {
                return;
            }
            if dragging {
                if self.find.drag_recorded {
                    return;
                }
                self.find.drag_recorded = true;
            }
            SelEntry::Sel(prev)
        };
        self.find.done.push(entry);
        if self.find.done.len() > SEL_HISTORY {
            self.find.done.remove(0);
        }
        self.find.undone.clear();
    }

    /// Mod-u / Mod-Shift-u (`undoSelection` / `redoSelection`).
    pub fn undo_selection(&mut self, redo: bool, cx: &mut Context<Self>) {
        if !redo && self.helix_owns_ctrl("ctrl-u", cx) {
            return;
        }
        let entry = if redo {
            self.find.undone.pop()
        } else {
            self.find.done.pop()
        };
        let Some(entry) = entry else { return };
        self.find.restoring = true;
        let back = match entry {
            SelEntry::Sel(s) => {
                let cur = self.engine.selection().clone();
                let s = s.clamp(self.engine.doc().len());
                let fx = self.engine.set_selection(s);
                self.apply_effects(fx, cx);
                SelEntry::Sel(cur)
            }
            SelEntry::Change => {
                self.undo(redo, cx);
                SelEntry::Change
            }
        };
        self.find.restoring = false;
        if redo {
            self.find.done.push(back);
        } else {
            self.find.undone.push(back);
        }
        cx.notify();
    }

    // ---- rectangular selection (Alt-drag) -------------------------------------------

    /// `getPos` of `rectangularSelection`: 1-based line and visual column
    /// under `p` (past the line end: the pointer's column).
    fn rect_pos(&self, frame: &Frame, p: Point<Pixels>) -> Option<(usize, usize)> {
        let (byte, line0) = frame.byte_at(p, false)?;
        let ch = self.char_of(byte);
        let doc = self.engine.doc();
        let line = doc.line_at(ch);
        let off = ch - line.from;
        let tab = self.options.tab_size.max(1) as usize;
        let col = if off == line.len() {
            let fl = frame.lines.iter().find(|l| l.line == line0)?;
            let w = f32::from(frame.char_w).max(1.);
            (f32::from((p.x - fl.x).max(px(0.))) / w).round() as usize
        } else {
            count_column(&doc.slice(line.from, line.to), tab, off)
        };
        Some((line.number, col))
    }

    fn rect_apply(&mut self, cur: (usize, usize), cx: &mut Context<Self>) {
        let Some(r) = self.find.rect.clone() else {
            return;
        };
        let doc = self.engine.doc();
        let tab = self.options.tab_size.max(1) as usize;
        let (l0, l1) = (r.line.min(cur.0), r.line.max(cur.0));
        let (c0, c1) = (r.col.min(cur.1), r.col.max(cur.1));
        let mut ranges = Vec::new();
        for i in l0..=l1 {
            let line = doc.line(i);
            let text = doc.slice(line.from, line.to);
            match find_column(&text, c0, tab, true) {
                None => ranges.push(HxRange::cursor(line.to)),
                Some(s) => {
                    let e = find_column(&text, c1, tab, false).unwrap_or(line.len());
                    ranges.push(HxRange::new(line.from + s, line.from + e));
                }
            }
        }
        if r.multiple {
            ranges.extend_from_slice(r.start.ranges());
        }
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::create(ranges, 0));
        self.apply_effects(fx, cx);
        self.pending_scroll = None;
    }

    /// Alt + left press in the text: start a rectangular selection.
    pub(crate) fn rect_mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        if !ev.modifiers.alt {
            return false;
        }
        let Some(frame) = self.frame.clone() else {
            return false;
        };
        let Some((line, col)) = self.rect_pos(&frame, ev.position) else {
            return false;
        };
        let multiple = if cfg!(target_os = "macos") {
            ev.modifiers.platform
        } else {
            ev.modifiers.control
        };
        self.drag_anchor = None;
        self.find.rect = Some(RectDrag {
            line,
            col,
            start: self.engine.selection().clone(),
            multiple,
        });
        self.rect_apply((line, col), cx);
        true
    }

    pub(crate) fn rect_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        if self.find.rect.is_none() {
            return false;
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            self.find.rect = None;
            return true;
        }
        if let Some(frame) = self.frame.clone()
            && let Some(cur) = self.rect_pos(&frame, ev.position)
        {
            self.rect_apply(cur, cx);
        }
        true
    }

    // ---- rendering -------------------------------------------------------------------

    fn field_el(
        &self,
        id: &'static str,
        field: FindField,
        text: &str,
        placeholder: &str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let st = &self.style;
        let focused = self.find.focus == Some(field);
        let sel = focused && self.find.all_selected && !text.is_empty();
        let caret = || div().w(px(1.)).h(px(14.)).bg(st.foreground).flex_none();
        let mut el = div()
            .id(id)
            .debug_selector(|| id.to_string())
            .flex()
            .flex_none()
            .items_center()
            .w(px(width))
            .h(px(24.))
            .px(px(6.))
            .border_1()
            .border_color(if focused { st.ring } else { st.border })
            .rounded(st.radius_sm)
            .bg(st.background)
            .text_color(st.foreground)
            .text_size(px(12.))
            .overflow_hidden()
            .whitespace_nowrap()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |v, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    v.find.focus = Some(field);
                    v.find.all_selected = false;
                    v.completion = None;
                    cx.notify();
                }),
            );
        if text.is_empty() {
            if focused {
                el = el.child(caret());
            }
            el = el.child(
                div()
                    .text_color(st.muted_foreground)
                    .child(placeholder.to_string()),
            );
        } else {
            el = el.child(
                div()
                    .when(sel, |d| d.bg(st.selection()))
                    .child(text.to_string()),
            );
            if focused && !sel {
                el = el.child(caret());
            }
        }
        el.into_any_element()
    }

    fn button_el(
        &self,
        id: &'static str,
        label: &str,
        on: fn(&mut EditorView, &mut Context<EditorView>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let st = &self.style;
        let (hbg, hfg) = (st.accent, st.accent_foreground);
        div()
            .id(id)
            .debug_selector(|| id.to_string())
            .flex_none()
            .px(px(8.))
            .py(px(3.))
            .border_1()
            .border_color(st.border)
            .rounded(st.radius_sm)
            .bg(st.background)
            .text_color(st.foreground)
            .text_size(px(11.))
            .hover(move |s| s.bg(hbg).text_color(hfg))
            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| on(v, cx)))
            .child(label.to_string())
            .into_any_element()
    }

    fn checkbox_el(
        &self,
        id: &'static str,
        label: &str,
        on: bool,
        which: u8,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let st = &self.style;
        div()
            .id(id)
            .debug_selector(|| id.to_string())
            .flex()
            .flex_none()
            .items_center()
            .text_size(px(11.))
            .text_color(st.muted_foreground)
            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.toggle(which, cx)))
            .child(
                div()
                    .ml(px(6.))
                    .mr(px(2.))
                    .size(px(13.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(if on { st.primary } else { st.border })
                    .bg(if on { st.primary } else { st.background })
                    .when(on, |d| {
                        d.child(
                            div()
                                .text_size(px(10.))
                                .text_color(st.primary_foreground)
                                .child("✓"),
                        )
                    }),
            )
            .child(label.to_string())
            .into_any_element()
    }

    /// The search panel (`.cm-panels-top .cm-search`), when open.
    pub(crate) fn render_find_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.find.open {
            return None;
        }
        let st = &self.style;
        let s = self.options.strings.clone();
        let q = self.find.query.clone();
        let (muted, fg) = (st.muted_foreground, st.foreground);
        let stop =
            |_: &MouseDownEvent, _: &mut Window, cx: &mut gpui_kit::App| cx.stop_propagation();
        Some(
            div()
                .id("fuchico-editor-find")
                .debug_selector(|| "fuchico-editor-find".into())
                .relative()
                .flex()
                .flex_none()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .py(px(6.))
                .bg(st.secondary)
                .text_color(st.foreground)
                .border_b_1()
                .border_color(st.border)
                .font_family(st.sans_family.clone())
                .text_size(px(12.))
                .on_mouse_down(MouseButton::Left, stop)
                .child(self.field_el(
                    "find-search",
                    FindField::Search,
                    &q.search,
                    &s.find,
                    150.,
                    cx,
                ))
                .child(self.button_el("find-next", &s.next, |v, cx| v.find_next(false, cx), cx))
                .child(self.button_el("find-prev", &s.previous, |v, cx| v.find_next(true, cx), cx))
                .child(self.button_el("find-all", &s.all, Self::select_all_matches, cx))
                .child(self.checkbox_el("find-case", &s.match_case, q.case_sensitive, 0, cx))
                .child(self.checkbox_el("find-regexp", &s.regexp, q.regexp, 1, cx))
                .child(self.checkbox_el("find-word", &s.by_word, q.whole_word, 2, cx))
                .child(self.field_el(
                    "find-replace",
                    FindField::Replace,
                    &q.replace,
                    &s.replace,
                    150.,
                    cx,
                ))
                .child(self.button_el("find-replace-one", &s.replace_one, Self::replace_next, cx))
                .child(self.button_el("find-replace-all", &s.replace_all, Self::replace_all, cx))
                .child(
                    div()
                        .id("find-close")
                        .debug_selector(|| "find-close".into())
                        .absolute()
                        .top_0()
                        .right(px(4.))
                        .px(px(4.))
                        .text_size(px(14.))
                        .text_color(muted)
                        .hover(move |s| s.text_color(fg))
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.close_find(cx)))
                        .child("×"),
                )
                .into_any_element(),
        )
    }

    /// The go-to-line dialog (`.cm-panels-bottom .cm-dialog`), when open.
    pub(crate) fn render_goto(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let value = self.find.goto.clone()?;
        let st = &self.style;
        let s = self.options.strings.clone();
        let (muted, fg) = (st.muted_foreground, st.foreground);
        let stop =
            |_: &MouseDownEvent, _: &mut Window, cx: &mut gpui_kit::App| cx.stop_propagation();
        Some(
            div()
                .id("fuchico-editor-goto")
                .debug_selector(|| "fuchico-editor-goto".into())
                .relative()
                .flex()
                .flex_none()
                .items_center()
                // `.cm-dialog`: padding 2px 19px 4px 6px.
                .pt(px(2.))
                .pb(px(4.))
                .pl(px(6.))
                .pr(px(19.))
                .bg(st.secondary)
                .text_color(st.foreground)
                .border_t_1()
                .border_color(st.border)
                .font_family(st.sans_family.clone())
                .text_size(px(12.))
                .on_mouse_down(MouseButton::Left, stop)
                .child(
                    div()
                        .text_size(px(12. * 0.8))
                        .child(format!("{}: ", s.go_to_line)),
                )
                .child(self.field_el("goto-input", FindField::Goto, &value, "", 120., cx))
                .child(div().w(px(4.)))
                .child(self.button_el("goto-go", &s.go, Self::submit_goto, cx))
                .child(
                    div()
                        .id("goto-close")
                        .debug_selector(|| "goto-close".into())
                        .absolute()
                        .top(px(3.))
                        .right(px(4.))
                        .text_size(px(14.))
                        .text_color(muted)
                        .hover(move |s| s.text_color(fg))
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.close_goto(cx)))
                        .child("×"),
                )
                .into_any_element(),
        )
    }

    /// The outline overlay (`OutlineOverlay.tsx`): backdrop, input, fuzzy
    /// highlighted rows with `L<line>`, scrollable list, empty state.
    pub(crate) fn render_outline(&self, o: &OutlineState, cx: &mut Context<Self>) -> AnyElement {
        let st = &self.style;
        let s = &self.options.strings;
        let hl = HighlightStyle {
            background_color: Some(mix(st.syntax_number, 0.35)),
            ..Default::default()
        };
        let mut list = div()
            .id("fuchico-editor-outline-list")
            .debug_selector(|| "fuchico-editor-outline-list".into())
            .flex()
            .flex_col()
            .flex_shrink(1.)
            .min_h_0()
            .p(px(4.))
            .overflow_y_scroll();
        if o.filtered.is_empty() {
            list = list.child(
                div()
                    .debug_selector(|| "outline-empty".into())
                    .p(px(16.))
                    .flex()
                    .justify_center()
                    .text_size(px(12.))
                    .text_color(st.muted_foreground)
                    .child(s.no_headers.clone()),
            );
        }
        for (row, &i) in o.filtered.iter().enumerate() {
            let h = &o.items[i];
            let active = row == o.selected;
            let ranges = outline_highlights(&o.query, &h.text);
            let text: SharedString = h.text.clone().into();
            let label = StyledText::new(text).with_highlights(ranges.into_iter().map(|r| (r, hl)));
            let sel_name = format!("outline-item-{row}");
            let muted = if active {
                st.accent_foreground
            } else {
                st.muted_foreground
            };
            list = list.child(
                div()
                    .id(("fuchico-editor-outline-item", row))
                    .debug_selector(move || sel_name)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .w_full()
                    .py(px(6.))
                    .pr(px(10.))
                    .pl(px(10. + 14. * h.level.saturating_sub(1) as f32))
                    .rounded(st.radius_sm)
                    .text_size(px(12.))
                    .text_color(st.foreground)
                    .when(active, |d| d.bg(st.accent).text_color(st.accent_foreground))
                    .on_mouse_move(cx.listener(move |v, _: &MouseMoveEvent, _, cx| {
                        if let Overlay::Outline(o) = &mut v.overlay
                            && o.selected != row
                        {
                            o.selected = row;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.outline_pick(row, cx)))
                    .child({
                        let sel_name = format!("outline-hash-{row}");
                        div()
                            .debug_selector(move || sel_name)
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(st.muted_foreground)
                            .child("#")
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .child({
                        let sel_name = format!("outline-line-{row}");
                        div()
                            .debug_selector(move || sel_name)
                            .flex_none()
                            .text_size(px(10.))
                            .text_color(muted)
                            .child(format!("L{}", h.line))
                    }),
            );
        }
        let input = div()
            .flex()
            .flex_none()
            .items_center()
            .px(px(12.))
            .py(px(10.))
            .border_b_1()
            .border_color(st.border)
            .text_size(px(13.))
            .text_color(st.foreground)
            .child(div().w(px(1.)).h(px(15.)).bg(st.foreground))
            .child(if o.query.is_empty() {
                div()
                    .text_color(st.muted_foreground)
                    .child(s.go_to_header.clone())
            } else {
                div().child(o.query.clone())
            });
        let panel = div()
            .id("fuchico-editor-outline")
            .debug_selector(|| "fuchico-editor-outline".into())
            .w(px(440.))
            .max_w(relative(0.9))
            .max_h(relative(0.6))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(st.card)
            .text_color(st.card_foreground)
            .border_1()
            .border_color(st.border)
            .rounded(st.radius_md)
            .shadow(vec![gpui_kit::BoxShadow {
                color: gpui_kit::black().opacity(0.30),
                offset: gpui_kit::point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(input)
            .child(list);
        // `padding-top: 10%` resolves against the backdrop's width.
        let pad_top = 0.1 * self.viewport.0.max(0.);
        div()
            .id("fuchico-editor-outline-backdrop")
            .debug_selector(|| "outline-backdrop".into())
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(pad_top))
            .bg(gpui_kit::black().opacity(0.25))
            .font_family(st.sans_family.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|v, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    v.overlay = Overlay::None;
                    cx.notify();
                }),
            )
            .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
            .child(panel)
            .into_any_element()
    }
}

/// Byte ranges of the fuzzy-matched chars of `text` (one per char).
fn outline_highlights(query: &str, text: &str) -> Vec<Range<usize>> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let r = fuchico_core::fuzzy::fuzzy_match(query, text);
    if !r.matched {
        return Vec::new();
    }
    let starts: Vec<(usize, char)> = text.char_indices().collect();
    let mut idx = r.indices.clone();
    idx.sort_unstable();
    idx.dedup();
    idx.into_iter()
        .filter_map(|i| starts.get(i).map(|&(b, c)| b..b + c.len_utf8()))
        .collect()
}
