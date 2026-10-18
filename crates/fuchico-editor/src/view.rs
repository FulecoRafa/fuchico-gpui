//! [`EditorView`]: the editor entity (state, input routing, host API and
//! overlays). Layout and painting of the text live in `element.rs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use chrono::Local;
use fuchico_core::fuzzy::fuzzy_match;
use fuchico_core::outline::OutlineHeader;
use fuchico_helix::{
    Change, Completions, Config, Editor, Effect, External, Key, KeyEvent, ModeType,
    Modifiers as HxMods, PromptKind, Range as HxRange, Scroll, ScrollAlign, Selection,
    TextEdit as HxEdit,
};
use fuchico_markdown::completion::should_open_completion;
use fuchico_markdown::text::{Assoc, map_pos};
use fuchico_markdown::{
    CompletionContext, CompletionOption, FenceToolbar, FoldState, MarkdownModel, Settings,
    TableData, TableEditState, TableKey, TableModel, TableOutcome, TextEdit as MdEdit,
};
use gpui_kit::{
    App, ClipboardEntry, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollWheelEvent,
    SharedString, Styled, Subscription, Window, div, px,
};

use crate::actions::*;
use crate::commands::{self, CommandEdit, TokenKind};
use crate::element::{EditorElement, Frame, Hit};
use crate::host::{SharedLayout, ViewHost};
use crate::style::{EditorStyle, mix};

// ---- public types -----------------------------------------------------------

/// Gutter line numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineNumbers {
    #[default]
    Absolute,
    /// Distance to the cursor line (the cursor line shows its number).
    Relative,
    Off,
}

/// Cursor shape in insert mode (normal/select always draw a block).
/// cm-helix draws a block in insert mode too unless configured otherwise
/// (the original does not override it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InsertCursor {
    Bar,
    #[default]
    Block,
}

/// Which key engine drives the view.
///
/// Seam for further engines (a Vim engine plugs in here as a new variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineKind {
    /// Modal Helix keys (fuchico-helix).
    #[default]
    Helix,
    /// Plain non-modal editing (CodeMirror defaults): always inserting,
    /// shift-arrows extend the selection, Escape does nothing.
    Normal,
}

/// View options (host settings).
#[derive(Debug, Clone, PartialEq)]
pub struct EditorOptions {
    pub line_numbers: LineNumbers,
    /// Soft wrap at the viewport width (the original had none: default off).
    pub soft_wrap: bool,
    /// Ruler columns (`Settings → Rulers`).
    pub rulers: Vec<u32>,
    pub insert_cursor: InsertCursor,
    /// Vault paths (wikilink resolution + completion).
    pub vault_files: Vec<String>,
    pub all_tags: Vec<String>,
    /// Vault-relative path of the file (for `:write` messages and dropped
    /// path links).
    pub path: Option<String>,
    /// `:` asks the host to open its command palette
    /// ([`EditorEvent::OpenCommandLine`]) instead of the inline bar.
    pub command_line_via_host: bool,
    pub fold_start_marker: String,
    pub fold_end_marker: String,
    /// Draw image embeds (`![](x.png)`, `![[x.png]]`) below their line.
    /// Not in the original (which shows them as text): off by default.
    pub inline_images: bool,
    /// Absolute vault directory; image paths resolve against it (and the
    /// note's directory, from `path`).
    pub vault_root: Option<std::path::PathBuf>,
    /// Draw the mode chip (bottom-left).
    pub mode_chip: bool,
    /// Helix "external" commands the host implements (`Space f`, `Space b`,
    /// `gn`/`gp`, `:bc`, `Space /`); enabled ones arrive as
    /// [`EditorEvent::External`]. All off by default, like cm-helix.
    pub externals: fuchico_helix::ExternalCommands,
    /// Indent width in spaces (`Settings → Tab size`).
    pub tab_size: u32,
    pub engine: EngineKind,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions {
            line_numbers: LineNumbers::Absolute,
            soft_wrap: false,
            rulers: Vec::new(),
            insert_cursor: InsertCursor::Block,
            vault_files: Vec::new(),
            all_tags: Vec::new(),
            path: None,
            command_line_via_host: false,
            fold_start_marker: ":::fold".into(),
            fold_end_marker: ":::endfold".into(),
            inline_images: false,
            vault_root: None,
            mode_chip: true,
            externals: Default::default(),
            tab_size: 2,
            engine: EngineKind::Helix,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDirection {
    In,
    Out,
    Reset,
}

/// A mermaid fence the host shows a preview for. `key` is stable for the
/// block's lifetime (the fence moves with edits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidBlock {
    pub key: u64,
    pub source: String,
}

/// Everything the host reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorEvent {
    /// The text changed (`dirty` = differs from the last saved text).
    Changed {
        dirty: bool,
    },
    /// The dirty flag flipped.
    DirtyChanged(bool),
    /// `:w` / Mod-s.
    Save,
    /// `:q`.
    Quit,
    OpenWikilink {
        target: String,
        resolved: Option<String>,
    },
    OpenLink(String),
    TagClicked(String),
    HostCommand {
        name: String,
        args: Vec<String>,
    },
    External(External),
    ChangeTheme(String),
    Message {
        text: String,
        error: bool,
    },
    ModeChanged(ModeType),
    /// `:` with [`EditorOptions::command_line_via_host`].
    OpenCommandLine {
        placeholder: Option<String>,
    },
    MermaidOpen(MermaidBlock),
    MermaidUpdate(MermaidBlock),
    MermaidClose {
        key: u64,
    },
    /// Mod-v with an image on the clipboard: the host saves it and calls
    /// [`EditorView::insert_image_link`].
    PasteImage {
        mime: String,
        bytes: Vec<u8>,
    },
    Zoom(ZoomDirection),
}

// ---- overlay state ----------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct PromptState {
    pub kind: PromptKind,
    pub value: String,
    pub placeholder: Option<String>,
    pub completions: Completions,
    pub selected: Option<usize>,
    /// Text before Tab-cycling started (cycling restarts from it).
    pub typed: String,
    /// `Ctrl-r` was pressed: the next char names a register to insert.
    pub reading_register: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FindState {
    pub query: String,
    /// Byte ranges of the matches.
    pub matches: Vec<Range<usize>>,
    pub current: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct OutlineState {
    pub query: String,
    pub items: Vec<OutlineHeader>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletionState {
    pub options: Vec<CompletionOption>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct LangMenu {
    pub toolbar: FenceToolbar,
    pub at: Point<Pixels>,
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum Overlay {
    None,
    Prompt(PromptState),
    Find(FindState),
    Outline(OutlineState),
}

/// Active table cell.
#[derive(Debug, Clone)]
pub(crate) struct TableEdit {
    pub state: TableEditState,
    pub data: TableData,
}

// ---- the view -----------------------------------------------------------------

pub struct EditorView {
    pub(crate) engine: Editor,
    pub(crate) shared: Rc<RefCell<SharedLayout>>,
    pub(crate) model: MarkdownModel,
    pub(crate) folds: FoldState,
    pub(crate) style: EditorStyle,
    pub(crate) options: EditorOptions,
    pub(crate) focus: FocusHandle,

    // geometry
    pub(crate) scroll_y: f32,
    pub(crate) pending_scroll: Option<Scroll>,
    pub(crate) prompt_scroll_start: f32,
    pub(crate) layout_dirty: bool,
    /// Prefix sums of line heights (len = lines + 1), body-line units in px.
    pub(crate) offsets: Vec<f32>,
    /// Measured visual rows of wrapped lines (0-based line → rows).
    pub(crate) wrap_rows: HashMap<usize, usize>,
    pub(crate) wrap_width: Option<f32>,
    /// Inline images: 0-based line → resolved file (first embed per line).
    pub(crate) image_lines: HashMap<usize, std::path::PathBuf>,
    /// Natural sizes of loaded images.
    pub(crate) image_natural: HashMap<std::path::PathBuf, (f32, f32)>,
    pub(crate) viewport: (f32, f32),
    pub(crate) frame: Option<Rc<Frame>>,
    _blur: Subscription,

    // overlays
    pub(crate) overlay: Overlay,
    pub(crate) completion: Option<CompletionState>,
    pub(crate) lang_menu: Option<LangMenu>,
    pub(crate) table: Option<TableEdit>,
    pub(crate) copied_fence: Option<usize>,
    pub(crate) message: Option<(String, bool)>,

    // IME / mouse
    pub(crate) marked: Option<Range<usize>>,
    pub(crate) drag_anchor: Option<usize>,

    // host bookkeeping
    saved_text: String,
    dirty: bool,
    mermaid: Vec<(u64, usize, String)>,
    next_mermaid_key: u64,
    suppress_completion: bool,
}

impl EventEmitter<EditorEvent> for EditorView {}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn md_settings(o: &EditorOptions) -> Settings {
    Settings {
        fold_start_marker: o.fold_start_marker.clone(),
        fold_end_marker: o.fold_end_marker.clone(),
        tab_size: o.tab_size.max(1) as usize,
        rulers: o.rulers.clone(),
        inline_images: o.inline_images,
    }
}

impl EditorView {
    pub fn new(
        text: &str,
        options: EditorOptions,
        style: EditorStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        // cm-helix closes the `:` / search input (uncommitted) on blur.
        let blur = cx.on_blur(&focus, window, |v, _, cx| {
            if matches!(v.overlay, Overlay::Prompt(_)) {
                v.prompt_finish(false, cx);
                cx.notify();
            }
        });
        let shared = Rc::new(RefCell::new(SharedLayout {
            path: options.path.clone(),
            half_page: 10,
            ..Default::default()
        }));
        let config = Config {
            externals: options.externals.clone(),
            tab_size: options.tab_size.max(1) as usize,
            indent_unit: " ".repeat(options.tab_size.max(1) as usize),
            ..Config::default()
        };
        let engine = Editor::with_host(
            text,
            config,
            Box::new(ViewHost {
                shared: shared.clone(),
            }),
        );
        let normalized = engine.text();
        let model = MarkdownModel::new(&normalized, md_settings(&options));
        let mut view = EditorView {
            engine,
            shared,
            model,
            folds: FoldState::new(),
            style,
            options,
            focus,
            _blur: blur,
            scroll_y: 0.,
            pending_scroll: None,
            prompt_scroll_start: 0.,
            layout_dirty: true,
            offsets: Vec::new(),
            wrap_rows: HashMap::new(),
            image_lines: HashMap::new(),
            image_natural: HashMap::new(),
            wrap_width: None,
            viewport: (0., 0.),
            frame: None,
            overlay: Overlay::None,
            completion: None,
            lang_menu: None,
            table: None,
            copied_fence: None,
            message: None,
            marked: None,
            drag_anchor: None,
            saved_text: normalized,
            dirty: false,
            mermaid: Vec::new(),
            next_mermaid_key: 1,
            suppress_completion: false,
        };
        view.ensure_engine_mode(cx);
        view
    }

    // ---- read accessors ---------------------------------------------------

    /// The current (unsaved) buffer.
    pub fn text(&self) -> String {
        self.engine.text()
    }
    /// Same as [`Self::text`], borrowed from the preview model.
    pub fn buffer(&self) -> &str {
        self.model.text()
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    /// `NOR` / `INS` / `SEL`.
    pub fn mode_label(&self) -> &'static str {
        self.engine.mode_label()
    }
    pub fn mode(&self) -> ModeType {
        self.engine.mode().ty
    }
    /// Pending keys (`"2"`, `"g"`, `"<space>"`, ...).
    pub fn pending_status(&self) -> String {
        self.engine.pending_status()
    }
    /// 1-based line / column of the main cursor.
    pub fn line_col(&self) -> (usize, usize) {
        self.engine.line_col()
    }
    /// The engine selection (char offsets, CodeMirror semantics).
    pub fn selection(&self) -> &Selection {
        self.engine.selection()
    }
    pub fn engine(&self) -> &Editor {
        &self.engine
    }
    /// Direct engine access (registers, config, custom commands). Effects
    /// returned by engine calls should be passed to [`Self::apply_effects`].
    pub fn engine_mut(&mut self) -> &mut Editor {
        &mut self.engine
    }
    pub fn model(&self) -> &MarkdownModel {
        &self.model
    }
    pub fn folds(&self) -> &FoldState {
        &self.folds
    }
    pub fn style(&self) -> &EditorStyle {
        &self.style
    }
    pub fn options(&self) -> &EditorOptions {
        &self.options
    }
    /// Scroll offset in pixels (for persistence).
    pub fn scroll_position(&self) -> f32 {
        self.scroll_y
    }
    /// First (at least partly) visible 1-based line of the last frame.
    pub fn first_visible_line(&self) -> usize {
        self.shared.borrow().visible.map_or(1, |v| v.0)
    }
    pub fn visible_lines(&self) -> Option<(usize, usize)> {
        self.shared.borrow().visible
    }
    /// The status-line message of the last command, if any.
    pub fn message(&self) -> Option<(String, bool)> {
        self.message.clone()
    }
    /// Inline prompt (`:` / search) label + text, if open.
    pub fn prompt(&self) -> Option<(String, String)> {
        match &self.overlay {
            Overlay::Prompt(p) => Some((prompt_label(p.kind).to_string(), p.value.clone())),
            _ => None,
        }
    }
    pub fn prompt_completions(&self) -> Vec<String> {
        match &self.overlay {
            Overlay::Prompt(p) => p.completions.options.clone(),
            _ => Vec::new(),
        }
    }
    pub fn find_query(&self) -> Option<(String, usize, usize)> {
        match &self.overlay {
            Overlay::Find(f) => Some((f.query.clone(), f.current, f.matches.len())),
            _ => None,
        }
    }
    pub fn outline_items(&self) -> Option<Vec<String>> {
        match &self.overlay {
            Overlay::Outline(o) => Some(
                o.filtered
                    .iter()
                    .map(|&i| o.items[i].text.clone())
                    .collect(),
            ),
            _ => None,
        }
    }
    pub fn completion_labels(&self) -> Option<Vec<String>> {
        self.completion
            .as_ref()
            .map(|c| c.options.iter().map(|o| o.label.clone()).collect())
    }
    /// Active table cell `(row, col, editing)`.
    pub fn table_cell(&self) -> Option<(usize, usize, bool)> {
        self.table
            .as_ref()
            .map(|t| (t.state.row, t.state.col, t.state.editing))
    }
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }
    pub fn open_mermaid_blocks(&self) -> Vec<MermaidBlock> {
        self.mermaid
            .iter()
            .map(|(k, _, s)| MermaidBlock {
                key: *k,
                source: s.clone(),
            })
            .collect()
    }

    // ---- host mutators ----------------------------------------------------

    /// External reload: replaces the text (diff-mapped selection, folds
    /// kept where possible) and marks it saved.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let fx = self.engine.set_text(text);
        self.apply_effects(fx, cx);
        self.mark_saved(cx);
    }

    /// The host wrote the buffer: it becomes the clean state.
    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved_text = self.engine.text();
        self.set_dirty(false, cx);
    }

    pub fn set_style(&mut self, style: EditorStyle, cx: &mut Context<Self>) {
        self.style = style;
        self.layout_dirty = true;
        self.wrap_rows.clear();
        cx.notify();
    }

    pub fn set_font_size(&mut self, size: Pixels, cx: &mut Context<Self>) {
        let s = self.style.clone().with_font_size(size);
        self.set_style(s, cx);
    }

    pub fn set_options(&mut self, options: EditorOptions, cx: &mut Context<Self>) {
        let settings = md_settings(&options);
        if settings != *self.model.settings() {
            self.model.set_settings(settings);
        }
        self.shared.borrow_mut().path = options.path.clone();
        let cfg = self.engine.config_mut();
        cfg.externals = options.externals.clone();
        cfg.tab_size = options.tab_size.max(1) as usize;
        cfg.indent_unit = " ".repeat(cfg.tab_size);
        self.options = options;
        self.ensure_engine_mode(cx);
        self.layout_dirty = true;
        self.wrap_rows.clear();
        cx.notify();
    }

    pub fn set_vault(
        &mut self,
        vault_files: Vec<String>,
        all_tags: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.options.vault_files = vault_files;
        self.options.all_tags = all_tags;
        cx.notify();
    }

    pub fn set_scroll_position(&mut self, y: f32, cx: &mut Context<Self>) {
        self.scroll_y = y.max(0.);
        self.pending_scroll = None;
        cx.notify();
    }

    /// Moves the cursor to the start of 1-based `line` and centres it.
    pub fn reveal_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let doc = self.engine.doc();
        let line = line.clamp(1, doc.lines());
        let from = doc.line(line).from;
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::single(from, from));
        self.apply_effects(fx, cx);
        self.pending_scroll = Some(Scroll::To {
            pos: from,
            align: ScrollAlign::Center,
        });
        cx.notify();
    }

    pub fn go_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        self.reveal_line(line, cx);
    }

    /// Selection by char offsets (pointer semantics: collapsed = block in
    /// normal mode).
    pub fn set_selection(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::single(anchor, head));
        self.apply_effects(fx, cx);
    }

    /// Feeds keys in fuchico-helix notation (`"ihello<Esc>"`) straight to the
    /// engine; overlays (prompt, find, outline, completion) are bypassed.
    pub fn handle_keys(&mut self, keys: &str, cx: &mut Context<Self>) {
        for ev in fuchico_helix::parse_keys(keys) {
            self.feed_key(ev, cx);
        }
    }

    /// Runs `line` as an accepted `:` command (host palette integration).
    pub fn execute_command(&mut self, line: &str, cx: &mut Context<Self>) {
        let fx = self.engine.execute_command(line);
        self.apply_effects(fx, cx);
    }

    /// Inserts links for files dropped on the editor at the main cursor.
    pub fn insert_dropped_paths(&mut self, paths: &[String], cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let head = self.engine.selection().main().head;
        let before = head
            .checked_sub(1)
            .and_then(|p| self.engine.doc().char_at(p));
        let current = self.options.path.clone().unwrap_or_default();
        let text = fuchico_core::attachments::drop_insert_text(paths, &current, before);
        self.insert_at_main(&text, cx);
    }

    /// Inserts a (pasted image) markdown link at the main cursor.
    pub fn insert_image_link(&mut self, link: &str, cx: &mut Context<Self>) {
        self.insert_at_main(link, cx);
    }

    /// Replaces the first occurrence of `placeholder` (e.g. the
    /// `![uploading image…]()` of a pending paste) with `insert`, leaving
    /// the cursor after it. Returns `false` (and does nothing) when the
    /// placeholder is gone.
    pub fn replace_placeholder(
        &mut self,
        placeholder: &str,
        insert: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(idx) = self.model.text().find(placeholder) else {
            return false;
        };
        let from = self.char_of(idx);
        let to = self.char_of(idx + placeholder.len());
        let fx = self
            .engine
            .apply_host_edit(vec![Change::new(from, to, insert)], None);
        self.apply_effects(fx, cx);
        let end = from + insert.chars().count();
        let fx = self.engine.set_selection(Selection::single(end, end));
        self.apply_effects(fx, cx);
        true
    }

    fn insert_at_main(&mut self, text: &str, cx: &mut Context<Self>) {
        let r = self.engine.selection().main();
        let (from, to) = if self.mode() == ModeType::Insert {
            (r.from(), r.to())
        } else {
            (r.to(), r.to())
        };
        let end = from + text.chars().count();
        let fx = self
            .engine
            .apply_host_edit(vec![Change::new(from, to, text)], None);
        self.apply_effects(fx, cx);
        let fx = self.engine.set_selection(Selection::single(end, end));
        self.apply_effects(fx, cx);
    }

    /// The host closed a mermaid preview.
    pub fn close_mermaid(&mut self, key: u64, cx: &mut Context<Self>) {
        self.mermaid.retain(|(k, _, _)| *k != key);
        cx.notify();
    }

    /// Opens the preview of the mermaid fence starting at byte `anchor`.
    pub fn open_mermaid(&mut self, anchor: usize, cx: &mut Context<Self>) {
        let Some(src) = self.mermaid_source(anchor) else {
            return;
        };
        if let Some((k, _, _)) = self.mermaid.iter().find(|(_, a, _)| *a == anchor) {
            let key = *k;
            cx.emit(EditorEvent::MermaidOpen(MermaidBlock { key, source: src }));
            return;
        }
        let key = self.next_mermaid_key;
        self.next_mermaid_key += 1;
        self.mermaid.push((key, anchor, src.clone()));
        cx.emit(EditorEvent::MermaidOpen(MermaidBlock { key, source: src }));
    }

    fn mermaid_source(&self, anchor: usize) -> Option<String> {
        let p = self.model.parsed();
        let f = p
            .fences
            .iter()
            .find(|f| f.range.start == anchor && f.is_mermaid())?;
        let lines = self.model.lines();
        let from = lines.line_range(f.open_line).end + 1;
        let to = if f.closed {
            lines.line_range(f.last_line).start
        } else {
            f.range.end
        };
        let t = self.model.text();
        let (from, to) = (from.min(t.len()), to.min(t.len()));
        Some(if from < to {
            t[from..to].trim_end_matches('\n').to_string()
        } else {
            String::new()
        })
    }

    // ---- conversions --------------------------------------------------------

    pub(crate) fn byte_of(&self, ch: usize) -> usize {
        let d = self.engine.doc();
        d.char_to_byte(ch.min(d.len()))
    }
    pub(crate) fn char_of(&self, byte: usize) -> usize {
        let t = self.model.text();
        let mut b = byte.min(t.len());
        while !t.is_char_boundary(b) {
            b -= 1;
        }
        self.engine.doc().byte_to_char(b)
    }
    fn ranges_bytes(&self) -> Vec<Range<usize>> {
        self.engine
            .selection()
            .ranges()
            .iter()
            .map(|r| self.byte_of(r.from())..self.byte_of(r.to()))
            .collect()
    }
    fn heads_bytes(&self) -> Vec<usize> {
        self.engine
            .selection()
            .ranges()
            .iter()
            .map(|r| self.byte_of(r.head))
            .collect()
    }

    /// Applies a byte-offset command edit as one undo step.
    fn apply_command_edit(&mut self, e: CommandEdit, cx: &mut Context<Self>) {
        let changes: Vec<Change> = e
            .changes
            .iter()
            .map(|(r, s)| Change::new(self.char_of(r.start), self.char_of(r.end), s.clone()))
            .collect();
        if !changes.is_empty() {
            let fx = self.engine.apply_host_edit(changes, None);
            self.apply_effects(fx, cx);
        }
        if !e.selection.is_empty() {
            let ranges: Vec<HxRange> = e
                .selection
                .iter()
                .map(|&(a, h)| HxRange::new(self.char_of(a), self.char_of(h)))
                .collect();
            let fx = self.engine.set_selection(Selection::create(ranges, 0));
            self.apply_effects(fx, cx);
        }
    }

    /// Applies a markdown edit as an undo step (`group` coalesces runs of
    /// edits, e.g. typing in one table cell).
    fn apply_md_edit(&mut self, e: &MdEdit, group: Option<&str>, cx: &mut Context<Self>) {
        let ch = self.md_change(e);
        let fx = self.engine.apply_host_edit(vec![ch], group);
        self.apply_effects(fx, cx);
    }

    /// Applies a markdown edit inside the running insert session (completion
    /// accept), without an undo step of its own.
    fn apply_md_edit_unrecorded(&mut self, e: &MdEdit, cx: &mut Context<Self>) {
        let ch = self.md_change(e);
        let fx = self.engine.apply_changes(vec![ch]);
        self.apply_effects(fx, cx);
    }

    fn md_change(&self, e: &MdEdit) -> Change {
        Change::new(
            self.char_of(e.range.start),
            self.char_of(e.range.end),
            e.insert.clone(),
        )
    }

    // ---- effects --------------------------------------------------------------

    /// Processes engine effects (also for host calls into [`Self::engine_mut`]).
    pub fn apply_effects(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        let mut doc_changed = false;
        let mut sel_changed = false;
        for fx in effects {
            match fx {
                Effect::DocChanged(edits) => {
                    self.on_doc_changed(&edits, cx);
                    doc_changed = true;
                }
                Effect::SelectionChanged => sel_changed = true,
                Effect::ModeChanged { to, .. } => {
                    if to != ModeType::Insert {
                        self.completion = None;
                        self.marked = None;
                    }
                    cx.emit(EditorEvent::ModeChanged(to));
                }
                Effect::StatusChanged | Effect::Unhandled => {}
                Effect::Scroll(Scroll::RestorePromptStart) => {
                    self.scroll_y = self.prompt_scroll_start;
                    self.pending_scroll = None;
                }
                Effect::Scroll(s) => self.pending_scroll = Some(s),
                Effect::Message { text, error } => {
                    self.message = Some((text.clone(), error));
                    cx.emit(EditorEvent::Message { text, error });
                }
                Effect::OpenCommandLine { placeholder } => {
                    if self.options.command_line_via_host {
                        let _ = self.engine.prompt_close(false);
                        cx.emit(EditorEvent::OpenCommandLine { placeholder });
                    } else {
                        self.open_prompt(PromptKind::Command, placeholder);
                    }
                }
                Effect::OpenSearch { kind, placeholder } => {
                    self.open_prompt(PromptKind::Search(kind), placeholder);
                }
                Effect::External(e) => cx.emit(EditorEvent::External(e)),
                Effect::Save => cx.emit(EditorEvent::Save),
                Effect::Quit => cx.emit(EditorEvent::Quit),
                Effect::HostCommand { name, args } => {
                    cx.emit(EditorEvent::HostCommand { name, args })
                }
                Effect::ChangeTheme(t) => cx.emit(EditorEvent::ChangeTheme(t)),
            }
        }
        if sel_changed || doc_changed {
            if self.pending_scroll.is_none() && self.drag_anchor.is_none() {
                let head = self.engine.selection().main().head;
                self.pending_scroll = Some(Scroll::To {
                    pos: head,
                    align: ScrollAlign::Nearest,
                });
            }
            let head = self.byte_of(self.engine.selection().main().head);
            let before = self.folds.ranges().len();
            self.folds.reveal(head);
            if self.folds.ranges().len() != before {
                self.layout_dirty = true;
            }
            if let Some(t) = &self.table
                && !t.state.editing
                && self
                    .model
                    .table_at(head)
                    .is_none_or(|(_, m)| m.range.start != t.state.anchor)
            {
                self.table = None;
            }
        }
        if doc_changed && self.mode() == ModeType::Insert && !self.suppress_completion {
            self.refresh_completion(true);
        } else if sel_changed && !doc_changed && !self.suppress_completion {
            self.completion = None;
        }
        cx.notify();
    }

    fn on_doc_changed(&mut self, edits: &[HxEdit], cx: &mut Context<Self>) {
        let md: Vec<MdEdit> = edits
            .iter()
            .map(|e| MdEdit::new(e.from_byte..e.to_byte, e.insert.clone()))
            .collect();
        self.folds.map(&md);
        let text = self.engine.text();
        self.model.update(&text, &md);
        self.layout_dirty = true;
        self.marked = self
            .marked
            .take()
            .filter(|m| m.end <= self.engine.doc().len());
        if let Some(t) = &mut self.table {
            t.state.anchor = map_pos(t.state.anchor, &md, Assoc::Before);
        }
        if let Overlay::Find(f) = &mut self.overlay {
            f.matches = find_matches(self.model.text(), &f.query);
            f.current = f.current.min(f.matches.len().saturating_sub(1));
        }
        // Mermaid previews follow their fence.
        let mut events = Vec::new();
        let mut blocks = std::mem::take(&mut self.mermaid);
        blocks.retain_mut(|(key, anchor, src)| {
            *anchor = map_pos(*anchor, &md, Assoc::After);
            match self.mermaid_source(*anchor) {
                Some(s) => {
                    if s != *src {
                        *src = s.clone();
                        events.push(EditorEvent::MermaidUpdate(MermaidBlock {
                            key: *key,
                            source: s,
                        }));
                    }
                    true
                }
                None => {
                    events.push(EditorEvent::MermaidClose { key: *key });
                    false
                }
            }
        });
        self.mermaid = blocks;
        for e in events {
            cx.emit(e);
        }
        let dirty = text != self.saved_text;
        self.set_dirty(dirty, cx);
        cx.emit(EditorEvent::Changed { dirty });
    }

    fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        if dirty != self.dirty {
            self.dirty = dirty;
            cx.emit(EditorEvent::DirtyChanged(dirty));
        }
    }

    // ---- keys -------------------------------------------------------------------

    fn feed_key(&mut self, ev: KeyEvent, cx: &mut Context<Self>) {
        // Bridge the system clipboard for paste/replace keys.
        if let Key::Char(c) = ev.key
            && (matches!(c, 'p' | 'P' | 'R') || !self.engine.pending_status().is_empty())
        {
            let clip = cx.read_from_clipboard().and_then(|i| i.text());
            self.shared.borrow_mut().clipboard = clip;
        }
        self.message = None;
        let fx = self.engine.handle_key(ev);
        let written = self.shared.borrow_mut().clipboard_written.take();
        if let Some(w) = written {
            cx.write_to_clipboard(ClipboardItem::new_string(w));
        }
        self.apply_effects(fx, cx);
    }

    pub(crate) fn on_key_down(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &ev.keystroke;
        if ks.modifiers.platform || ks.modifiers.function {
            return; // Mod-* belongs to actions / the host
        }
        if self.lang_menu.is_some() {
            self.lang_menu_key(ks, cx);
            cx.stop_propagation();
            return;
        }
        let handled = match self.overlay {
            Overlay::Prompt(_) => {
                self.prompt_key(ks, cx);
                true
            }
            Overlay::Find(_) => self.find_key(ks, window, cx),
            Overlay::Outline(_) => {
                self.outline_key(ks, cx);
                true
            }
            Overlay::None => false,
        };
        if handled {
            cx.stop_propagation();
            return;
        }
        if self.completion.is_some() && self.completion_key(ks, cx) {
            cx.stop_propagation();
            return;
        }
        if self.table.is_some() && self.table_key(ks, cx) {
            cx.stop_propagation();
            return;
        }
        let insert = self.mode() == ModeType::Insert;
        if insert && typed_char(ks).is_some() {
            return; // goes to the input handler (IME)
        }
        if self.options.engine == EngineKind::Normal && self.normal_key(ks, cx) {
            cx.stop_propagation();
            return;
        }
        if let Some(kev) = to_helix(ks) {
            self.feed_key(kev, cx);
            cx.stop_propagation();
        }
    }

    // ---- non-modal engine ----------------------------------------------------------

    /// [`EngineKind::Normal`] keeps the engine in insert mode.
    fn ensure_engine_mode(&mut self, cx: &mut Context<Self>) {
        if self.options.engine == EngineKind::Normal && self.mode() != ModeType::Insert {
            if self.mode() == ModeType::Select {
                self.feed_key(KeyEvent::new(Key::Escape, HxMods::default()), cx);
            }
            if self.mode() == ModeType::Normal {
                self.feed_key(KeyEvent::char('i'), cx);
            }
        }
    }

    /// Non-modal keys the insert keymap lacks. Returns whether it handled.
    fn normal_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = ks.modifiers;
        let key = ks.key.as_str();
        if key == "escape" && !m.control && !m.alt {
            return true;
        }
        let main = self.engine.selection().main();
        if matches!(key, "backspace" | "delete")
            && !m.control
            && !m.alt
            && !main.is_empty()
        {
            self.message = None;
            let fx = self.engine.insert_text("");
            self.apply_effects(fx, cx);
            return true;
        }
        let nav = matches!(
            key,
            "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown"
        );
        if nav && m.shift && !m.control {
            let anchor = main.anchor;
            let mut plain = ks.clone();
            plain.modifiers.shift = false;
            if let Some(kev) = to_helix(&plain) {
                if !main.is_empty() {
                    // Move from the head, not from the collapsed range edge.
                    let fx = self
                        .engine
                        .set_selection_from_pointer(Selection::single(main.head, main.head));
                    self.apply_effects(fx, cx);
                }
                self.feed_key(kev, cx);
                let head = self.engine.selection().main().head;
                let fx = self
                    .engine
                    .set_selection_from_pointer(Selection::single(anchor, head));
                self.apply_effects(fx, cx);
            }
            return true;
        }
        if nav && !m.shift && !main.is_empty() && matches!(key, "left" | "right") {
            let to = if key == "left" { main.from() } else { main.to() };
            let fx = self
                .engine
                .set_selection_from_pointer(Selection::single(to, to));
            self.apply_effects(fx, cx);
            return true;
        }
        false
    }

    /// Mod-z / Mod-shift-z (CodeMirror `historyKeymap`), every engine.
    pub fn undo(&mut self, redo: bool, cx: &mut Context<Self>) {
        let was_insert = self.mode() == ModeType::Insert;
        if was_insert {
            self.feed_key(KeyEvent::new(Key::Escape, HxMods::default()), cx);
        }
        self.feed_key(KeyEvent::char(if redo { 'U' } else { 'u' }), cx);
        if was_insert && self.options.engine == EngineKind::Helix {
            self.feed_key(KeyEvent::char('i'), cx);
        }
        self.ensure_engine_mode(cx);
    }

    /// Mod-a.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let len = self.engine.doc().len();
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::single(0, len));
        self.apply_effects(fx, cx);
    }

    // ---- prompt (`:` and search) ---------------------------------------------------

    fn open_prompt(&mut self, kind: PromptKind, placeholder: Option<String>) {
        self.prompt_scroll_start = self.scroll_y;
        let completions = match kind {
            PromptKind::Command => self.engine.prompt_completions(""),
            _ => Completions::default(),
        };
        self.overlay = Overlay::Prompt(PromptState {
            kind,
            value: String::new(),
            placeholder,
            completions,
            selected: None,
            typed: String::new(),
            reading_register: false,
        });
        self.completion = None;
    }

    fn prompt_set(&mut self, value: String, cx: &mut Context<Self>) {
        let fx = self.engine.prompt_input(&value);
        let completions = self.engine.prompt_completions(&value);
        if let Overlay::Prompt(p) = &mut self.overlay {
            p.value = value.clone();
            p.typed = value;
            p.completions = completions;
            p.selected = None;
        }
        self.apply_effects(fx, cx);
    }

    fn set_reading_register(&mut self, on: bool) {
        if let Overlay::Prompt(p) = &mut self.overlay {
            p.reading_register = on;
        }
    }

    fn prompt_finish(&mut self, accept: bool, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        let fx = self.engine.prompt_close(accept);
        self.apply_effects(fx, cx);
    }

    fn prompt_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
        let Overlay::Prompt(p) = &self.overlay else {
            return;
        };
        let p = p.clone();
        let m = ks.modifiers;
        // cm-helix `commandInput`: `Ctrl-r <reg>` appends the register's
        // first value; Escape only cancels the pending read.
        if p.kind == PromptKind::Command {
            if ks.key == "r" && m.control && !m.alt && !m.platform {
                self.set_reading_register(true);
                cx.notify();
                return;
            }
            if p.reading_register {
                if ks.key == "escape" {
                    self.set_reading_register(false);
                } else if let Some(s) = typed_char(ks)
                    && let Some(reg) = s.chars().next()
                    && s.chars().count() == 1
                {
                    self.set_reading_register(false);
                    let text = self.engine.prompt_read_register(reg).unwrap_or_default();
                    let mut v = p.value.clone();
                    v.push_str(&text);
                    self.prompt_set(v, cx);
                }
                cx.notify();
                return;
            }
        }
        match ks.key.as_str() {
            "escape" => self.prompt_finish(false, cx),
            "enter" => self.prompt_finish(true, cx),
            "backspace" => {
                if p.value.is_empty() {
                    self.prompt_finish(false, cx);
                } else {
                    let mut v = p.value.clone();
                    v.pop();
                    self.prompt_set(v, cx);
                }
            }
            "tab" => {
                let n = p.completions.options.len();
                if n == 0 {
                    return;
                }
                let next = match (p.selected, m.shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                let value = self
                    .engine
                    .prompt_accept_completion(&p.typed, &p.completions.options[next]);
                let fx = self.engine.prompt_input(&value);
                if let Overlay::Prompt(pp) = &mut self.overlay {
                    pp.value = value;
                    pp.selected = Some(next);
                }
                self.apply_effects(fx, cx);
            }
            "up" | "down" => {
                if let Some(v) = self.engine.prompt_history(ks.key == "up") {
                    self.prompt_set(v, cx);
                }
            }
            "u" if m.control => self.prompt_set(String::new(), cx),
            "w" if m.control => {
                let t = p.value.trim_end();
                let cut = t.rfind(' ').map_or(0, |i| i + 1);
                self.prompt_set(t[..cut].to_string(), cx);
            }
            _ => {
                if let Some(s) = typed_char(ks) {
                    let mut v = p.value.clone();
                    v.push_str(&s);
                    self.prompt_set(v, cx);
                }
            }
        }
        cx.notify();
    }

    // ---- find bar (Mod-f) ------------------------------------------------------

    pub(crate) fn open_find(&mut self, cx: &mut Context<Self>) {
        let r = self.engine.selection().main();
        let sel = self.engine.doc().slice(r.from(), r.to());
        let query = if sel.chars().count() > 1 && !sel.contains('\n') {
            sel
        } else {
            String::new()
        };
        let matches = find_matches(self.model.text(), &query);
        self.overlay = Overlay::Find(FindState {
            query,
            matches,
            current: 0,
        });
        cx.notify();
    }

    fn find_set(&mut self, query: String, cx: &mut Context<Self>) {
        let head = self.byte_of(self.engine.selection().main().from());
        let matches = find_matches(self.model.text(), &query);
        let current = matches.iter().position(|m| m.start >= head).unwrap_or(0);
        self.overlay = Overlay::Find(FindState {
            query,
            matches,
            current,
        });
        self.find_select(cx);
    }

    fn find_select(&mut self, cx: &mut Context<Self>) {
        let Overlay::Find(f) = &self.overlay else {
            return;
        };
        let Some(m) = f.matches.get(f.current).cloned() else {
            return;
        };
        let (a, h) = (self.char_of(m.start), self.char_of(m.end));
        let fx = self.engine.set_selection(Selection::single(a, h));
        self.apply_effects(fx, cx);
        self.pending_scroll = Some(Scroll::To {
            pos: h,
            align: ScrollAlign::Center,
        });
    }

    fn find_key(&mut self, ks: &Keystroke, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Overlay::Find(f) = &self.overlay else {
            return false;
        };
        let f = f.clone();
        match ks.key.as_str() {
            "escape" => self.overlay = Overlay::None,
            "enter" | "f3" => {
                let n = f.matches.len();
                if n > 0 {
                    let cur = if ks.modifiers.shift {
                        (f.current + n - 1) % n
                    } else {
                        (f.current + 1) % n
                    };
                    // The first Enter after typing selects the current match.
                    let cur = if self.find_selected(&f) {
                        cur
                    } else {
                        f.current
                    };
                    if let Overlay::Find(ff) = &mut self.overlay {
                        ff.current = cur;
                    }
                    self.find_select(cx);
                }
            }
            "backspace" => {
                let mut q = f.query.clone();
                q.pop();
                self.find_set(q, cx);
            }
            _ => match typed_char(ks) {
                Some(s) => {
                    let mut q = f.query.clone();
                    q.push_str(&s);
                    self.find_set(q, cx);
                }
                None => return true,
            },
        }
        cx.notify();
        true
    }

    fn find_selected(&self, f: &FindState) -> bool {
        f.matches.get(f.current).is_some_and(|m| {
            let r = self.engine.selection().main();
            self.byte_of(r.from()) == m.start && self.byte_of(r.to()) == m.end
        })
    }

    // ---- outline (Mod-o) ---------------------------------------------------------

    pub(crate) fn open_outline(&mut self, cx: &mut Context<Self>) {
        let items = self.model.outline();
        let filtered = (0..items.len()).collect();
        self.overlay = Overlay::Outline(OutlineState {
            query: String::new(),
            items,
            filtered,
            selected: 0,
        });
        cx.notify();
    }

    fn outline_filter(o: &mut OutlineState) {
        if o.query.is_empty() {
            o.filtered = (0..o.items.len()).collect();
        } else {
            let mut scored: Vec<(i64, usize)> = o
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, h)| {
                    let r = fuzzy_match(&o.query, &h.text);
                    r.matched.then_some((r.score as i64, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            o.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
        o.selected = 0;
    }

    pub(crate) fn outline_pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Overlay::Outline(o) = &self.overlay else {
            return;
        };
        let Some(&i) = o.filtered.get(ix) else { return };
        let line = o.items[i].line;
        self.overlay = Overlay::None;
        self.reveal_line(line, cx);
    }

    fn outline_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
        let Overlay::Outline(o) = &mut self.overlay else {
            return;
        };
        let n = o.filtered.len();
        match ks.key.as_str() {
            "escape" => self.overlay = Overlay::None,
            "enter" => {
                let s = o.selected;
                self.outline_pick(s, cx);
            }
            "down" => o.selected = if n == 0 { 0 } else { (o.selected + 1) % n },
            "up" => o.selected = if n == 0 { 0 } else { (o.selected + n - 1) % n },
            "n" if ks.modifiers.control => {
                o.selected = if n == 0 { 0 } else { (o.selected + 1) % n }
            }
            "p" if ks.modifiers.control => {
                o.selected = if n == 0 { 0 } else { (o.selected + n - 1) % n }
            }
            "backspace" => {
                o.query.pop();
                Self::outline_filter(o);
            }
            _ => {
                if let Some(s) = typed_char(ks) {
                    o.query.push_str(&s);
                    Self::outline_filter(o);
                }
            }
        }
        cx.notify();
    }

    // ---- completion popup --------------------------------------------------------

    /// Re-queries the completion sources at the main cursor. `trigger`:
    /// open only when a source's trigger matches (typing); otherwise
    /// (explicit pickers) open whenever there are options.
    pub(crate) fn refresh_completion(&mut self, trigger: bool) {
        let head = self.byte_of(self.engine.selection().main().head);
        let text = self.model.text();
        let line_start = text[..head].rfind('\n').map_or(0, |i| i + 1);
        let before = &text[line_start..head];
        if trigger && self.completion.is_none() && !should_open_completion(before, false) {
            return;
        }
        let read = |_: &str| -> Option<String> { None };
        let ctx = CompletionContext {
            pos: head,
            vault_files: &self.options.vault_files,
            all_tags: &self.options.all_tags,
            today: Local::now().date_naive(),
            read_note: &read,
        };
        let options = self.model.completions(&ctx);
        self.completion = (!options.is_empty()).then_some(CompletionState {
            options,
            selected: 0,
        });
    }

    pub(crate) fn accept_completion(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let Some(opt) = c.options.get(ix).cloned() else {
            return;
        };
        self.suppress_completion = opt.picked;
        self.apply_md_edit_unrecorded(&opt.edit, cx);
        let cur = self.char_of(opt.cursor);
        let fx = self.engine.set_selection(Selection::single(cur, cur));
        self.apply_effects(fx, cx);
        self.suppress_completion = false;
        if !opt.picked && self.mode() == ModeType::Insert {
            self.refresh_completion(true);
        }
        cx.notify();
    }

    fn completion_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(c) = &mut self.completion else {
            return false;
        };
        let n = c.options.len();
        let m = ks.modifiers;
        match ks.key.as_str() {
            "down" => c.selected = (c.selected + 1) % n,
            "up" => c.selected = (c.selected + n - 1) % n,
            "n" if m.control => c.selected = (c.selected + 1) % n,
            "p" if m.control => c.selected = (c.selected + n - 1) % n,
            "pagedown" => c.selected = (c.selected + 8).min(n - 1),
            "pageup" => c.selected = c.selected.saturating_sub(8),
            "enter" | "tab" => {
                let s = c.selected;
                self.accept_completion(s, cx);
            }
            "escape" => self.completion = None,
            _ => return false,
        }
        cx.notify();
        true
    }

    // ---- tables -------------------------------------------------------------------

    fn table_model(&self) -> Option<TableModel> {
        let t = self.table.as_ref()?;
        self.model
            .table_at(t.state.anchor)
            .map(|(_, m)| m)
            .filter(|m| m.range.start == t.state.anchor)
    }

    fn apply_table_outcome(&mut self, out: TableOutcome, cx: &mut Context<Self>) {
        self.suppress_completion = true;
        if let Some(e) = &out.edit {
            self.apply_md_edit(e, None, cx);
        }
        let sel = self.char_of(out.selection);
        let fx = self.engine.set_selection(Selection::single(sel, sel));
        self.table = out.state.map(|state| TableEdit {
            state,
            data: out.data.clone(),
        });
        self.apply_effects(fx, cx);
        if let Some(t) = &mut self.table {
            // apply_effects may have dropped a navigating table; re-arm.
            t.data = out.data;
        } else if let Some(state) = out.state {
            self.table = Some(TableEdit {
                state,
                data: out.data,
            });
        }
        self.suppress_completion = false;
        cx.notify();
    }

    /// Activates cell `(row, col)` of the table starting at byte `anchor`.
    pub fn table_activate(
        &mut self,
        anchor: usize,
        row: usize,
        col: usize,
        editing: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((_, model)) = self.model.table_at(anchor) else {
            return;
        };
        let data = model.data();
        let out = fuchico_markdown::table::table_activate(
            self.model.text(),
            &model,
            data,
            row,
            col,
            editing,
        );
        self.apply_table_outcome(out, cx);
    }

    pub fn table_add_row(&mut self, anchor: usize, cx: &mut Context<Self>) {
        let Some((_, model)) = self.model.table_at(anchor) else {
            return;
        };
        let st = self.table.as_ref().map(|t| t.state);
        let out =
            fuchico_markdown::table::table_add_row(self.model.text(), &model, model.data(), st);
        self.apply_table_outcome(out, cx);
    }

    pub fn table_add_column(&mut self, anchor: usize, cx: &mut Context<Self>) {
        let Some((_, model)) = self.model.table_at(anchor) else {
            return;
        };
        let st = self.table.as_ref().map(|t| t.state);
        let out =
            fuchico_markdown::table::table_add_column(self.model.text(), &model, model.data(), st);
        self.apply_table_outcome(out, cx);
    }

    fn table_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(model) = self.table_model() else {
            self.table = None;
            return false;
        };
        let t = self.table.clone().unwrap();
        let m = ks.modifiers;
        let key = match (ks.key.as_str(), t.state.editing) {
            ("escape", _) => Some(TableKey::Escape),
            ("tab", _) if m.shift => Some(TableKey::ShiftTab),
            ("tab", _) => Some(TableKey::Tab),
            ("enter", true) => Some(TableKey::Enter),
            ("enter", false) | ("i", false) => Some(TableKey::Edit),
            ("left", false) | ("h", false) => Some(TableKey::Left),
            ("right", false) | ("l", false) => Some(TableKey::Right),
            ("down", false) | ("j", false) => Some(TableKey::Down),
            ("up", false) | ("k", false) => Some(TableKey::Up),
            ("up", true) => Some(TableKey::Up),
            ("down", true) => Some(TableKey::Down),
            _ => None,
        };
        if let Some(key) = key {
            let out =
                fuchico_markdown::table::table_key(self.model.text(), &model, t.data, t.state, key);
            self.apply_table_outcome(out, cx);
            return true;
        }
        if !t.state.editing {
            return true; // swallow other keys while navigating cells
        }
        let (r, c) = (t.state.row, t.state.col);
        let mut cell = t.data.cell(r, c).to_string();
        if ks.key == "backspace" {
            cell.pop();
        } else if let Some(s) = typed_char(ks) {
            cell.push_str(&s);
        } else {
            return true;
        }
        self.table_set_cell(model, t, cell, cx);
        true
    }

    fn table_set_cell(
        &mut self,
        model: TableModel,
        mut t: TableEdit,
        cell: String,
        cx: &mut Context<Self>,
    ) {
        t.data.set_cell(t.state.row, t.state.col, cell);
        let edit = model.flush(self.model.text(), &t.data);
        self.suppress_completion = true;
        self.table = Some(t.clone());
        if let Some(e) = edit {
            let group = format!("table:{}:{}:{}", t.state.anchor, t.state.row, t.state.col);
            self.apply_md_edit(&e, Some(&group), cx);
        }
        self.table = Some(t);
        self.suppress_completion = false;
        cx.notify();
    }

    /// Text typed into the active table cell (input handler path).
    pub(crate) fn table_input(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(t) = self.table.clone().filter(|t| t.state.editing) else {
            return false;
        };
        let Some(model) = self.table_model() else {
            return false;
        };
        let mut cell = t.data.cell(t.state.row, t.state.col).to_string();
        cell.push_str(text);
        self.table_set_cell(model, t, cell, cx);
        true
    }

    // ---- language menu -----------------------------------------------------------

    pub(crate) fn open_lang_menu(
        &mut self,
        toolbar: FenceToolbar,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let selected = toolbar
            .options
            .iter()
            .position(|o| *o == toolbar.language)
            .unwrap_or(0);
        self.lang_menu = Some(LangMenu {
            toolbar,
            at,
            selected,
        });
        cx.notify();
    }

    pub(crate) fn pick_language(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.lang_menu.take() else {
            return;
        };
        if let Some(lang) = menu.toolbar.options.get(ix) {
            let e = menu.toolbar.set_language(lang);
            self.apply_md_edit(&e, None, cx);
        }
        cx.notify();
    }

    fn lang_menu_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
        let Some(m) = &mut self.lang_menu else { return };
        let n = m.toolbar.options.len().max(1);
        match ks.key.as_str() {
            "escape" => self.lang_menu = None,
            "down" | "j" => m.selected = (m.selected + 1) % n,
            "up" | "k" => m.selected = (m.selected + n - 1) % n,
            "enter" => {
                let s = m.selected;
                self.pick_language(s, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    // ---- actions ------------------------------------------------------------------

    fn act_toggle_checkbox(
        &mut self,
        _: &EditorToggleCheckbox,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(e) = commands::toggle_checkbox(self.model.text(), &self.heads_bytes()) {
            self.apply_command_edit(e, cx);
        }
    }
    fn act_insert_date(&mut self, _: &EditorInsertDate, _: &mut Window, cx: &mut Context<Self>) {
        let s = commands::format_date(&Local::now().naive_local());
        self.insert_text_at_selections(&s, cx);
    }
    fn act_insert_datetime(
        &mut self,
        _: &EditorInsertDateTime,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = commands::format_date_time(&Local::now().naive_local());
        self.insert_text_at_selections(&s, cx);
    }
    /// `insertAtCursor`: every range replaced by `text`.
    pub fn insert_text_at_selections(&mut self, text: &str, cx: &mut Context<Self>) {
        let e = commands::insert_at_cursor(&self.ranges_bytes(), text);
        self.apply_command_edit(e, cx);
    }
    fn act_insert_region(
        &mut self,
        _: &EditorInsertRegion,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (s, e) = (
            self.options.fold_start_marker.clone(),
            self.options.fold_end_marker.clone(),
        );
        // Like the original: in normal mode the block cursor is a 1-char
        // CodeMirror selection, so the current line gets wrapped.
        let ranges = self.ranges_bytes();
        let edit = commands::insert_region(self.model.text(), &ranges, &s, &e);
        self.apply_command_edit(edit, cx);
    }
    fn act_insert_table(&mut self, _: &EditorInsertTable, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.ranges_bytes()[self.engine.selection().main_index()].clone();
        let r = if self.mode() != ModeType::Insert {
            r.start..r.start
        } else {
            r
        };
        let (edit, from) = commands::insert_table(self.model.text(), r);
        self.apply_command_edit(edit, cx);
        self.table_activate(from, 0, 0, true, cx);
    }
    fn act_toggle_task(
        &mut self,
        _: &EditorToggleTaskLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(e) = commands::toggle_task_line(self.model.text(), &self.ranges_bytes()) {
            self.apply_command_edit(e, cx);
        }
    }
    fn pick(&mut self, kind: TokenKind, cx: &mut Context<Self>) {
        let head = self.byte_of(self.engine.selection().main().head);
        let e = commands::pick_token(self.model.text(), head, kind);
        if self.mode() != ModeType::Insert {
            let fx = self.engine.handle_key(KeyEvent::char('i'));
            self.apply_effects(fx, cx);
        }
        self.suppress_completion = true;
        self.apply_command_edit(e, cx);
        self.suppress_completion = false;
        self.completion = None;
        self.refresh_completion(false);
        cx.notify();
    }
    fn act_pick_due(&mut self, _: &EditorPickDueDate, _: &mut Window, cx: &mut Context<Self>) {
        self.pick(TokenKind::Due, cx);
    }
    fn act_pick_recur(&mut self, _: &EditorPickRecurrence, _: &mut Window, cx: &mut Context<Self>) {
        self.pick(TokenKind::Recurrence, cx);
    }
    fn act_outline(&mut self, _: &EditorOpenOutline, _: &mut Window, cx: &mut Context<Self>) {
        self.open_outline(cx);
    }
    fn act_goto(&mut self, a: &GoToLine, _: &mut Window, cx: &mut Context<Self>) {
        self.reveal_line(a.line as usize, cx);
    }
    fn act_tab(&mut self, _: &EditorTab, window: &mut Window, cx: &mut Context<Self>) {
        self.synth_key("tab", window, cx);
    }
    fn act_backtab(&mut self, _: &EditorBacktab, window: &mut Window, cx: &mut Context<Self>) {
        self.synth_key("shift-tab", window, cx);
    }
    fn synth_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(keystroke) = Keystroke::parse(key) else {
            return;
        };
        let ev = KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        };
        self.on_key_down(&ev, window, cx);
    }
    fn act_find(&mut self, _: &OpenFind, _: &mut Window, cx: &mut Context<Self>) {
        self.open_find(cx);
    }
    fn act_save(&mut self, _: &SaveBuffer, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Save);
    }
    fn act_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Zoom(ZoomDirection::In));
    }
    fn act_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Zoom(ZoomDirection::Out));
    }
    fn act_zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Zoom(ZoomDirection::Reset));
    }

    /// Fold/unfold the line under the main cursor (0-based `line`).
    pub fn toggle_fold_line(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.model.toggle_fold(&mut self.folds, line) {
            self.after_fold(cx);
        }
    }

    fn after_fold(&mut self, cx: &mut Context<Self>) {
        // Keep the cursor out of folded ranges (they would reveal at once).
        let head = self.byte_of(self.engine.selection().main().head);
        if let Some(r) = self.folds.covering(head) {
            let ls = self.model.text()[..r.start]
                .rfind('\n')
                .map_or(0, |i| i + 1);
            let c = self.char_of(ls);
            let fx = self
                .engine
                .set_selection_from_pointer(Selection::single(c, c));
            // The fold must survive this selection change.
            let keep = self.folds.clone();
            self.apply_effects(fx, cx);
            self.folds = keep;
        }
        self.layout_dirty = true;
        cx.notify();
    }

    fn act_fold(&mut self, _: &FoldAtCursor, _: &mut Window, cx: &mut Context<Self>) {
        let head = self.byte_of(self.engine.selection().main().head);
        let line = self.model.lines().line_of(head);
        for l in (0..=line).rev() {
            if let Some(r) = self.model.foldable(l)
                && (l == line || r.end >= head)
            {
                self.folds.fold(r);
                self.after_fold(cx);
                return;
            }
        }
    }
    fn act_unfold(&mut self, _: &UnfoldAtCursor, _: &mut Window, cx: &mut Context<Self>) {
        let head = self.byte_of(self.engine.selection().main().head);
        let lr = self.model.line_range(self.model.lines().line_of(head));
        self.folds.unfold_in(lr.start, lr.end);
        self.layout_dirty = true;
        cx.notify();
    }
    fn act_fold_all(&mut self, _: &FoldAll, _: &mut Window, cx: &mut Context<Self>) {
        for r in self.model.fold_all() {
            self.folds.fold(r);
        }
        self.after_fold(cx);
    }
    fn act_unfold_all(&mut self, _: &UnfoldAll, _: &mut Window, cx: &mut Context<Self>) {
        self.folds.clear();
        self.layout_dirty = true;
        cx.notify();
    }

    fn selected_text(&self) -> String {
        let d = self.engine.doc();
        self.engine
            .selection()
            .ranges()
            .iter()
            .map(|r| d.slice(r.from(), r.to()))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn act_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text()));
    }
    fn act_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text()));
        let changes: Vec<Change> = self
            .engine
            .selection()
            .ranges()
            .iter()
            .map(|r| Change::new(r.from(), r.to(), ""))
            .collect();
        let fx = self.engine.apply_host_edit(changes, None);
        self.apply_effects(fx, cx);
    }
    fn act_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for e in item.entries() {
            if let ClipboardEntry::Image(img) = e {
                cx.emit(EditorEvent::PasteImage {
                    mime: img.format.mime_type().to_string(),
                    bytes: img.bytes.clone(),
                });
                return;
            }
        }
        let Some(text) = item.text() else { return };
        if self.mode() == ModeType::Insert {
            let fx = self.engine.insert_text(&text);
            self.apply_effects(fx, cx);
        } else {
            let changes: Vec<Change> = self
                .engine
                .selection()
                .ranges()
                .iter()
                .map(|r| Change::new(r.from(), r.to(), text.clone()))
                .collect();
            let fx = self.engine.apply_host_edit(changes, None);
            self.apply_effects(fx, cx);
        }
    }

    // ---- mouse ------------------------------------------------------------------------

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.lang_menu = None;
        if !matches!(self.overlay, Overlay::None | Overlay::Find(_)) {
            if let Overlay::Prompt(_) = self.overlay {
                self.prompt_finish(false, cx);
            } else {
                self.overlay = Overlay::None;
            }
        }
        self.completion = None;
        let Some(frame) = self.frame.clone() else {
            return;
        };
        if let Some(hit) = frame.hit_at(ev.position) {
            self.on_hit(hit, ev.position, cx);
            return;
        }
        let Some(pos) = self.pos_at(&frame, ev.position) else {
            return;
        };
        self.table = self.table.take().filter(|_| false);
        let anchor = if ev.modifiers.shift {
            // Extend from the anchor; a 1-char block cursor (normal mode)
            // extends from whichever side keeps the clicked char inside.
            let r = self.engine.selection().main();
            if r.to() - r.from() <= 1 {
                if pos >= r.from() { r.from() } else { r.to() }
            } else {
                r.anchor
            }
        } else if ev.click_count == 2 {
            // Double click: the word under the pointer.
            let (a, b) = word_at(self.engine.doc(), pos);
            self.drag_anchor = Some(a);
            let fx = self
                .engine
                .set_selection_from_pointer(Selection::single(a, b));
            self.apply_effects(fx, cx);
            return;
        } else {
            pos
        };
        self.drag_anchor = Some(anchor);
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::single(anchor, pos));
        self.apply_effects(fx, cx);
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(anchor), Some(MouseButton::Left)) = (self.drag_anchor, ev.pressed_button) else {
            return;
        };
        let Some(frame) = self.frame.clone() else {
            return;
        };
        let Some(pos) = self.pos_at(&frame, ev.position) else {
            return;
        };
        let fx = self
            .engine
            .set_selection_from_pointer(Selection::single(anchor, pos));
        self.apply_effects(fx, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag_anchor = None;
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let lh = self.style.line_height_px();
        let d = ev.delta.pixel_delta(lh);
        self.scroll_y = (self.scroll_y - f32::from(d.y)).max(0.);
        self.pending_scroll = None;
        cx.notify();
    }

    fn on_hit(&mut self, hit: Hit, at: Point<Pixels>, cx: &mut Context<Self>) {
        match hit {
            Hit::Checkbox(edit) => self.apply_md_edit(&edit, None, cx),
            Hit::Link(url) => cx.emit(EditorEvent::OpenLink(url)),
            Hit::Wikilink { target, resolved } => {
                cx.emit(EditorEvent::OpenWikilink { target, resolved })
            }
            Hit::Tag(tag) => cx.emit(EditorEvent::TagClicked(tag)),
            Hit::FoldGutter(line) => self.toggle_fold_line(line, cx),
            Hit::RegionChevron(byte) => {
                let line = self.model.lines().line_of(byte);
                self.toggle_fold_line(line, cx);
            }
            Hit::FoldPill(range) => {
                self.folds.unfold_in(range.start, range.start);
                self.layout_dirty = true;
                cx.notify();
            }
            Hit::Copy { fence, text } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.copied_fence = Some(fence);
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(2000))
                        .await;
                    let _ = this.update(cx, |v, cx| {
                        v.copied_fence = None;
                        cx.notify();
                    });
                })
                .detach();
                cx.notify();
            }
            Hit::Lang(tb) => self.open_lang_menu(tb, at, cx),
            Hit::Mermaid { anchor } => self.open_mermaid(anchor, cx),
            Hit::TableCell { anchor, row, col } => self.table_activate(anchor, row, col, true, cx),
            Hit::TableAddRow { anchor } => self.table_add_row(anchor, cx),
            Hit::TableAddCol { anchor } => self.table_add_column(anchor, cx),
        }
    }

    /// Char offset under a window point (clamped to the visible text).
    pub(crate) fn pos_at(&self, frame: &Frame, p: Point<Pixels>) -> Option<usize> {
        let block = self.mode() != ModeType::Insert;
        let (byte, _) = frame.byte_at(p, block)?;
        Some(self.char_of(byte))
    }

    /// Debug/test view of the last frame: clickable regions as
    /// `(debug name, window bounds)` (e.g. `"FoldGutter(0)"`, `"Checkbox(..)"`).
    pub fn debug_hits(&self) -> Vec<(String, gpui_kit::Bounds<Pixels>)> {
        self.frame.as_ref().map_or_else(Vec::new, |f| {
            f.hits.iter().map(|(b, h)| (format!("{h:?}"), *b)).collect()
        })
    }

    /// Painted cursors of the last frame: (window bounds, primary, fill
    /// colour).
    pub fn debug_cursors(&self) -> Vec<(gpui_kit::Bounds<Pixels>, bool, gpui_kit::Hsla)> {
        self.frame
            .as_ref()
            .map_or_else(Vec::new, |f| f.cursors.clone())
    }

    /// Gutter labels of the last frame: (0-based line, label).
    pub fn debug_gutter(&self) -> Vec<(usize, String)> {
        self.frame
            .as_ref()
            .map_or_else(Vec::new, |f| f.gutter.clone())
    }

    /// Window x of the rulers painted in the last frame, and the x of
    /// the text column 0 (`(rulers, char width)`).
    pub fn debug_rulers(&self) -> (Vec<Pixels>, Pixels) {
        self.frame
            .as_ref()
            .map_or_else(|| (Vec::new(), px(0.)), |f| (f.rulers.clone(), f.char_w))
    }

    /// Inline image previews painted in the last frame: (0-based line,
    /// bounds).
    pub fn debug_images(&self) -> Vec<(usize, gpui_kit::Bounds<Pixels>)> {
        self.frame
            .as_ref()
            .map_or_else(Vec::new, |f| f.images.clone())
    }

    /// 0-based lines laid out in the last frame (only visible ones).
    pub fn debug_laid_out_lines(&self) -> Vec<usize> {
        self.frame
            .as_ref()
            .map_or_else(Vec::new, |f| f.lines.iter().map(|l| l.line).collect())
    }

    /// Char offset at a window position (tests / host drag-and-drop).
    pub fn position_for_point(&self, p: Point<Pixels>) -> Option<usize> {
        self.pos_at(self.frame.as_ref()?, p)
    }

    /// Window-space bounds of the char at `pos` in the last frame.
    pub fn bounds_for_char(&self, pos: usize) -> Option<gpui_kit::Bounds<Pixels>> {
        let f = self.frame.as_ref()?;
        f.char_bounds(self.byte_of(pos), self.model.text())
    }

    // ---- input handler (IME) ----------------------------------------------------------

    pub(crate) fn ime_insert(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        if self.lang_menu.is_some() {
            return;
        }
        match &mut self.overlay {
            Overlay::Prompt(p) => {
                let v = format!("{}{text}", p.value);
                self.prompt_set(v, cx);
                return;
            }
            Overlay::Find(f) => {
                let q = format!("{}{text}", f.query);
                self.find_set(q, cx);
                cx.notify();
                return;
            }
            Overlay::Outline(o) => {
                o.query.push_str(text);
                Self::outline_filter(o);
                cx.notify();
                return;
            }
            Overlay::None => {}
        }
        if self.table_input(text, cx) {
            return;
        }
        if let Some(m) = self.marked.take() {
            // Commit: drop the composition, then type the final text.
            let fx = self
                .engine
                .apply_changes(vec![Change::new(m.start, m.end, "")]);
            self.apply_effects(fx, cx);
            let fx = self
                .engine
                .set_selection(Selection::single(m.start, m.start));
            self.apply_effects(fx, cx);
        } else if let Some(r) = range_utf16
            && !r.is_empty()
            && self.mode() == ModeType::Insert
        {
            // Accent / replacement of already typed text.
            let rope = self.engine.doc().rope();
            let a = rope.utf16_cu_to_char(r.start.min(rope.len_utf16_cu()));
            let b = rope.utf16_cu_to_char(r.end.min(rope.len_utf16_cu()));
            let fx = self.engine.apply_changes(vec![Change::new(a, b, "")]);
            self.apply_effects(fx, cx);
            let fx = self.engine.set_selection(Selection::single(a, a));
            self.apply_effects(fx, cx);
        }
        let fx = self.engine.insert_text(text);
        self.apply_effects(fx, cx);
    }

    pub(crate) fn ime_mark(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.mode() != ModeType::Insert || !matches!(self.overlay, Overlay::None) {
            return;
        }
        let (from, to) = match self.marked.take() {
            Some(m) => (m.start, m.end),
            None => {
                let h = self.engine.selection().main().head;
                (h, h)
            }
        };
        self.suppress_completion = true;
        let fx = self.engine.apply_changes(vec![Change::new(from, to, text)]);
        self.apply_effects(fx, cx);
        let end = from + text.chars().count();
        let fx = self.engine.set_selection(Selection::single(end, end));
        self.apply_effects(fx, cx);
        self.suppress_completion = false;
        self.marked = (!text.is_empty()).then_some(from..end);
        cx.notify();
    }

    // ---- render -----------------------------------------------------------------------

    fn chip_label(&self) -> &'static str {
        match self.mode() {
            ModeType::Normal => "NORMAL",
            ModeType::Insert => "INSERT",
            ModeType::Select => "SELECT",
        }
    }
}

/// Columns and rows of the `:` completion grid (`repeat(auto-fill,
/// minmax(10em, 1fr))`, filled column by column).
#[doc(hidden)]
pub fn prompt_grid(n: usize, width: Pixels, em: Pixels) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let usable = f32::from(width) - 16.;
    let cols = ((usable / (f32::from(em) * 10.)).floor() as usize).clamp(1, n);
    let rows = n.div_ceil(cols);
    // Fill every column the rows need (no trailing empty columns).
    (n.div_ceil(rows), rows)
}

pub(crate) fn prompt_label(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::Command => ":",
        PromptKind::Search(k) => k.label(),
    }
}

/// Case-insensitive literal matches (capped).
pub(crate) fn find_matches(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let re = regex::RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build();
    match re {
        Ok(re) => re.find_iter(text).take(10_000).map(|m| m.range()).collect(),
        Err(_) => Vec::new(),
    }
}

fn word_at(doc: &fuchico_helix::Text, pos: usize) -> (usize, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut a = pos;
    while a > 0 && doc.char_at(a - 1).is_some_and(is_word) {
        a -= 1;
    }
    let mut b = pos;
    while doc.char_at(b).is_some_and(is_word) {
        b += 1;
    }
    if a == b {
        (pos, (pos + 1).min(doc.len()))
    } else {
        (a, b)
    }
}

const NAMED: &[&str] = &[
    "escape",
    "enter",
    "backspace",
    "delete",
    "tab",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "insert",
];

/// Printable text of a keystroke without ctrl/alt/cmd (typing).
pub(crate) fn typed_char(ks: &Keystroke) -> Option<String> {
    let m = ks.modifiers;
    if m.control || m.alt || m.platform || m.function {
        return None;
    }
    if NAMED.contains(&ks.key.as_str())
        || (ks.key.starts_with('f') && ks.key.len() > 1 && ks.key[1..].parse::<u8>().is_ok())
    {
        return None;
    }
    let s = ks
        .key_char
        .clone()
        .or_else(|| (ks.key.chars().count() == 1).then(|| ks.key.clone()))?;
    (!s.is_empty() && !s.chars().any(char::is_control)).then_some(s)
}

/// gpui keystroke → fuchico-helix key event.
pub(crate) fn to_helix(ks: &Keystroke) -> Option<KeyEvent> {
    let m = ks.modifiers;
    let mods = HxMods {
        ctrl: m.control,
        alt: m.alt,
        shift: m.shift,
        meta: false,
    };
    let key = match ks.key.as_str() {
        "escape" => Key::Escape,
        "enter" => Key::Enter,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "tab" => Key::Tab,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "space" => Key::Char(' '),
        k if k.starts_with('f') && k.len() > 1 && k[1..].parse::<u8>().is_ok() => {
            Key::F(k[1..].parse().ok()?)
        }
        k => {
            if !m.control
                && !m.alt
                && let Some(s) = &ks.key_char
                && let Some(c) = s.chars().next()
            {
                Key::Char(c)
            } else {
                let mut cs = k.chars();
                let c = cs.next()?;
                if cs.next().is_some() {
                    return Some(KeyEvent::new(Key::Other(k.to_string()), mods));
                }
                Key::Char(if m.shift { shifted(c) } else { c })
            }
        }
    };
    Some(KeyEvent::new(key, mods))
}

/// The character Shift produces for `c` on a US layout (`;` -> `:`, `1` ->
/// `!`, `a` -> `A`), for modified keys whose `key_char` is not reported.
fn shifted(c: char) -> char {
    const PAIRS: &[(char, char)] = &[
        ('`', '~'),
        ('1', '!'),
        ('2', '@'),
        ('3', '#'),
        ('4', '$'),
        ('5', '%'),
        ('6', '^'),
        ('7', '&'),
        ('8', '*'),
        ('9', '('),
        ('0', ')'),
        ('-', '_'),
        ('=', '+'),
        ('[', '{'),
        (']', '}'),
        ('\\', '|'),
        (';', ':'),
        ('\'', '"'),
        (',', '<'),
        ('.', '>'),
        ('/', '?'),
    ];
    PAIRS
        .iter()
        .find(|(k, _)| *k == c)
        .map(|(_, s)| *s)
        .unwrap_or_else(|| c.to_ascii_uppercase())
}

#[cfg(test)]
mod key_tests {
    use super::*;

    fn ev(s: &str) -> KeyEvent {
        to_helix(&Keystroke::parse(s).unwrap()).unwrap()
    }

    #[test]
    fn alt_shift_punctuation_maps_to_the_shifted_char() {
        assert_eq!(ev("alt-shift-;").key, Key::Char(':'));
        assert!(ev("alt-shift-;").modifiers.alt);
        assert_eq!(ev("alt-;").key, Key::Char(';'));
        assert_eq!(ev("alt-shift-1").key, Key::Char('!'));
        assert_eq!(ev("ctrl-shift-a").key, Key::Char('A'));
        assert_eq!(ev("alt-shift-,").key, Key::Char('<'));
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let st = self.style.clone();
        let mono: SharedString = st.mono_family.clone();
        let sans: SharedString = st.sans_family.clone();
        let fs = st.font_size;
        let lh = st.line_height_px();
        let mut root = div()
            .id("fuchico-editor")
            .key_context(crate::KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(st.background)
            .text_color(st.foreground)
            .font_family(mono.clone())
            .text_size(fs)
            .on_action(cx.listener(Self::act_toggle_checkbox))
            .on_action(cx.listener(Self::act_insert_date))
            .on_action(cx.listener(Self::act_insert_datetime))
            .on_action(cx.listener(Self::act_insert_region))
            .on_action(cx.listener(Self::act_insert_table))
            .on_action(cx.listener(Self::act_toggle_task))
            .on_action(cx.listener(Self::act_pick_due))
            .on_action(cx.listener(Self::act_pick_recur))
            .on_action(cx.listener(Self::act_outline))
            .on_action(cx.listener(Self::act_goto))
            .on_action(cx.listener(Self::act_find))
            .on_action(cx.listener(Self::act_save))
            .on_action(cx.listener(Self::act_zoom_in))
            .on_action(cx.listener(Self::act_zoom_out))
            .on_action(cx.listener(Self::act_zoom_reset))
            .on_action(cx.listener(Self::act_fold))
            .on_action(cx.listener(Self::act_unfold))
            .on_action(cx.listener(Self::act_fold_all))
            .on_action(cx.listener(Self::act_unfold_all))
            .on_action(cx.listener(Self::act_copy))
            .on_action(cx.listener(Self::act_cut))
            .on_action(cx.listener(Self::act_paste))
            .on_action(cx.listener(Self::act_tab))
            .on_action(cx.listener(Self::act_backtab))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.undo(false, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.undo(true, cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(EditorElement { view: cx.entity() });

        let stop = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation();
        let bottom_bar = matches!(self.overlay, Overlay::Prompt(_));

        // Mode chip (bottom-left, over the gutter).
        if self.options.mode_chip && self.options.engine == EngineKind::Helix && !bottom_bar {
            root = root.child(
                div()
                    .id("fuchico-editor-mode-chip")
                    .absolute()
                    // `.helix-mode-chip`: bottom/left 12px, padding 2px 7px,
                    // radius-md (8px), sans 10px semibold, uppercase.
                    .left(px(12.))
                    .bottom(px(12.))
                    .px(px(7.))
                    .py(px(2.))
                    .rounded(px(8.))
                    .shadow(vec![gpui_kit::BoxShadow {
                        color: st.foreground.opacity(0.08),
                        offset: gpui_kit::point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .border_1()
                    .border_color(st.chip_border())
                    .bg(st.chip_background())
                    .text_color(st.chip_text())
                    .font_family(st.sans_family.clone())
                    .text_size(px(10.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(format!("{}{}", self.chip_label(), {
                        let p = self.pending_status();
                        if p.is_empty() {
                            String::new()
                        } else {
                            format!(" {p}")
                        }
                    })),
            );
        }
        if let Some((msg, err)) = &self.message
            && !bottom_bar
        {
            root = root.child(
                div()
                    .id("fuchico-editor-message")
                    .absolute()
                    .left(px(84.))
                    .bottom(px(4.))
                    .text_size(px(11.))
                    .text_color(if *err {
                        st.destructive
                    } else {
                        st.muted_foreground
                    })
                    .child(msg.clone()),
            );
        }

        match &self.overlay {
            Overlay::Prompt(p) => {
                let label = prompt_label(p.kind);
                let shown = if p.value.is_empty() {
                    p.placeholder.clone().unwrap_or_default()
                } else {
                    p.value.clone()
                };
                let ghost = p.value.is_empty();
                // `.cm-hx-command-autocomplete`: every option, in a grid of
                // `minmax(10em, 1fr)` columns filled column by column
                // (`positionPopup` reorders them), no row cap.
                let width = self
                    .frame
                    .as_ref()
                    .map_or(px(600.), |f| f.bounds.size.width);
                let options = &p.completions.options;
                let (cols, rows) = prompt_grid(options.len(), width, st.font_size);
                let mut list = div().flex().flex_row().px(px(8.));
                for c in 0..cols {
                    let mut col = div().flex().flex_col().flex_1().min_w(st.font_size * 10.);
                    for i in (c * rows..(c + 1) * rows).take_while(|&i| i < options.len()) {
                        let sel = p.selected == Some(i);
                        col = col.child(
                            div()
                                .mb(px(1.))
                                .when_sel(sel, st.accent, st.accent_foreground)
                                .child(options[i].clone()),
                        );
                    }
                    list = list.child(col);
                }
                let mut bar = div()
                    .id("fuchico-editor-prompt")
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .flex()
                    .flex_col()
                    .bg(st.card)
                    .border_t_1()
                    .border_color(st.border)
                    .on_mouse_down(MouseButton::Left, stop);
                // `.cm-hx-command-popup`: help above the options.
                if let Some(h) = &p.completions.help {
                    bar = bar.child(
                        div()
                            .px(px(8.))
                            .text_size(px(11.))
                            .text_color(st.muted_foreground)
                            .child(h.clone()),
                    );
                }
                if !p.completions.options.is_empty() {
                    bar = bar.child(list);
                }
                bar = bar.child(
                    div()
                        .flex()
                        .items_center()
                        .h(lh + px(6.))
                        .px(px(8.))
                        .gap(px(6.))
                        .child(div().text_color(st.muted_foreground).child(label))
                        .child(
                            div()
                                .flex()
                                .child(
                                    div()
                                        .text_color(if ghost {
                                            st.muted_foreground
                                        } else {
                                            st.foreground
                                        })
                                        .child(shown),
                                )
                                .child(div().w(px(1.5)).h(lh).bg(st.foreground)),
                        ),
                );
                root = root.child(bar);
            }
            Overlay::Find(f) => {
                let count = if f.matches.is_empty() {
                    if f.query.is_empty() {
                        String::new()
                    } else {
                        "No results".into()
                    }
                } else {
                    format!("{} of {}", f.current + 1, f.matches.len())
                };
                root = root.child(
                    div()
                        .id("fuchico-editor-find")
                        .absolute()
                        .top(px(6.))
                        .right(px(16.))
                        .w(px(320.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(4.))
                        .bg(st.card)
                        .border_1()
                        .border_color(st.border)
                        .rounded(st.radius_sm)
                        .font_family(sans.clone())
                        .text_size(px(12.))
                        .on_mouse_down(MouseButton::Left, stop)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .px(px(6.))
                                .border_1()
                                .border_color(st.input)
                                .rounded(px(4.))
                                .bg(st.background)
                                .child(div().font_family(mono.clone()).child(f.query.clone()))
                                .child(div().w(px(1.5)).h(px(16.)).bg(st.foreground)),
                        )
                        .child(div().text_color(st.muted_foreground).child(count)),
                );
            }
            Overlay::Outline(o) => {
                let mut list = div().flex().flex_col().py(px(4.));
                for (row, &i) in o.filtered.iter().enumerate().take(14) {
                    let h = &o.items[i];
                    let sel = row == o.selected;
                    list = list.child(
                        div()
                            .id(("fuchico-editor-outline-item", row))
                            .flex()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(3.))
                            .pl(px(10. + 12. * (h.level.saturating_sub(1)) as f32))
                            .when_sel(sel, st.accent, st.accent_foreground)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |v, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    v.outline_pick(row, cx);
                                }),
                            )
                            .child(
                                div()
                                    .text_color(st.muted_foreground)
                                    .child(format!("H{}", h.level)),
                            )
                            .child(h.text.clone()),
                    );
                }
                if o.filtered.is_empty() {
                    list = list.child(
                        div()
                            .px(px(10.))
                            .text_color(st.muted_foreground)
                            .child("No headings"),
                    );
                }
                root = root.child(
                    div()
                        .absolute()
                        .top(px(40.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("fuchico-editor-outline")
                                .w(px(480.))
                                .flex()
                                .flex_col()
                                .bg(st.card)
                                .border_1()
                                .border_color(st.border)
                                .rounded(st.radius_md)
                                .shadow_lg()
                                .font_family(sans.clone())
                                .text_size(px(13.))
                                .on_mouse_down(MouseButton::Left, stop)
                                .child(
                                    div()
                                        .flex()
                                        .px(px(10.))
                                        .py(px(8.))
                                        .border_b_1()
                                        .border_color(st.border)
                                        .child(if o.query.is_empty() {
                                            div()
                                                .text_color(st.muted_foreground)
                                                .child("Go to header…")
                                        } else {
                                            div().child(o.query.clone())
                                        })
                                        .child(div().w(px(1.5)).h(px(16.)).bg(st.foreground)),
                                )
                                .child(list),
                        ),
                );
            }
            Overlay::None => {}
        }

        // Completion popup under the main cursor.
        if let Some(c) = &self.completion
            && let Some(at) = self.frame.as_ref().and_then(|f| f.cursor_anchor)
        {
            let origin = self
                .frame
                .as_ref()
                .map(|f| f.bounds.origin)
                .unwrap_or_default();
            let mut list = div().flex().flex_col().py(px(2.));
            let first = c.selected.saturating_sub(9);
            for (i, o) in c.options.iter().enumerate().skip(first).take(10) {
                let sel = i == c.selected;
                let mut row = div()
                    .id(("fuchico-editor-completion", i))
                    .flex()
                    .justify_between()
                    .gap(px(12.))
                    .px(px(8.))
                    .when_sel(sel, st.accent, st.accent_foreground)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |v, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            v.accept_completion(i, cx);
                        }),
                    )
                    .child(o.label.clone());
                if let Some(d) = &o.detail {
                    row = row.child(
                        div()
                            .text_color(st.muted_foreground)
                            .text_size(px(11.))
                            .child(d.clone()),
                    );
                }
                list = list.child(row);
            }
            root = root.child(
                div()
                    .id("fuchico-editor-completions")
                    .absolute()
                    .left(at.x - origin.x)
                    .top(at.y - origin.y)
                    .min_w(px(200.))
                    .max_w(px(420.))
                    .bg(st.card)
                    .border_1()
                    .border_color(st.border)
                    .rounded(st.radius_sm)
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, stop)
                    .child(list),
            );
        }

        if let Some(m) = &self.lang_menu {
            let origin = self
                .frame
                .as_ref()
                .map(|f| f.bounds.origin)
                .unwrap_or_default();
            let mut list = div().flex().flex_col().py(px(2.));
            for (i, o) in m.toolbar.options.iter().enumerate() {
                list = list.child(
                    div()
                        .id(("fuchico-editor-lang", i))
                        .px(px(8.))
                        .when_sel(i == m.selected, st.accent, st.accent_foreground)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |v, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                v.pick_language(i, cx);
                            }),
                        )
                        .child(if o.is_empty() {
                            "plain".to_string()
                        } else {
                            o.clone()
                        }),
                );
            }
            root = root.child(
                div()
                    .id("fuchico-editor-lang-menu")
                    .absolute()
                    .left(m.at.x - origin.x)
                    .top(m.at.y - origin.y + px(14.))
                    .max_h(px(260.))
                    .overflow_hidden()
                    .bg(st.card)
                    .border_1()
                    .border_color(st.border)
                    .rounded(st.radius_sm)
                    .text_size(px(11.))
                    .on_mouse_down(MouseButton::Left, stop)
                    .child(list),
            );
        }
        let _ = (window, mix);
        root
    }
}

/// Selected-row colours helper for overlay lists.
trait WhenSel: Styled + Sized {
    fn when_sel(self, sel: bool, bg: gpui_kit::Hsla, fg: gpui_kit::Hsla) -> Self {
        if sel {
            self.bg(bg).text_color(fg)
        } else {
            self
        }
    }
}
impl<T: Styled> WhenSel for T {}
