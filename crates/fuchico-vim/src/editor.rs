//! The public [`Editor`]: document, selection, Vim state and effects.

use std::collections::HashMap;

use fuchico_helix::change::diff;
use fuchico_helix::{
    Change, ChangeSet, Config, Effect, Host, KeyEvent, MemoryHost, ModeType, Scroll, ScrollAlign,
    Selection, Text, parse_keys,
};

use crate::history::UndoHistory;
use crate::prompt::Prompt;
use crate::search::Query;
use crate::state::{VimGlobals, VimState};

/// A bookmark (`cm.setBookmark`): a char offset mapped through changes
/// with `MapMode.TrackDel` (`None` once deleted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Marker {
    pub offset: Option<usize>,
    pub assoc: i32,
    /// Line handles are deleted when their line start is (`TrackAfter`).
    pub line_handle: bool,
}

/// `cm.curOp`.
#[derive(Debug, Clone, Default)]
pub(crate) struct CurOp {
    pub depth: i64,
    pub is_vim_op: bool,
    /// Inserted texts of every change (`op.change` linked list).
    pub changes: Vec<String>,
    /// `op.changeHandlers` snapshot: `None` = not taken yet, `Some(b)` =
    /// whether vim's insert-mode change listener was attached.
    pub change_handlers: Option<bool>,
    pub cursor_activity: bool,
    pub change_start: Option<usize>,
}

/// A Vim modal editor (port of `@replit/codemirror-vim` 6.3.0).
pub struct Editor {
    pub(crate) doc: Text,
    pub(crate) sel: Selection,
    pub(crate) vim: VimState,
    pub(crate) g: VimGlobals,
    pub(crate) config: Config,
    pub(crate) host: Box<dyn Host>,
    pub(crate) markers: HashMap<usize, Marker>,
    pub(crate) next_marker: usize,
    pub(crate) cur_op: Option<CurOp>,
    pub(crate) last_change_end_offset: usize,
    /// `cm.state.overwrite` (replace mode).
    pub(crate) overwrite: bool,
    /// vim's insert-mode `change` listener is attached / was ever attached.
    pub(crate) change_listener: bool,
    pub(crate) change_listener_ever: bool,
    pub(crate) history: UndoHistory,
    pub(crate) prompt: Option<Prompt>,
    /// A prompt opened while a mapping / `:normal` runs (`virtualPrompt`).
    pub(crate) virtual_prompt: Option<Prompt>,
    pub(crate) key_to_key_stack: Vec<Option<String>>,
    pub(crate) noremap: bool,
    /// The search highlight overlay (`cm6Query` with `forVim`).
    pub(crate) highlight: Option<Query>,
    pub(crate) effects: Vec<Effect>,
    pub(crate) edits: Vec<Effect>,
    pub(crate) scroll: Option<Scroll>,
    pub(crate) want_scroll_into_view: bool,
    /// Last `showConfirm` / status message.
    pub(crate) message: Option<(String, bool)>,
    /// `vimGlobalState.isReplayingLastEdit`-style flag set by `.`.
    pub(crate) replaying_last_edit: bool,
    /// `<C-o>` ran: return to insert mode after the next command.
    pub(crate) one_normal_pending: bool,
    pub(crate) epoch: std::time::Instant,
    /// Added to the wall clock (tests advance time with it).
    pub(crate) clock_offset: f64,
    /// A notification is open (the next key closes it).
    pub(crate) notification_open: bool,
    /// Time of the last partial insert-mode key match (`<Esc>` mappings
    /// like `jk` time out after `insertModeEscKeysTimeout`).
    pub(crate) last_partial_ms: Option<f64>,
    /// Running `:global` chains.
    pub(crate) ex_chains: HashMap<usize, crate::ex::GlobalChain>,
    pub(crate) next_chain: usize,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("doc", &self.doc.to_string())
            .field("sel", &self.sel)
            .field("mode", &self.vim.mode)
            .finish_non_exhaustive()
    }
}

/// What the status-bar / cursor rendering depends on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snapshot {
    sel: Selection,
    mode: ModeType,
    label: &'static str,
    status: String,
    prompt: bool,
}

impl Editor {
    /// An editor with the default [`Config`] and a [`MemoryHost`].
    pub fn new(text: &str) -> Self {
        Self::with_host(text, Config::default(), Box::new(MemoryHost::default()))
    }

    pub fn with_config(text: &str, config: Config) -> Self {
        Self::with_host(text, config, Box::new(MemoryHost::default()))
    }

    pub fn with_host(text: &str, config: Config, host: Box<dyn Host>) -> Self {
        Editor {
            doc: Text::new(text),
            sel: Selection::single(0, 0),
            vim: VimState::new(),
            g: VimGlobals::default(),
            config,
            host,
            markers: HashMap::new(),
            next_marker: 0,
            cur_op: None,
            last_change_end_offset: 0,
            overwrite: false,
            change_listener: false,
            change_listener_ever: false,
            history: UndoHistory::default(),
            prompt: None,
            virtual_prompt: None,
            key_to_key_stack: Vec::new(),
            noremap: false,
            highlight: None,
            effects: Vec::new(),
            edits: Vec::new(),
            scroll: None,
            want_scroll_into_view: false,
            message: None,
            replaying_last_edit: false,
            one_normal_pending: false,
            epoch: std::time::Instant::now(),
            clock_offset: 0.0,
            notification_open: false,
            last_partial_ms: None,
            ex_chains: HashMap::new(),
            next_chain: 0,
        }
    }

    // ---- accessors ----------------------------------------------------------

    pub fn doc(&self) -> &Text {
        &self.doc
    }
    pub fn text(&self) -> String {
        self.doc.to_string()
    }
    /// The CodeMirror selection. In normal mode it is an empty cursor; in
    /// visual mode it covers the highlighted text (the head is one past the
    /// block cursor when the selection is forward).
    pub fn selection(&self) -> &Selection {
        &self.sel
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }
    pub fn host_mut(&mut self) -> &mut dyn Host {
        self.host.as_mut()
    }

    /// Insert / Select (any visual mode) / Normal.
    pub fn mode_type(&self) -> ModeType {
        if self.vim.insert_mode {
            ModeType::Insert
        } else if self.vim.visual_mode {
            ModeType::Select
        } else {
            ModeType::Normal
        }
    }

    /// `NORMAL`, `INSERT`, `REPLACE`, `VISUAL`, `VISUAL LINE`,
    /// `VISUAL BLOCK` (cm-vim's `--MODE--` without dashes).
    pub fn mode_label(&self) -> &'static str {
        if self.vim.insert_mode {
            if self.overwrite { "REPLACE" } else { "INSERT" }
        } else if self.vim.visual_mode {
            if self.vim.visual_block {
                "VISUAL BLOCK"
            } else if self.vim.visual_line {
                "VISUAL LINE"
            } else {
                "VISUAL"
            }
        } else {
            "NORMAL"
        }
    }

    /// `(C-O)` is shown after the mode while `<C-o>` runs one normal
    /// command from insert mode.
    pub fn insert_mode_return(&self) -> bool {
        self.vim.insert_mode_return
    }

    /// Keys typed so far for the pending command (`vim.status`).
    pub fn pending_status(&self) -> String {
        self.vim.status.clone()
    }

    /// The register being recorded (`recording @q`), if any.
    pub fn recording(&self) -> Option<char> {
        let m = &self.g.macro_mode_state;
        if m.is_recording { m.latest_register } else { None }
    }

    /// 1-based line and column of the main head.
    pub fn line_col(&self) -> (usize, usize) {
        let head = self.sel.main().head;
        let line = self.doc.line_at(head);
        (line.number, head - line.from + 1)
    }

    /// Char offsets of the block cursors to draw (cm-vim's block cursor
    /// plugin): the head, or `head - 1` for a forward non-empty range.
    /// Insert mode (not replace) draws bar cursors at the heads.
    pub fn block_cursors(&self) -> Vec<usize> {
        let ranges: Vec<_> = if self.vim.visual_block {
            vec![self.sel.main()]
        } else {
            self.sel.ranges().to_vec()
        };
        ranges
            .iter()
            .map(|r| {
                if !self.vim.insert_mode && r.anchor < r.head {
                    // the head sits after the last selected char
                    r.head - 1
                } else {
                    r.head
                }
            })
            .collect()
    }

    /// The cursor shape cm-vim draws.
    pub fn cursor_shape(&self) -> CursorShape {
        if self.vim.insert_mode {
            if self.overwrite { CursorShape::Underline } else { CursorShape::Bar }
        } else if !self.vim.input_state.key_buffer.is_empty()
            || self.vim.input_state.operator.is_some()
            || !self.vim.status.is_empty()
        {
            CursorShape::Half
        } else {
            CursorShape::Block
        }
    }

    /// Ranges to paint as search matches (`hlsearch` overlay), in the
    /// visible range `from..to` (char offsets).
    pub fn search_highlight(&self, from: usize, to: usize) -> Vec<(usize, usize)> {
        match &self.highlight {
            Some(q) => q.matches(&self.doc, from.min(self.doc.len()), to.min(self.doc.len())),
            None => Vec::new(),
        }
    }

    /// The last message shown with `showConfirm` (text, is_error).
    pub fn message(&self) -> Option<(&str, bool)> {
        self.message.as_ref().map(|(t, e)| (t.as_str(), *e))
    }

    /// Global Vim state (registers, search, macros, mappings, history).
    pub fn globals(&self) -> &VimGlobals {
        &self.g
    }
    pub fn set_globals(&mut self, g: VimGlobals) {
        self.g = g;
    }
    /// Mutable access to the globals (hosts sharing one [`VimGlobals`]
    /// between editors swap it in and out with `std::mem::swap`, which,
    /// unlike [`Editor::take_globals`], builds no default keymap).
    pub fn globals_mut(&mut self) -> &mut VimGlobals {
        &mut self.g
    }
    /// Move the globals out (to hand them to another editor).
    pub fn take_globals(&mut self) -> VimGlobals {
        std::mem::take(&mut self.g)
    }

    /// Text of a register (`"a`, `"0`, `"+` …), as `p` would paste it.
    pub fn register(&self, name: char) -> Option<String> {
        let c = name.to_lowercase().next()?;
        self.g.register_controller.registers.get(&c).map(|r| r.text())
    }

    // ---- host entry points ----------------------------------------------------

    /// Replace the selection from the host (a non-vim operation: cm-vim
    /// treats it as an external selection and may enter/leave visual mode).
    pub fn set_selection(&mut self, sel: Selection) -> Vec<Effect> {
        let snap = self.snapshot();
        let sel = sel.clamp(self.doc.len());
        self.dispatch_tr(None, Some(sel), Some("select"));
        self.finish(snap)
    }

    /// A mouse selection. Same as [`Editor::set_selection`] (cm-vim enters
    /// visual mode on a non-empty selection, `handleExternalSelection`).
    pub fn set_selection_from_pointer(&mut self, sel: Selection) -> Vec<Effect> {
        self.set_selection(sel)
    }

    /// Replace the whole document (external reload): clears undo history.
    pub fn set_text(&mut self, text: &str) -> Vec<Effect> {
        let snap = self.snapshot();
        let new = Text::new(text);
        let changes = diff(&self.doc, &new);
        self.history.suspend = true;
        self.dispatch_tr(Some(changes), None, None);
        self.history.suspend = false;
        self.history.clear();
        self.finish(snap)
    }

    /// Apply an edit made by the host (autocomplete, close brackets, paste
    /// handlers …). Recorded in undo history and, in insert mode, in the
    /// insert-mode change log used by `.`.
    pub fn apply_changes(&mut self, changes: Vec<Change>) -> Vec<Effect> {
        let snap = self.snapshot();
        self.dispatch_tr(Some(ChangeSet::new(changes)), None, Some("input"));
        self.finish(snap)
    }

    /// Apply an edit and a new selection made by the host in one step.
    pub fn apply_transaction(&mut self, changes: Vec<Change>, sel: Selection) -> Vec<Effect> {
        let snap = self.snapshot();
        self.dispatch_tr(Some(ChangeSet::new(changes)), Some(sel), Some("input"));
        self.finish(snap)
    }

    /// Handle one key press.
    pub fn handle_key(&mut self, ev: KeyEvent) -> Vec<Effect> {
        let snap = self.snapshot();
        self.plugin_handle_key(&ev);
        self.finish(snap)
    }

    /// Feed a key sequence in [`parse_keys`] notation (`"3dw"`, `"<Esc>"`,
    /// `"<C-r>"`, `"<CR>"`).
    pub fn handle_keys(&mut self, keys: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        for ev in parse_keys(&normalize_key_names(keys)) {
            out.extend(self.handle_key(ev));
        }
        out
    }

    /// Text input (typing, IME commit, paste). In insert mode it replaces
    /// the selection (overwriting in replace mode); in other modes every
    /// char is handled as a key.
    pub fn insert_text(&mut self, text: &str) -> Vec<Effect> {
        if self.vim.insert_mode
            && self.vim.input_state.key_buffer.is_empty()
            && self.prompt.is_none()
        {
            let snap = self.snapshot();
            self.type_text(text);
            return self.finish(snap);
        }
        let mut out = Vec::new();
        for c in text.chars() {
            out.extend(self.handle_key(KeyEvent::char(c)));
        }
        out
    }

    /// CodeMirror's `undo` / `redo` commands (the host's Mod-z /
    /// Mod-Shift-z): one history step, without leaving the current mode.
    pub fn history_command(&mut self, redo: bool) -> Vec<Effect> {
        let snap = self.snapshot();
        self.operation(false, |ed| ed.run_history_command(redo));
        self.finish(snap)
    }

    /// Advance the editor's clock (tests: insert-mode key timeouts and
    /// undo grouping).
    #[doc(hidden)]
    pub fn advance_clock(&mut self, ms: f64) {
        self.clock_offset += ms;
    }

    // ---- effects ----------------------------------------------------------------

    pub(crate) fn snapshot(&mut self) -> Snapshot {
        self.effects.clear();
        self.edits.clear();
        self.scroll = None;
        self.want_scroll_into_view = false;
        Snapshot {
            sel: self.sel.clone(),
            mode: self.mode_type(),
            label: self.mode_label(),
            status: self.vim.status.clone(),
            prompt: self.prompt.is_some(),
        }
    }

    pub(crate) fn finish(&mut self, snap: Snapshot) -> Vec<Effect> {
        // flush "+ writes
        let writes = std::mem::take(&mut self.g.register_controller.clipboard_writes);
        for w in writes {
            self.host.clipboard_write(&w);
        }
        let mut out = std::mem::take(&mut self.edits);
        if !self.sel.same(&snap.sel) {
            out.push(Effect::SelectionChanged);
        }
        let mode = self.mode_type();
        if mode != snap.mode {
            out.push(Effect::ModeChanged {
                from: snap.mode,
                to: mode,
            });
        }
        if self.mode_label() != snap.label
            || self.vim.status != snap.status
            || self.prompt.is_some() != snap.prompt
        {
            out.push(Effect::StatusChanged);
        }
        if let Some(s) = self.scroll.take() {
            out.push(Effect::Scroll(s));
        } else if self.want_scroll_into_view || !out.is_empty() {
            let doc_or_sel = out
                .iter()
                .any(|e| matches!(e, Effect::DocChanged(_) | Effect::SelectionChanged));
            if doc_or_sel || self.want_scroll_into_view {
                out.push(Effect::Scroll(Scroll::To {
                    pos: self.sel.main().head,
                    align: ScrollAlign::Nearest,
                }));
            }
        }
        out.append(&mut self.effects);
        out
    }

    pub(crate) fn emit(&mut self, e: Effect) {
        self.effects.push(e);
    }

    /// `showConfirm` / `cm.openNotification`.
    pub(crate) fn show_confirm(&mut self, text: &str) {
        self.message = Some((text.to_string(), false));
        self.effects.push(Effect::Message {
            text: text.to_string(),
            error: false,
        });
    }

    pub(crate) fn show_error(&mut self, text: &str) {
        self.message = Some((text.to_string(), true));
        self.effects.push(Effect::Message {
            text: text.to_string(),
            error: true,
        });
    }
}

/// Cursor shapes cm-vim draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    /// Insert mode.
    Bar,
    /// Replace mode (`R`).
    Underline,
    /// A command is pending (`d`, `2`, `"a` …).
    Half,
}

/// Accept cm-vim key names in [`Editor::handle_keys`]: `<CR>`, `<BS>`,
/// `<Del>`, `<Ins>` and `<C-x>` style names are mapped to helix
/// `parse_keys` names.
fn normalize_key_names(keys: &str) -> String {
    let mut out = String::new();
    for k in crate::keymap::split_keys(keys) {
        if k.len() > 2 && k.starts_with('<') && k.ends_with('>') {
            let inner = &k[1..k.len() - 1];
            let (mods, base) = match inner.rfind('-') {
                Some(i) if i + 1 < inner.len() => (&inner[..=i], &inner[i + 1..]),
                _ => ("", inner),
            };
            let base = match base.to_ascii_lowercase().as_str() {
                "ins" | "insert" => "Insert".to_string(),
                "pageup" => "PageUp".to_string(),
                "pagedown" => "PageDown".to_string(),
                _ => base.to_string(),
            };
            out.push('<');
            out.push_str(mods);
            out.push_str(&base);
            out.push('>');
        } else {
            out.push_str(&k);
        }
    }
    out
}
