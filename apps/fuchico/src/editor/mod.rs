//! Markdown editor pane (parity §4 "Editor"): the `fuchico-editor`
//! [`EditorView`] hosted as a [`PaneItem`].
//!
//! * Load: `fs_read_file` off the UI thread. Text mounts the editor; binary
//!   and too-large files show "Binary file · 12.0 KB" / "File too large · …"
//!   (an image preview for image extensions); an IO error its message.
//! * Save: `:w` / Mod-s / the palette, plus an 800 ms trailing autosave
//!   after each edit. Saving an unchanged buffer is a no-op. Writes happen
//!   off-thread with `source: "editor"`; a failure toasts and keeps the
//!   tab dirty. Pending edits are flushed when the pane goes away (tab
//!   switch / close / window close) and on app quit — a deliberate
//!   improvement over the original, which dropped them.
//! * External changes (`fs:file-written` from another surface, `fs:changed`
//!   for the folder): a clean buffer reloads; a dirty one shows the
//!   conflict banner ("Reload from disk" / "Keep my version").
//! * Settings and theme apply live (engine, line numbers, tab size, rulers,
//!   fold markers, font family / size, colours); the rebindable shortcuts
//!   are re-bound by [`sync_editor_shortcuts`].
//! * Scroll offsets persist per absolute path (`helix.scrollPositions`).
//! * Navigation: wikilinks / relative links → `OpenFile`, external links →
//!   the browser, tags → the tags view, `:` → the command palette (Helix,
//!   main window), Helix `Space f/b`, `gn/gp`, `Space /`, `:bc`.
//! * Mermaid previews go through the [`crate::mermaid`] tracker (dock in
//!   the main window, a diagram window from an editor window).
//! * Pasted / dropped images are saved under `<root>/attachments`.

#[cfg(test)]
mod ui_tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fuchico_core::attachments::{image_link, save_image};
use fuchico_core::doc_stats::{DocStats, compute_doc_stats};
use fuchico_core::events::CoreEvent;
use fuchico_core::fs::file::{ReadResult, fs_read_file, fs_write_file};
use fuchico_editor::{
    EditorEvent, EditorOptions, EditorStyle, EditorView, EngineKind, LineNumbers, MermaidBlock, ZoomDirection,
};
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, Global,
    InteractiveElement, IntoElement, ObjectFit, ParentElement, Render, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Task, WeakEntity, Window, div, img, px,
};

use crate::bus::{self, AppEvent};
use crate::settings::ui_state::UiState;
use crate::settings::{KeybindingMode, Settings, SettingsStore};
use crate::t;
use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::pane::{PaneContext, PaneEvent, PaneItem};
use crate::workspace::status_bar::StatusBar;

/// Trailing autosave debounce (`useDocument`'s 800 ms).
pub const AUTOSAVE_MS: u64 = 800;
/// Status-bar word count debounce.
const STATS_MS: u64 = 200;
/// Vault file / tag index refresh debounce after a file-system event.
const VAULT_REFRESH_MS: u64 = 500;

const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico"];

/// App-wide setup: the editor crate's key bindings, the rebindable
/// shortcuts (kept in sync with settings) and the export buffer provider.
pub fn init(cx: &mut App) {
    fuchico_editor::init(cx);
    sync_editor_shortcuts(cx);
    cx.observe_global::<SettingsStore>(sync_editor_shortcuts).detach();
    crate::export::set_buffer_provider(cx, |path, cx| {
        let open = cx.try_global::<OpenBuffers>()?;
        let view = open.0.get(path)?.upgrade()?;
        Some(view.read(cx).text())
    });
}

/// Editors by path, for the export buffer provider (exports include
/// unsaved edits).
#[derive(Default)]
struct OpenBuffers(HashMap<String, WeakEntity<EditorView>>);
impl Global for OpenBuffers {}

fn register_buffer(path: &str, view: &Entity<EditorView>, cx: &mut App) {
    let open = cx.default_global::<OpenBuffers>();
    open.0.retain(|_, v| v.upgrade().is_some());
    open.0.insert(path.to_string(), view.downgrade());
}

fn unregister_buffer(path: &str, view: Option<&Entity<EditorView>>, cx: &mut App) {
    if let Some(open) = cx.try_global::<OpenBuffers>()
        && open.0.get(path).is_some_and(|w| view.is_none_or(|v| w.entity_id() == v.entity_id()))
    {
        cx.global_mut::<OpenBuffers>().0.remove(path);
    }
}

// ---- rebindable shortcuts ---------------------------------------------------

/// The keystrokes each rebindable editor action is currently bound to.
struct BoundShortcuts(HashMap<&'static str, Vec<String>>);
impl Global for BoundShortcuts {}

fn editor_binding(ks: &str, action: Box<dyn gpui_kit::Action>) -> Option<gpui_kit::KeyBinding> {
    let predicate = gpui_kit::KeyBindingContextPredicate::parse(fuchico_editor::KEY_CONTEXT).ok()?;
    gpui_kit::KeyBinding::load(ks, action, Some(predicate.into()), false, None, &gpui_kit::DummyKeyboardMapper).ok()
}

/// Re-binds the `SHORTCUT_ACTIONS` whose binding changed in settings:
/// the old keystrokes get an `Unbind` and the new ones a binding in the
/// editor context (`fuchico_editor::init` bound the defaults).
pub fn sync_editor_shortcuts(cx: &mut App) {
    let settings = SettingsStore::get(cx);
    if !cx.has_global::<BoundShortcuts>() {
        let def = Settings::default();
        let bound = crate::keymap::SHORTCUT_ACTIONS
            .iter()
            .map(|a| (a.id, crate::keymap::binding_to_keystrokes(def.shortcut(a.id))))
            .collect();
        cx.set_global(BoundShortcuts(bound));
    }
    let mut bindings = Vec::new();
    let mut changed = Vec::new();
    for a in crate::keymap::SHORTCUT_ACTIONS {
        let want = crate::keymap::binding_to_keystrokes(settings.shortcut(a.id));
        let have = cx.global::<BoundShortcuts>().0.get(a.id).cloned().unwrap_or_default();
        if want == have {
            continue;
        }
        let Some(action) = crate::actions::editor_action_for(a.id) else { continue };
        let name = gpui_kit::SharedString::from(action.name());
        bindings.extend(have.iter().filter_map(|ks| editor_binding(ks, Box::new(gpui_kit::Unbind(name.clone())))));
        bindings.extend(want.iter().filter_map(|ks| editor_binding(ks, action.boxed_clone())));
        changed.push((a.id, want));
    }
    if !bindings.is_empty() {
        cx.bind_keys(bindings);
    }
    for (id, want) in changed {
        cx.global_mut::<BoundShortcuts>().0.insert(id, want);
    }
}

// ---- settings / theme mapping -------------------------------------------------

/// The engine for a keybinding mode.
// TODO(vim): `fuchico-vim` (branch wave3/vim) plugs in as `EngineKind::Vim`;
// until it lands Vim mode drives the modal Helix engine without the Helix
// extras (mode chip, `:` palette).
pub fn engine_for(mode: KeybindingMode) -> EngineKind {
    match mode {
        KeybindingMode::Helix | KeybindingMode::Vim => EngineKind::Helix,
        KeybindingMode::Normal => EngineKind::Normal,
    }
}

/// `EditorOptions` from the settings and the pane context. `vault` is the
/// current file / tag index.
pub fn options_for(s: &Settings, path: &str, pcx: &PaneContext, vault: (Vec<String>, Vec<String>)) -> EditorOptions {
    let helix = s.keybinding_mode == KeybindingMode::Helix;
    let main = !pcx.in_window;
    EditorOptions {
        line_numbers: if s.relative_line_numbers { LineNumbers::Relative } else { LineNumbers::Absolute },
        rulers: s.rulers.clone(),
        vault_files: vault.0,
        all_tags: vault.1,
        path: Some(path.to_string()),
        command_line_via_host: helix && main,
        fold_start_marker: s.fold_start_marker.clone(),
        fold_end_marker: s.fold_end_marker.clone(),
        inline_images: false,
        vault_root: pcx.root.as_ref().map(PathBuf::from),
        mode_chip: helix,
        externals: fuchico_helix::ExternalCommands {
            file_picker: main,
            buffer_picker: main,
            buffer_next: main,
            buffer_previous: main,
            buffer_close: true,
            global_search: main,
        },
        tab_size: s.tab_size.max(1),
        engine: engine_for(s.keybinding_mode),
        ..EditorOptions::default()
    }
}

/// `EditorStyle` from the app theme (same token names).
pub fn style_for(th: &AppTheme) -> EditorStyle {
    let c = &th.colors;
    macro_rules! style {
        ($($f:ident),+) => {
            EditorStyle {
                dark: th.appearance.is_dark(),
                mono_family: th.mono_font.clone(),
                sans_family: th.ui_font.clone(),
                font_size: px(th.editor_font_size),
                line_height: 1.55,
                radius_sm: px(th.radius.sm),
                radius_md: px(th.radius.md),
                $($f: c.$f,)+
            }
        };
    }
    style!(
        background,
        foreground,
        card,
        card_foreground,
        primary,
        primary_foreground,
        secondary,
        secondary_foreground,
        muted,
        muted_foreground,
        accent,
        accent_foreground,
        destructive,
        border,
        input,
        ring,
        syntax_keyword,
        syntax_string,
        syntax_number,
        syntax_function,
        syntax_type,
        syntax_comment
    )
}

/// `formatBytes`: `512 B`, `1.5 KB`, `2.0 MB`.
pub fn format_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.)
    } else {
        format!("{:.1} MB", n as f64 / 1024. / 1024.)
    }
}

fn extension(path: &str) -> Option<String> {
    Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_lowercase)
}

fn is_image_path(path: &str) -> bool {
    extension(path).is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
}

fn image_mime(path: &str) -> &'static str {
    match extension(path).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        _ => "",
    }
}

/// Parent folder strings an `fs:changed {dir}` may use for this file (as
/// given and canonicalized). Blocking; call off the UI thread.
fn parent_dirs(path: &str) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(p) = Path::new(path).parent() {
        v.push(p.to_string_lossy().into_owned());
        if let Ok(c) = std::fs::canonicalize(p) {
            let c = c.to_string_lossy().into_owned();
            if !v.contains(&c) {
                v.push(c);
            }
        }
    }
    v
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Markdown files and tags of the vault (wikilink resolution and
/// completion). Blocking.
fn scan_vault(root: &str) -> (Vec<String>, Vec<String>) {
    let files = fuchico_core::fs::tree::fs_list_markdown_files(root.to_string()).unwrap_or_default();
    let tags = fuchico_core::tags::tags_scan(root.to_string())
        .map(|t| t.into_iter().map(|e| e.tag).collect())
        .unwrap_or_default();
    (files, tags)
}

/// Char range of 1-based `line` in `text` (clamped to the last line).
fn line_range(text: &str, line: u32) -> (usize, usize) {
    let idx = (line as usize).max(1) - 1;
    let lines: Vec<&str> = text.split('\n').collect();
    let idx = idx.min(lines.len() - 1);
    let start: usize = lines[..idx].iter().map(|l| l.chars().count() + 1).sum();
    (start, start + lines[idx].chars().count())
}

// ---- the pane -----------------------------------------------------------------

enum State {
    Loading,
    Failed(String),
    /// Binary or too large, with its size in bytes.
    NotText { size: u64, too_large: bool },
    Ready(Entity<EditorView>),
}

pub struct EditorPane {
    path: String,
    pcx: PaneContext,
    state: State,
    dirs: Vec<String>,
    focus: FocusHandle,
    stats: DocStats,
    /// The file's content as last read or written by us.
    disk_text: String,
    /// Mirrors the tab's dirty dot (buffer dirty, or the last save failed).
    dirty: bool,
    save_failed: bool,
    /// The conflict banner is shown.
    conflict: bool,
    /// A reload read is in flight.
    reloading: bool,
    /// `reveal_line` before the file finished loading.
    pending_line: Option<u32>,
    /// Editor mermaid keys → tracker `blockKey`s.
    mermaid: HashMap<u64, String>,
    vault: (Vec<String>, Vec<String>),
    autosave: Option<Task<()>>,
    stats_task: Option<Task<()>>,
    vault_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _view_subs: Vec<Subscription>,
}

impl EditorPane {
    pub fn new(path: String, pcx: PaneContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = path.clone();
        let read = cx.background_spawn(async move { (fs_read_file(p.clone()), parent_dirs(&p)) });
        cx.spawn_in(window, async move |this, cx| {
            let (res, dirs) = read.await;
            this.update_in(cx, |this, window, cx| {
                this.dirs = dirs;
                match res {
                    Ok(ReadResult::Text { content, .. }) => this.mount(content, window, cx),
                    Ok(ReadResult::Binary { size }) => this.state = State::NotText { size, too_large: false },
                    Ok(ReadResult::TooLarge { size, .. }) => this.state = State::NotText { size, too_large: true },
                    Err(e) => this.state = State::Failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        let subs = vec![
            bus::subscribe_in(window, cx, |this: &mut Self, ev, _, cx| this.on_bus(ev, cx)),
            cx.observe_global::<SettingsStore>(|this: &mut Self, cx| this.apply_settings(cx)),
            cx.observe_global::<AppTheme>(|this: &mut Self, cx| {
                if let State::Ready(v) = &this.state {
                    let style = style_for(AppTheme::get(cx));
                    v.update(cx, |v, cx| {
                        if *v.style() != style {
                            v.set_style(style, cx)
                        }
                    });
                }
            }),
            cx.on_release(|this: &mut Self, cx: &mut App| this.teardown(cx)),
            cx.on_app_quit(|this: &mut Self, cx| {
                let task = this.flush(cx);
                this.persist_scroll(cx);
                async move {
                    if let Some(t) = task {
                        t.await;
                    }
                }
            }),
        ];
        let dirs = Path::new(&path).parent().map(|p| vec![p.to_string_lossy().into_owned()]).unwrap_or_default();
        let mut pane = EditorPane {
            path,
            pcx,
            state: State::Loading,
            dirs,
            focus: cx.focus_handle(),
            stats: DocStats { words: 0, reading_time_min: 0 },
            disk_text: String::new(),
            dirty: false,
            save_failed: false,
            conflict: false,
            reloading: false,
            pending_line: None,
            mermaid: HashMap::new(),
            vault: (Vec::new(), Vec::new()),
            autosave: None,
            stats_task: None,
            vault_task: None,
            _subs: subs,
            _view_subs: Vec::new(),
        };
        pane.refresh_vault(Duration::ZERO, cx);
        pane
    }

    fn mount(&mut self, content: String, window: &mut Window, cx: &mut Context<Self>) {
        let settings = SettingsStore::get(cx);
        let options = options_for(&settings, &self.path, &self.pcx, self.vault.clone());
        let style = style_for(AppTheme::get(cx));
        let had_focus = self.focus.is_focused(window);
        let view = cx.new(|cx| EditorView::new(&content, options, style, window, cx));
        self.disk_text = view.read(cx).text();
        self.stats = compute_doc_stats(&self.disk_text);
        self._view_subs = vec![cx.subscribe_in(&view, window, |this, _, ev, window, cx| this.on_editor(ev, window, cx))];
        register_buffer(&self.path, &view, cx);
        if let Some(line) = self.pending_line.take() {
            Self::focus_line(&view, line, cx);
        } else if let Some(y) = cx.try_global::<UiState>().and_then(|s| s.scroll.get(&self.path)) {
            view.update(cx, |v, cx| v.set_scroll_position(y as f32, cx));
        }
        if had_focus {
            let h = view.read(cx).focus_handle(cx);
            window.focus(&h, cx);
        }
        self.state = State::Ready(view);
    }

    pub fn view(&self) -> Option<&Entity<EditorView>> {
        match &self.state {
            State::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// `focusLine`: centres 1-based `line` and selects all of it.
    fn focus_line(view: &Entity<EditorView>, line: u32, cx: &mut App) {
        view.update(cx, |v, cx| {
            v.reveal_line(line as usize, cx);
            let (from, to) = line_range(&v.text(), line);
            v.set_selection(from, to, cx);
        });
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let State::Ready(v) = &self.state else { return };
        let s = SettingsStore::get(cx);
        let options = options_for(&s, &self.path, &self.pcx, self.vault.clone());
        v.update(cx, |v, cx| {
            if *v.options() != options {
                v.set_options(options, cx);
            }
        });
    }

    /// Re-indexes the vault's files and tags after `delay`.
    fn refresh_vault(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(root) = self.pcx.root.clone() else { return };
        self.vault_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let vault = cx.background_spawn(async move { scan_vault(&root) }).await;
            this.update(cx, |this, cx| {
                if this.vault == vault {
                    return;
                }
                this.vault = vault.clone();
                if let State::Ready(v) = &this.state {
                    v.update(cx, |v, cx| v.set_vault(vault.0, vault.1, cx));
                }
            })
            .ok();
        }));
    }

    // ---- editor events ------------------------------------------------------

    fn on_editor(&mut self, ev: &EditorEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            EditorEvent::Changed { dirty } => {
                if *dirty {
                    self.save_failed = false;
                    self.schedule_autosave(cx);
                }
                self.schedule_stats(cx);
            }
            EditorEvent::DirtyChanged(d) => self.set_dirty(*d, cx),
            EditorEvent::Save => self.save_now(cx),
            EditorEvent::Quit => cx.emit(PaneEvent::Close),
            EditorEvent::OpenWikilink { resolved, target } => match resolved {
                Some(p) => cx.emit(PaneEvent::OpenFile { path: p.clone(), line: None }),
                None => log::debug!("editor: unresolved wikilink [[{target}]]"),
            },
            EditorEvent::OpenLink(href) => {
                if fuchico_core::wikilinks::is_external_link(href) {
                    cx.open_url(href);
                } else if let Some(p) = fuchico_core::wikilinks::resolve_relative_markdown_link(href, &self.path) {
                    cx.emit(PaneEvent::OpenFile { path: p, line: None });
                }
            }
            EditorEvent::TagClicked(tag) => cx.emit(PaneEvent::OpenTag(tag.clone())),
            EditorEvent::OpenCommandLine { .. } => cx.emit(PaneEvent::OpenCommandPalette(":".into())),
            EditorEvent::External(ext) => {
                use fuchico_helix::External as X;
                cx.emit(match ext {
                    X::FilePicker | X::BufferPicker => PaneEvent::QuickOpen,
                    X::BufferNext => PaneEvent::CycleTab(1),
                    X::BufferPrevious => PaneEvent::CycleTab(-1),
                    X::BufferClose(_) => PaneEvent::Close,
                    X::GlobalSearch(q) => PaneEvent::GlobalSearch(q.clone()),
                });
            }
            EditorEvent::MermaidOpen(block) => self.mermaid_open(block, cx),
            EditorEvent::MermaidUpdate(MermaidBlock { key, source }) => {
                if let Some(bk) = self.mermaid.get(key) {
                    crate::mermaid::update_block(bk, Some(source.clone()), cx);
                }
            }
            EditorEvent::MermaidClose { key } => {
                if let Some(bk) = self.mermaid.get(key) {
                    crate::mermaid::update_block(bk, None, cx);
                }
            }
            EditorEvent::PasteImage { mime, bytes } => {
                self.save_images(vec![(String::new(), mime.clone(), bytes.clone())], cx)
            }
            EditorEvent::Zoom(dir) => crate::windows::zoom_font(
                cx,
                match dir {
                    ZoomDirection::In => Some(1.),
                    ZoomDirection::Out => Some(-1.),
                    ZoomDirection::Reset => None,
                },
            ),
            // Shown by the view itself (message line).
            EditorEvent::Message { .. }
            | EditorEvent::ChangeTheme(_)
            | EditorEvent::HostCommand { .. }
            | EditorEvent::ModeChanged(_) => {}
        }
    }

    fn mermaid_open(&mut self, block: &MermaidBlock, cx: &mut Context<Self>) {
        let bk = match self.mermaid.get(&block.key) {
            Some(k) if crate::mermaid::is_tracked(k, cx) => k.clone(),
            _ => {
                let k = crate::mermaid::new_block_key();
                self.mermaid.insert(block.key, k.clone());
                k
            }
        };
        crate::mermaid::open_block(&bk, &block.source, cx);
        if self.pcx.in_window {
            crate::mermaid::window::open_mermaid_window(bk, t!(AppDiagram), Some(block.source.clone()), cx);
        } else {
            cx.emit(PaneEvent::OpenMermaid { block_key: bk, initial_text: block.source.clone() });
        }
    }

    fn set_dirty(&mut self, d: bool, cx: &mut Context<Self>) {
        let d = d || self.save_failed;
        if self.dirty != d {
            self.dirty = d;
            cx.emit(PaneEvent::DirtyChanged(d));
            cx.notify();
        }
    }

    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        self.autosave = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(AUTOSAVE_MS)).await;
            this.update(cx, |this, cx| {
                this.autosave = None;
                this.write(cx);
            })
            .ok();
        }));
    }

    fn schedule_stats(&mut self, cx: &mut Context<Self>) {
        self.stats_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(STATS_MS)).await;
            let Ok(Some(text)) = this.update(cx, |this, cx| this.view().map(|v| v.read(cx).text())) else { return };
            let stats = cx.background_spawn(async move { compute_doc_stats(&text) }).await;
            this.update(cx, |this, cx| {
                this.stats = stats;
                cx.notify();
            })
            .ok();
        }));
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        self.autosave = None;
        self.write(cx);
    }

    /// Writes the buffer if it has unsaved edits (off-thread). The view is
    /// marked saved once the write lands, unless it changed meanwhile.
    fn write(&mut self, cx: &mut Context<Self>) {
        let State::Ready(v) = &self.state else { return };
        if !v.read(cx).is_dirty() && !self.save_failed {
            return;
        }
        let text = v.read(cx).text();
        let path = self.path.clone();
        let body = text.clone();
        let write = cx.background_spawn(async move { fs_write_file(path, body, Some("editor".into()), &*bus::sink()) });
        cx.spawn(async move |this, cx| {
            let res = write.await;
            this.update(cx, |this, cx| {
                let State::Ready(v) = &this.state else { return };
                let v = v.clone();
                match res {
                    Ok(_) => {
                        this.disk_text = text.clone();
                        this.save_failed = false;
                        this.conflict = false;
                        if v.read(cx).text() == text {
                            v.update(cx, |v, cx| v.mark_saved(cx));
                        }
                        let d = v.read(cx).is_dirty();
                        this.set_dirty(d, cx);
                    }
                    Err(e) => {
                        log::warn!("editor: save failed: {e}");
                        this.save_failed = true;
                        this.set_dirty(true, cx);
                        let name = crate::workspace::tabs::basename(&this.path).to_string();
                        let msg = t!(EditorSaveFailed, name = name, error = e);
                        if let Some(w) = cx.active_window() {
                            let _ = w.update(cx, |_, window, cx| crate::ui::toast::toast_error(window, cx, msg));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Starts writing pending edits (the pane is going away, so nothing
    /// waits on the result). Returns the write task.
    fn flush(&mut self, cx: &mut App) -> Option<Task<()>> {
        let State::Ready(v) = &self.state else { return None };
        let v = v.read(cx);
        if !v.is_dirty() && !self.save_failed {
            return None;
        }
        let (path, text) = (self.path.clone(), v.text());
        self.autosave = None;
        Some(cx.background_spawn(async move {
            if let Err(e) = fs_write_file(path, text, Some("editor".into()), &*bus::sink()) {
                log::warn!("editor: flush failed: {e}");
            }
        }))
    }

    fn persist_scroll(&self, cx: &mut App) {
        let State::Ready(v) = &self.state else { return };
        let y = v.read(cx).scroll_position() as f64;
        if cx.has_global::<UiState>() {
            UiState::set_scroll(cx, &self.path, y);
        }
    }

    /// The pane was dropped (tab switch / close / window close).
    fn teardown(&mut self, cx: &mut App) {
        if let Some(t) = self.flush(cx) {
            t.detach();
        }
        self.persist_scroll(cx);
        for bk in self.mermaid.values() {
            crate::mermaid::close_block(bk, cx);
        }
        let view = self.view().cloned();
        unregister_buffer(&self.path, view.as_ref(), cx);
    }

    // ---- external changes -----------------------------------------------------

    fn on_bus(&mut self, ev: &AppEvent, cx: &mut Context<Self>) {
        match ev {
            AppEvent::Core(core) => {
                let (ours, vault) = match core {
                    CoreEvent::FsChanged { dir, .. } => (self.dirs.iter().any(|d| same_dir(d, dir)), true),
                    CoreEvent::FileWritten { path, source } => {
                        (*path == self.path && source.as_deref() != Some("editor"), path.ends_with(".md"))
                    }
                    _ => (false, false),
                };
                if ours {
                    self.reload_from_disk(cx);
                }
                if vault {
                    self.refresh_vault(Duration::from_millis(VAULT_REFRESH_MS), cx);
                }
            }
            AppEvent::MermaidClosed { block_key } => {
                if let Some((&key, _)) = self.mermaid.iter().find(|(_, bk)| *bk == block_key)
                    && let State::Ready(v) = &self.state
                {
                    v.update(cx, |v, cx| v.close_mermaid(key, cx));
                }
            }
            AppEvent::GotoLine { path, line } if *path == self.path => match &self.state {
                State::Ready(v) => Self::focus_line(v, *line, cx),
                _ => self.pending_line = Some(*line),
            },
            _ => {}
        }
    }

    /// Re-reads the file: what we already know → nothing; clean buffer →
    /// replace; dirty → conflict banner.
    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.reloading || !matches!(self.state, State::Ready(_)) {
            return;
        }
        self.reloading = true;
        let p = self.path.clone();
        let read = cx.background_spawn(async move { fs_read_file(p) });
        cx.spawn(async move |this, cx| {
            let res = read.await;
            this.update(cx, |this, cx| {
                this.reloading = false;
                let Ok(ReadResult::Text { content, .. }) = res else { return };
                let State::Ready(v) = &this.state else { return };
                let v = v.clone();
                let normalized = content.replace("\r\n", "\n");
                if normalized == this.disk_text || normalized == v.read(cx).text() {
                    return;
                }
                if v.read(cx).is_dirty() {
                    this.conflict = true;
                } else {
                    v.update(cx, |v, cx| v.set_text(&content, cx));
                    this.disk_text = v.read(cx).text();
                    this.stats = compute_doc_stats(&this.disk_text);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Conflict banner: "Reload from disk".
    pub fn resolve_conflict_reload(&mut self, cx: &mut Context<Self>) {
        self.conflict = false;
        let p = self.path.clone();
        let read = cx.background_spawn(async move { fs_read_file(p) });
        cx.spawn(async move |this, cx| {
            let Ok(ReadResult::Text { content, .. }) = read.await else { return };
            this.update(cx, |this, cx| {
                let State::Ready(v) = &this.state else { return };
                let v = v.clone();
                this.autosave = None;
                this.save_failed = false;
                v.update(cx, |v, cx| v.set_text(&content, cx));
                this.disk_text = v.read(cx).text();
                this.stats = compute_doc_stats(&this.disk_text);
                this.set_dirty(false, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Conflict banner: "Keep my version".
    pub fn resolve_conflict_keep(&mut self, cx: &mut Context<Self>) {
        self.conflict = false;
        cx.notify();
    }

    pub fn has_conflict(&self) -> bool {
        self.conflict
    }

    // ---- images -------------------------------------------------------------

    /// Saves `(file_name, mime, bytes)` images into `<root>/attachments`
    /// and inserts their links. Nothing without a vault.
    fn save_images(&mut self, images: Vec<(String, String, Vec<u8>)>, cx: &mut Context<Self>) {
        let Some(root) = self.pcx.root.clone() else { return };
        let current = self.path.clone();
        let task = cx.background_spawn(async move {
            let dir = format!("{root}/attachments");
            let now = chrono::Local::now().naive_local();
            images
                .into_iter()
                .filter_map(|(name, mime, bytes)| match save_image(&bytes, &name, &mime, &dir, &now) {
                    Ok(saved) => Some(image_link(&saved, &current)),
                    Err(e) => {
                        log::warn!("editor: image save failed: {e}");
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let links = task.await;
            if links.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                if let State::Ready(v) = &this.state {
                    v.update(cx, |v, cx| v.insert_image_link(&links.join("\n"), cx));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Finder files dropped on the editor: images are copied into the
    /// attachments folder and linked.
    pub fn drop_external(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let files: Vec<PathBuf> = paths.iter().filter(|p| is_image_path(&p.to_string_lossy())).cloned().collect();
        if files.is_empty() || self.pcx.root.is_none() {
            return;
        }
        let read = cx.background_spawn(async move {
            files
                .into_iter()
                .filter_map(|p| {
                    let bytes = std::fs::read(&p).ok()?;
                    let name = p.file_name()?.to_string_lossy().into_owned();
                    Some((name, image_mime(&p.to_string_lossy()).to_string(), bytes))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let images = read.await;
            this.update(cx, |this, cx| this.save_images(images, cx)).ok();
        })
        .detach();
    }
}

impl EventEmitter<PaneEvent> for EditorPane {}

impl Focusable for EditorPane {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.state {
            State::Ready(v) => v.read(cx).focus_handle(cx),
            _ => self.focus.clone(),
        }
    }
}

impl PaneItem for EditorPane {
    fn path(&self) -> &str {
        &self.path
    }

    fn set_path(&mut self, path: String, cx: &mut Context<Self>) {
        let view = self.view().cloned();
        unregister_buffer(&self.path, view.as_ref(), cx);
        if let Some(v) = &view {
            register_buffer(&path, v, cx);
        }
        self.dirs = Path::new(&path).parent().map(|p| vec![p.to_string_lossy().into_owned()]).unwrap_or_default();
        self.path = path;
        self.apply_settings(cx);
        cx.notify();
    }

    fn reveal_line(&mut self, line: u32, _window: &mut Window, cx: &mut Context<Self>) {
        match &self.state {
            State::Ready(v) => Self::focus_line(v, line, cx),
            _ => self.pending_line = Some(line),
        }
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn insert_dropped_paths(&mut self, paths: &[String], _window: &mut Window, cx: &mut Context<Self>) {
        if let State::Ready(v) = &self.state {
            v.update(cx, |v, cx| v.insert_dropped_paths(paths, cx));
        }
    }
}

impl Render for EditorPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let status = |child: gpui_kit::AnyElement, error: bool| {
            div()
                .track_focus(&self.focus)
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(u(24.))
                .bg(th.colors.background)
                .text_size(u(13.))
                .text_color(if error { th.colors.destructive } else { th.colors.muted_foreground })
                .child(child)
                .into_any_element()
        };
        let view = match &self.state {
            State::Loading => return status(t!(CommonLoading).into_any_element(), false),
            State::Failed(e) => return status(e.clone().into_any_element(), true),
            State::NotText { size, too_large } => {
                let child = if is_image_path(&self.path) {
                    img(PathBuf::from(&self.path))
                        .max_w_full()
                        .max_h_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element()
                } else {
                    let size = format_bytes(*size);
                    let text = if *too_large { t!(EditorTooLarge, size = size) } else { t!(EditorBinaryFile, size = size) };
                    text.into_any_element()
                };
                return status(child, false);
            }
            State::Ready(v) => v.clone(),
        };
        let button = |id: &'static str, label: String| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .px(u(8.))
                .py(u(1.))
                .rounded(u(4.))
                .border_1()
                .border_color(th.colors.border)
                .bg(th.colors.background)
                .cursor_pointer()
                .child(label)
        };
        let banner = self.conflict.then(|| {
            div()
                .flex()
                .items_center()
                .gap(u(8.))
                .px(u(10.))
                .py(u(4.))
                .text_size(u(12.))
                .bg(th.colors.background.blend(th.colors.destructive.opacity(0.12)))
                .border_b_1()
                .border_color(th.colors.border)
                .child(t!(EditorConflict))
                .child(
                    button("editor-conflict-reload", t!(EditorReloadFromDisk))
                        .on_click(cx.listener(|this, _, _, cx| this.resolve_conflict_reload(cx))),
                )
                .child(
                    button("editor-conflict-keep", t!(EditorKeepMine))
                        .on_click(cx.listener(|this, _, _, cx| this.resolve_conflict_keep(cx))),
                )
        });
        div()
            .id("editor-pane")
            .flex()
            .flex_col()
            .size_full()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.drop_external(paths.paths(), cx)))
            .children(banner)
            .child(div().flex_1().min_h_0().child(view))
            .child(StatusBar { stats: self.stats })
            .into_any_element()
    }
}
