//! The editor state machine: key routing, transactions and effects.

use crate::change::{Change, ChangeSet, diff};
use crate::effect::{Effect, Scroll, ScrollAlign};
use crate::history::{Checkpoint, History};
use crate::host::{Config, Host, MemoryHost};
use crate::keymap::{self, Binding, CheckpointKind, Cmd, KeymapTable};
use crate::keys::{Key, KeyEvent, parse_keys};
use crate::mode::{Expecting, Minor, Mode, ModeType};
use crate::prompt::Prompt;
use crate::registers::Registers;
use crate::selection::{Range, Selection, cm_to_internal, internal_to_cm, map_sel};
use crate::text::Text;
use crate::typable::CommandInfo;

/// cm-helix `syntaxHistoryField` (expand / shrink selection history).
#[derive(Debug, Clone, Default)]
pub(crate) struct SyntaxHistory {
    pub selections: Vec<Selection>,
    pub cursor: usize,
    pub head: Option<Selection>,
    pub frozen: bool,
}

/// A CodeMirror-style transaction.
#[derive(Debug, Default)]
pub(crate) struct Tr {
    pub changes: Option<ChangeSet>,
    /// Selection in the coordinates of the document *after* `changes`.
    pub selection: Option<Selection>,
    pub mode: Option<Mode>,
    pub scroll: Option<ScrollAlign>,
    /// Carries a syntax-history effect (does not reset the history).
    pub syntax: bool,
}

impl Tr {
    pub fn sel(sel: Selection) -> Tr {
        Tr {
            selection: Some(sel),
            ..Tr::default()
        }
    }
    pub fn mode(mode: Mode) -> Tr {
        Tr {
            mode: Some(mode),
            ..Tr::default()
        }
    }
    pub fn changes(changes: ChangeSet) -> Tr {
        Tr {
            changes: Some(changes),
            ..Tr::default()
        }
    }
    pub fn with_mode(mut self, mode: Mode) -> Tr {
        self.mode = Some(mode);
        self
    }
    pub fn with_sel(mut self, sel: Selection) -> Tr {
        self.selection = Some(sel);
        self
    }
    pub fn scrolled(mut self) -> Tr {
        self.scroll = Some(ScrollAlign::Nearest);
        self
    }
    pub fn centered(mut self) -> Tr {
        self.scroll = Some(ScrollAlign::Center);
        self
    }
}

struct Snapshot {
    sel: Selection,
    mode: Mode,
    status: String,
}

/// A Helix-style modal editor over one document.
///
/// Feed it keys with [`Editor::handle_key`] (or text with
/// [`Editor::insert_text`]) and apply the returned [`Effect`]s to the view.
pub struct Editor {
    pub(crate) doc: Text,
    pub(crate) sel: Selection,
    pub(crate) mode: Mode,
    pub(crate) registers: Registers,
    pub(crate) history: History,
    pub(crate) syntax: SyntaxHistory,
    pub(crate) config: Config,
    pub(crate) host: Box<dyn Host>,
    pub(crate) prompt: Option<Prompt>,
    pub(crate) host_commands: Vec<CommandInfo>,
    pub(crate) theme: Option<String>,
    effects: Vec<Effect>,
    edits: Vec<Effect>,
    pub(crate) dispatched: bool,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("doc", &self.doc.to_string())
            .field("sel", &self.sel)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
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
        let doc = Text::new(text);
        // cm-helix starts with a block cursor on the first char.
        let sel = if doc.is_empty() {
            Selection::single(0, 0)
        } else {
            Selection::single(1, 0)
        };
        let theme = config
            .theme
            .clone()
            .or_else(|| config.themes.first().cloned());
        Editor {
            doc,
            sel,
            mode: Mode::NORMAL,
            registers: Registers::new(),
            history: History::default(),
            syntax: SyntaxHistory::default(),
            config,
            host,
            prompt: None,
            host_commands: Vec::new(),
            theme,
            effects: Vec::new(),
            edits: Vec::new(),
            dispatched: false,
        }
    }

    // ---- accessors --------------------------------------------------------

    pub fn doc(&self) -> &Text {
        &self.doc
    }
    pub fn text(&self) -> String {
        self.doc.to_string()
    }
    pub fn selection(&self) -> &Selection {
        &self.sel
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    /// `NOR` / `INS` / `SEL`.
    pub fn mode_label(&self) -> &'static str {
        self.mode.ty.label()
    }
    /// Pending keys for the status line (`"3"`, `"g"`, `"mi"`, `"<space>"`).
    pub fn pending_status(&self) -> String {
        self.mode.status()
    }
    /// Register selected with `"x`, if any.
    pub fn pending_register(&self) -> Option<char> {
        self.mode.register
    }
    /// cm-helix `lineCol`: 1-based line and column of the main head.
    pub fn line_col(&self) -> (usize, usize) {
        let head = self.sel.main().head;
        let line = self.doc.line_at(head);
        (line.number, head - line.from + 1)
    }
    /// Char offsets of the block cursors to draw (the helix "internal" head
    /// of every range). In insert mode these are the CodeMirror heads.
    pub fn block_cursors(&self) -> Vec<usize> {
        self.sel
            .ranges()
            .iter()
            .map(|r| {
                if self.mode.ty == ModeType::Insert {
                    r.head
                } else {
                    cm_to_internal(*r, &self.doc).head
                }
            })
            .collect()
    }
    pub fn registers(&self) -> &Registers {
        &self.registers
    }
    /// Share registers between editors (cm-helix `globalInit`).
    pub fn set_registers(&mut self, registers: Registers) {
        self.registers = registers;
    }
    pub fn history(&self) -> &History {
        &self.history
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
    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }

    /// Replace the selection (e.g. from the host's own UI).
    pub fn set_selection(&mut self, sel: Selection) -> Vec<Effect> {
        let snap = self.snapshot();
        self.dispatch(Tr::sel(sel));
        self.finish(snap)
    }

    /// A selection made with the mouse: in normal/select mode an empty main
    /// range becomes a block cursor (cm-helix `selectByClickFilter`).
    pub fn set_selection_from_pointer(&mut self, sel: Selection) -> Vec<Effect> {
        let snap = self.snapshot();
        let sel = sel.clamp(self.doc.len());
        let sel = if self.mode.ty != ModeType::Insert && sel.main().is_empty() {
            let mut ranges = sel.ranges().to_vec();
            let i = sel.main_index();
            ranges[i] = internal_to_cm(ranges[i], &self.doc);
            Selection::create(ranges, i)
        } else {
            sel
        };
        self.dispatch(Tr::sel(sel));
        self.finish(snap)
    }

    /// Replace the whole document (external reload). Not recorded in undo
    /// history; the selection is mapped through the diff.
    pub fn set_text(&mut self, text: &str) -> Vec<Effect> {
        let snap = self.snapshot();
        let new = Text::new(text);
        let changes = diff(&self.doc, &new);
        self.dispatch(Tr::changes(changes));
        self.finish(snap)
    }

    /// Apply an edit made by the host (not recorded in undo history).
    pub fn apply_changes(&mut self, changes: Vec<Change>) -> Vec<Effect> {
        let snap = self.snapshot();
        self.dispatch(Tr::changes(ChangeSet::new(changes)));
        self.finish(snap)
    }

    // ---- input ---------------------------------------------------------------

    /// Handle one key press.
    pub fn handle_key(&mut self, ev: KeyEvent) -> Vec<Effect> {
        let snap = self.snapshot();
        self.key(&ev);
        self.finish(snap)
    }

    /// Convenience: feed a key sequence in [`parse_keys`] notation.
    pub fn handle_keys(&mut self, keys: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        for ev in parse_keys(keys) {
            out.extend(self.handle_key(ev));
        }
        out
    }

    /// Text input (IME commit, paste into the view, dead keys). In insert
    /// mode it replaces every selection; in other modes each char is
    /// treated as a typed key.
    pub fn insert_text(&mut self, text: &str) -> Vec<Effect> {
        if self.mode.ty == ModeType::Insert && self.mode.expecting.is_none() {
            let snap = self.snapshot();
            self.replace_selection(text);
            return self.finish(snap);
        }
        let mut out = Vec::new();
        for c in text.chars() {
            out.extend(self.handle_key(KeyEvent::char(c)));
        }
        out
    }

    fn key(&mut self, ev: &KeyEvent) {
        let name = ev.name();
        let ext = self.config.extensions;
        if self.mode.ty == ModeType::Insert {
            if let Some(b) = keymap::lookup(KeymapTable::Insert, &name, ext) {
                self.exec(b.cmd);
                return;
            }
            if let Some(c) = ev.plain_char() {
                if let Some(exp) = self.mode.expecting {
                    self.expect(exp, c);
                } else {
                    self.replace_selection(&c.to_string());
                }
                return;
            }
            self.effects.push(Effect::Unhandled);
            return;
        }

        if let (Some(exp), Some(c)) = (self.mode.expecting, ev.plain_char()) {
            self.expect(exp, c);
            return;
        }
        if ev.key == Key::Escape && ev.modifiers.is_none() {
            let b = keymap::lookup(KeymapTable::Normal, "Escape", ext).expect("escape binding");
            self.run_binding(b);
            return;
        }
        let table = match self.mode.minor.unwrap_or(Minor::Normal) {
            Minor::Normal => KeymapTable::Normal,
            Minor::Goto => KeymapTable::Goto,
            Minor::Match => KeymapTable::Match,
            Minor::Space => KeymapTable::Space,
            Minor::LeftBracket => KeymapTable::LeftBracket,
            Minor::RightBracket => KeymapTable::RightBracket,
            Minor::View => KeymapTable::View,
        };
        match keymap::lookup(table, &name, ext) {
            Some(b) => self.run_binding(b),
            None => {
                if ev.is_char() {
                    // cm-helix `unhandledCommandsFilter`: typing is swallowed;
                    // in a minor mode it cancels the pending prefix.
                    if table != KeymapTable::Normal {
                        let m = Mode::reset_for(self.mode.ty);
                        self.dispatch(Tr::mode(m));
                    }
                } else {
                    self.effects.push(Effect::Unhandled);
                }
            }
        }
    }

    fn run_binding(&mut self, b: &'static Binding) {
        let before = Checkpoint {
            doc: self.doc.clone(),
            selection: self.sel.clone(),
        };
        self.dispatched = false;
        self.exec(b.cmd);
        if self.dispatched {
            match b.checkpoint {
                CheckpointKind::None => {}
                CheckpointKind::Add => self.history.add(before, false),
                CheckpointKind::Temp => self.history.add(before, true),
            }
        }
    }

    // ---- transactions --------------------------------------------------------

    pub(crate) fn dispatch(&mut self, tr: Tr) {
        self.dispatched = true;
        match tr.changes.filter(|c| !c.is_empty()) {
            Some(ch) => {
                self.edits.push(Effect::DocChanged(ch.describe(&self.doc)));
                ch.apply(&mut self.doc);
                let sel = match tr.selection {
                    Some(s) => s,
                    None => self.sel.map(&ch, -1),
                };
                self.sel = sel.clamp(self.doc.len());
            }
            None => {
                if let Some(s) = tr.selection {
                    self.sel = s.clamp(self.doc.len());
                }
            }
        }
        if !tr.syntax && !self.syntax.selections.is_empty() && !self.syntax.frozen {
            self.syntax = SyntaxHistory::default();
        }
        if let Some(m) = tr.mode {
            self.mode = m;
        }
        if let Some(align) = tr.scroll {
            self.effects.push(Effect::Scroll(Scroll::To {
                pos: self.sel.main().head,
                align,
            }));
        }
    }

    /// CodeMirror `state.changeByRange`: `f` returns the changes for one
    /// range (in original document coordinates) and the resulting range in
    /// the coordinates of the original document with *only its own* changes
    /// applied.
    pub(crate) fn change_by_range(
        &self,
        mut f: impl FnMut(&Self, Range) -> (Vec<Change>, Range),
    ) -> Tr {
        let results: Vec<(Vec<Change>, Range)> =
            self.sel.ranges().iter().map(|r| f(self, *r)).collect();
        let mut ranges = Vec::with_capacity(results.len());
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
            let mapped = if others.is_empty() {
                *range
            } else {
                range.map(&ChangeSet::new(others), -1)
            };
            ranges.push(mapped);
        }
        let all: Vec<Change> = results.into_iter().flat_map(|(c, _)| c).collect();
        Tr {
            changes: Some(ChangeSet::new(all)),
            selection: Some(Selection::create(ranges, self.sel.main_index())),
            ..Tr::default()
        }
    }

    /// `state.replaceSelection(text)`.
    pub(crate) fn replace_selection(&mut self, text: &str) {
        let len = crate::text::char_len(text);
        let tr = self.change_by_range(|_, r| {
            (
                vec![Change::new(r.from(), r.to(), text)],
                Range::cursor(r.from() + len),
            )
        });
        self.dispatch(tr.scrolled());
    }

    pub(crate) fn emit(&mut self, e: Effect) {
        self.effects.push(e);
    }
    pub(crate) fn message(&mut self, text: impl Into<String>) {
        self.effects.push(Effect::Message {
            text: text.into(),
            error: false,
        });
    }
    pub(crate) fn error(&mut self, text: impl Into<String>) {
        self.effects.push(Effect::Message {
            text: text.into(),
            error: true,
        });
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            sel: self.sel.clone(),
            mode: self.mode,
            status: self.mode.status(),
        }
    }

    fn finish(&mut self, snap: Snapshot) -> Vec<Effect> {
        let mut out: Vec<Effect> = std::mem::take(&mut self.edits);
        if !snap.sel.same(&self.sel) {
            out.push(Effect::SelectionChanged);
        }
        if snap.mode.ty != self.mode.ty {
            out.push(Effect::ModeChanged {
                from: snap.mode.ty,
                to: self.mode.ty,
            });
        }
        if snap.status != self.mode.status() || snap.mode.register != self.mode.register {
            out.push(Effect::StatusChanged);
        }
        out.append(&mut self.effects);
        out
    }

    /// Run `f` as one input step and collect its effects (used by the prompt
    /// API).
    pub(crate) fn step(&mut self, f: impl FnOnce(&mut Self)) -> Vec<Effect> {
        let snap = self.snapshot();
        f(self);
        self.finish(snap)
    }

    // ---- shared helpers --------------------------------------------------------

    pub(crate) fn internal(&self, r: Range) -> Range {
        cm_to_internal(r, &self.doc)
    }
    pub(crate) fn to_cm(&self, r: Range) -> Range {
        internal_to_cm(r, &self.doc)
    }
    pub(crate) fn is_select(&self) -> bool {
        self.mode.ty == ModeType::Select
    }
    /// `MODE_EFF.NORMAL` / `MODE_EFF.SELECT` by the current type.
    pub(crate) fn reset_mode(&self) -> Mode {
        Mode::reset_for(self.mode.ty)
    }
    pub(crate) fn map_sel(&self, f: impl FnMut(Range) -> Range) -> Selection {
        map_sel(&self.sel, f)
    }
    pub(crate) fn range_text(&self, r: Range) -> String {
        self.doc.slice(r.from(), r.to())
    }
    pub(crate) fn selection_texts(&self) -> Vec<String> {
        self.sel
            .ranges()
            .iter()
            .map(|r| self.range_text(*r))
            .collect()
    }

    /// cm-helix `readRegister` (`None` = register unset).
    pub(crate) fn read_register(
        &mut self,
        reg: Option<char>,
    ) -> Result<Option<Vec<String>>, String> {
        let reg = reg.unwrap_or(self.config.default_register);
        Ok(match reg {
            '+' => {
                let yanked = self.registers.get('+').cloned();
                let copied = self.host.clipboard_read()?;
                if yanked.as_ref().map(|y| y.join("\n")).as_deref() == Some(copied.as_str()) {
                    yanked
                } else {
                    Some(vec![copied])
                }
            }
            '#' => Some((1..=self.sel.len()).map(|i| i.to_string()).collect()),
            '_' => Some(Vec::new()),
            '.' => Some(self.selection_texts()),
            '%' => Some(self.host.path().into_iter().collect()),
            r => self.registers.get(r).cloned(),
        })
    }

    /// `yankEffect`.
    pub(crate) fn yank_to(&mut self, reg: char, value: Vec<String>) {
        if let Some(clip) = self.registers.write(reg, value) {
            self.host.clipboard_write(&clip);
        }
    }

    pub(crate) fn expect(&mut self, exp: Expecting, c: char) {
        match exp {
            Expecting::Find { inclusive, forward } => self.find_text(c, inclusive, forward),
            Expecting::Replace => self.replace_with_char(c),
            Expecting::Surround => self.surround(c),
            Expecting::Delimiters { inclusive } => self.extend_to_delimiters(c, inclusive),
            Expecting::Register => {
                let m = Mode {
                    expecting: None,
                    register: Some(c),
                    ..self.mode
                };
                self.dispatch(Tr::mode(m));
            }
            Expecting::InsertRegister => match self.read_register(Some(c)) {
                Ok(yanked) => {
                    self.paste(yanked, false, 1, false, false);
                    self.dispatch(Tr::mode(Mode::INSERT));
                }
                Err(e) => self.error(e),
            },
            Expecting::DeleteSurround => self.delete_surround(c),
            Expecting::ReplaceSurround(None) => {
                let m = Mode {
                    expecting: Some(Expecting::ReplaceSurround(Some(c))),
                    ..self.mode
                };
                self.dispatch(Tr::mode(m));
            }
            Expecting::ReplaceSurround(Some(from)) => self.replace_surround(from, c),
        }
    }

    pub(crate) fn exec(&mut self, cmd: Cmd) {
        crate::commands::exec(self, cmd);
    }
}
