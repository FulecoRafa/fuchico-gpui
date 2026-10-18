//! File explorer (spec 01-shell "File explorer").
//!
//! A virtualized tree over lazily loaded directories (one `fs_read_dir` per
//! expanded folder, off the UI thread), with the original's selection model
//! (focus/anchor/multi-select), keyboard navigation and type-ahead, inline
//! create/rename inputs, delete with confirmation, cut/paste and
//! drag-and-drop moves (spring-loaded folders, drop on the root), Finder
//! file import, the row context menu, and `fs:changed` refreshes.
//! Pure rules live in [`tree`] and are unit tested there.

mod drag;
#[cfg(test)]
mod snapshot_tests;
mod tree;
#[cfg(test)]
mod ui_tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use fuchico_core::events::CoreEvent;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, DragMoveEvent, Entity, EventEmitter, ExternalPaths,
    FocusHandle, Focusable, KeyDownEvent, MouseButton, Pixels, Point, ScrollStrategy, SharedString,
    Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div, uniform_list,
};

use drag::DragGhost;
pub use drag::DraggedPaths;
pub use tree::{DirState, Entry, Row};
use tree::{PendingCreate, PendingKind, Selection};

use crate::assets::AppIcon;
use crate::bus::{self, AppEvent};
use crate::theme::{AppTheme, alpha};
use crate::ui::{ConfirmOptions, MenuEntry, build_menu, confirm, u};
use crate::workspace::view::ViewEvent;

pub const ROW_HEIGHT: f32 = 24.;

/// What a drag is over: `None` (no valid target) or the folder it lands in.
type DropTarget = Option<String>;

type MenuAction = Rc<dyn Fn(&mut Explorer, &mut Window, &mut Context<Explorer>)>;

pub struct Explorer {
    root: Option<String>,
    tree: tree::Tree,
    expanded: HashSet<String>,
    sel: Selection,
    /// The active tab's file (auto-selected whenever it is visible).
    active: Option<String>,
    /// Visible entries at the last reconcile; a change re-runs the
    /// prune/auto-select effects of the original.
    last_entries: Vec<String>,
    active_dirty: bool,
    cut: Vec<String>,
    pending: Option<PendingCreate>,
    renaming: Option<String>,
    input: Option<(Entity<InputState>, Subscription)>,
    drop_target: DropTarget,
    spring: Option<Task<()>>,
    /// Import notice: `(text, is_error)`.
    notice: Option<(String, bool)>,
    notice_task: Option<Task<()>>,
    type_ahead: (String, Option<Instant>),
    /// Rows of the last render (drop hit-testing between renders).
    rows: Rc<Vec<Row>>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _bus: Subscription,
}

impl EventEmitter<ViewEvent> for Explorer {}

impl Focusable for Explorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Explorer {
    pub fn new(root: Option<String>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = bus::subscribe(cx, |this: &mut Explorer, ev, cx| {
            if let AppEvent::Core(CoreEvent::FsChanged { dir, .. }) = ev {
                this.refetch(dir.clone(), cx);
            }
        });
        let mut this = Explorer {
            root: None,
            tree: tree::Tree::default(),
            expanded: HashSet::new(),
            sel: Selection::default(),
            active: None,
            last_entries: Vec::new(),
            active_dirty: false,
            cut: Vec::new(),
            pending: None,
            renaming: None,
            input: None,
            drop_target: None,
            spring: None,
            notice: None,
            notice_task: None,
            type_ahead: (String::new(), None),
            rows: Rc::new(Vec::new()),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _bus: sub,
        };
        this.set_root(root, cx);
        this
    }

    /// Root change resets all tree state (expansion is not persisted).
    pub fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.tree = tree::Tree::default();
        self.expanded.clear();
        self.sel = Selection::default();
        self.cut.clear();
        self.pending = None;
        self.renaming = None;
        self.input = None;
        self.drop_target = None;
        self.spring = None;
        self.last_entries.clear();
        self.active_dirty = true;
        if let Some(r) = self.root.clone() {
            self.refetch(r, cx);
        }
        cx.notify();
    }

    /// The active tab's file: auto-selected (single selection + anchor)
    /// whenever it changes, provided its row is visible.
    pub fn set_active_file(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if self.active != path {
            self.active = path;
            self.active_dirty = true;
            cx.notify();
        }
    }

    /// Fetches `dir` if it is the root or an expanded folder; "loading" only
    /// shows when it was not loaded before (no flicker on refresh).
    fn refetch(&mut self, dir: String, cx: &mut Context<Self>) {
        let relevant = self.root.as_deref() == Some(dir.as_str()) || self.expanded.contains(&dir);
        if !relevant {
            return;
        }
        if !self.tree.is_loaded(&dir) {
            self.tree.set(dir.clone(), DirState::Loading);
        }
        let d = dir.clone();
        let task = cx.background_spawn(async move { tree::read_dir(&d) });
        cx.spawn(async move |this, cx| {
            let state = task.await;
            this.update(cx, |this, cx| {
                this.tree.set(dir, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle(&mut self, dir: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_string());
            self.refetch(dir.to_string(), cx);
        }
        cx.notify();
    }

    fn refresh_root(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.refetch(root.clone(), cx);
        // `notifyFsChanged(root)`: other windows and indexes refresh too.
        bus::emit(
            cx,
            AppEvent::Core(CoreEvent::FsChanged {
                dir: root,
                from: Some("main".into()),
            }),
        );
    }

    fn input_open(&self) -> bool {
        self.renaming.is_some() || self.pending.is_some()
    }

    fn is_root(&self, path: &str) -> bool {
        self.root.as_deref() == Some(path)
    }

    fn entry_at(&self, path: &str) -> Option<(usize, &Entry, bool)> {
        self.rows.iter().enumerate().find_map(|(i, r)| match r {
            Row::Entry {
                entry, expanded, ..
            } if entry.path == path => Some((i, entry, *expanded)),
            _ => None,
        })
    }

    fn entries(&self) -> Vec<String> {
        tree::entry_paths(&self.rows)
    }

    fn scroll_to(&self, path: &str) {
        if let Some((ix, ..)) = self.entry_at(path) {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    // ----- mouse -------------------------------------------------------

    fn click_entry(
        &mut self,
        entry: &Entry,
        ev: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input_open() || !ev.standard_click() {
            return;
        }
        let m = ev.modifiers();
        let toggle = m.platform || m.control;
        let entries = self.entries();
        self.sel
            .select(&entry.path, m.shift, toggle && !m.shift, &entries);
        if !m.shift && !toggle {
            if entry.is_dir {
                self.toggle(&entry.path, cx);
            } else {
                cx.emit(ViewEvent::OpenFile {
                    path: entry.path.clone(),
                    line: None,
                });
            }
            if ev.click_count() == 2 {
                self.begin_rename(entry.path.clone(), window, cx);
            }
        }
        cx.notify();
    }

    fn right_click_entry(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.sel.contains(path) {
            self.sel.select_only(path);
            cx.notify();
        }
    }

    // ----- keyboard ----------------------------------------------------

    fn key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_key(ev, window, cx) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// `handleKeyDown`; returns whether the key was consumed.
    fn handle_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.input_open() || self.root.is_none() {
            return false;
        }
        let entries = self.entries();
        if entries.is_empty() {
            return false;
        }
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let key = ks.key.as_str();
        let cur = self
            .sel
            .focus
            .as_ref()
            .and_then(|f| entries.iter().position(|p| p == f));
        let modk = (m.platform || m.control) && !m.alt && !m.shift;
        if modk {
            match key {
                "x" => {
                    if let Some(i) = cur {
                        self.cut = self.sel.targets_for(&entries[i]);
                    }
                    return cur.is_some();
                }
                "v" if !self.cut.is_empty() => {
                    let dir = match cur {
                        Some(i) => {
                            let is_dir =
                                self.entry_at(&entries[i]).is_none_or(|(_, e, _)| e.is_dir);
                            tree::drop_dir_for(&entries[i], is_dir)
                        }
                        None => self.root.clone().unwrap_or_default(),
                    };
                    self.paste_into(dir, cx);
                    return true;
                }
                "a" => {
                    self.sel.select_all(&entries);
                    return true;
                }
                _ => {}
            }
        }
        if key == "escape" && !self.cut.is_empty() {
            self.cut.clear();
            return true;
        }
        let move_to = |this: &mut Self, next: isize, extend: bool| {
            let i = next.clamp(0, entries.len() as isize - 1) as usize;
            this.sel.select(&entries[i], extend, false, &entries);
            this.scroll_to(&entries[i]);
        };
        let plain = !m.platform && !m.control && !m.alt;
        match key {
            "down" if plain => {
                move_to(self, cur.map_or(0, |c| c as isize + 1), m.shift);
                true
            }
            "up" if plain => {
                move_to(
                    self,
                    cur.map_or(entries.len() as isize - 1, |c| c as isize - 1),
                    m.shift,
                );
                true
            }
            "right" if plain => {
                let Some(c) = cur else { return false };
                if let Some((_, e, expanded)) = self.entry_at(&entries[c]) {
                    if e.is_dir && !expanded {
                        let p = e.path.clone();
                        self.toggle(&p, cx);
                    } else if e.is_dir {
                        move_to(self, c as isize + 1, false);
                    }
                }
                true
            }
            "left" if plain => {
                let Some(c) = cur else { return false };
                if let Some((_, e, expanded)) = self.entry_at(&entries[c]) {
                    let p = e.path.clone();
                    if e.is_dir && expanded {
                        self.toggle(&p, cx);
                    } else if let Some(parent) = tree::parent_dir(&p).filter(|d| !self.is_root(d)) {
                        // Quirk: only the cursor moves; the highlight stays.
                        self.sel.focus = Some(parent);
                    }
                }
                true
            }
            "enter" if plain => {
                let Some(c) = cur else { return false };
                if let Some((_, e, _)) = self.entry_at(&entries[c]) {
                    let (p, is_dir) = (e.path.clone(), e.is_dir);
                    if is_dir {
                        self.toggle(&p, cx);
                    } else {
                        cx.emit(ViewEvent::OpenFile {
                            path: p,
                            line: None,
                        });
                    }
                }
                true
            }
            "delete" | "backspace" if !m.alt && !m.control => {
                let Some(c) = cur else { return false };
                let targets = self.sel.targets_for(&entries[c]);
                self.delete_all(targets, window, cx);
                true
            }
            "f2" => {
                let Some(c) = cur else { return false };
                self.begin_rename(entries[c].clone(), window, cx);
                true
            }
            _ => self.type_ahead(ks, &entries, cur),
        }
    }

    fn type_ahead(
        &mut self,
        ks: &gpui_kit::Keystroke,
        entries: &[String],
        cur: Option<usize>,
    ) -> bool {
        let m = ks.modifiers;
        if m.platform || m.control || m.alt || m.function {
            return false;
        }
        // `key_char` is what the key types; fall back to a 1-char key name.
        let typed = ks
            .key_char
            .as_deref()
            .or(Some(ks.key.as_str()).filter(|k| k.chars().count() == 1));
        let Some(ch) = typed.filter(|c| c.chars().count() == 1 && !c.chars().all(char::is_control))
        else {
            return false;
        };
        let now = Instant::now();
        let fresh = self.type_ahead.1.is_none_or(|at| {
            now.duration_since(at) >= Duration::from_millis(tree::TYPE_AHEAD_RESET_MS)
        });
        let ch = ch.to_lowercase();
        let buffer = if fresh {
            ch
        } else {
            format!("{}{ch}", self.type_ahead.0)
        };
        self.type_ahead = (buffer.clone(), Some(now));
        let names: Vec<String> = entries
            .iter()
            .map(|p| tree::basename(p).to_string())
            .collect();
        if let Some(i) = tree::type_ahead_target(&buffer, &names, cur) {
            self.sel.select(&entries[i], false, false, entries);
            self.scroll_to(&entries[i]);
        }
        true
    }

    // ----- inline inputs -----------------------------------------------

    fn start_input(
        &mut self,
        initial: &str,
        placeholder: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = initial.to_string();
        let state = cx.new(|cx| {
            let s = InputState::new(window, cx).default_value(initial);
            match placeholder {
                Some(p) => s.placeholder(p),
                None => s,
            }
        });
        let sub = cx.subscribe_in(
            &state,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    this.commit_input(cx);
                    window.focus(&this.focus, cx);
                }
                InputEvent::Blur => this.commit_input(cx),
                _ => {}
            },
        );
        state.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.input = Some((state, sub));
        cx.notify();
    }

    fn cancel_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        self.renaming = None;
        self.input = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn commit_input(&mut self, cx: &mut Context<Self>) {
        let Some((state, _)) = self.input.take() else {
            return;
        };
        let value = state.read(cx).value().to_string();
        if let Some(p) = self.pending.take() {
            self.commit_create(p, &value, cx);
        } else if let Some(from) = self.renaming.take() {
            self.commit_rename(from, &value, cx);
        }
        cx.notify();
    }

    fn begin_create(
        &mut self,
        parent: String,
        kind: PendingKind,
        default_ext: Option<&'static str>,
        placeholder: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.renaming = None;
        self.input = None;
        if !self.is_root(&parent) && !self.expanded.contains(&parent) {
            self.expanded.insert(parent.clone());
        }
        if !self.tree.contains(&parent) {
            self.refetch(parent.clone(), cx);
        }
        let ph = placeholder.clone().unwrap_or_else(|| match kind {
            PendingKind::File => crate::t!(ExplorerNewFile),
            PendingKind::Dir => crate::t!(ExplorerNewFolder),
        });
        self.pending = Some(PendingCreate {
            parent,
            kind,
            default_ext,
            placeholder,
        });
        self.start_input("", Some(ph), window, cx);
    }

    fn commit_create(&mut self, p: PendingCreate, value: &str, cx: &mut Context<Self>) {
        let Some(name) = tree::create_name(value, p.default_ext) else {
            return;
        };
        let path = tree::join(&p.parent, &name);
        let kind = p.kind;
        // "New drawing" goes through the same helper as the excalidraw module
        // so the file starts as a valid empty scene, not a 0-byte file.
        let drawing = kind == PendingKind::File && name.to_lowercase().ends_with(".excalidraw");
        let dir = p.parent.clone();
        let task = cx.background_spawn(async move {
            match kind {
                PendingKind::File if drawing => {
                    crate::excalidraw::create_drawing(&dir, &name).map(|_| ())
                }
                PendingKind::File => fuchico_core::fs::mutate::fs_create_file(path),
                PendingKind::Dir => fuchico_core::fs::mutate::fs_create_dir(path),
            }
        });
        let parent = p.parent;
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                log::warn!("create failed: {e}");
            }
            this.update(cx, |this, cx| this.refetch(parent, cx)).ok();
        })
        .detach();
    }

    fn begin_rename(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        self.input = None;
        let name = tree::basename(&path).to_string();
        self.renaming = Some(path);
        self.start_input(&name, None, window, cx);
    }

    fn commit_rename(&mut self, from: String, value: &str, cx: &mut Context<Self>) {
        let Some(to) = tree::rename_target(&from, value) else {
            return;
        };
        let task = cx.background_spawn({
            let (from, to) = (from.clone(), to.clone());
            async move { fuchico_core::fs::mutate::fs_rename(from, to) }
        });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(_) => cx.emit(ViewEvent::PathRenamed {
                        from: from.clone(),
                        to,
                    }),
                    Err(e) => log::warn!("rename failed: {e}"),
                }
                if let Some(parent) = tree::parent_dir(&from) {
                    this.refetch(parent, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    // ----- delete / move / import ------------------------------------

    fn delete_all(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let name = if paths.len() == 1 {
            format!("\"{}\"", tree::basename(&paths[0]))
        } else {
            crate::t!(ExplorerItemsCount, count = paths.len())
        };
        let this = cx.entity().downgrade();
        confirm(
            window,
            cx,
            ConfirmOptions::new(crate::t!(ExplorerConfirmDelete, name = name)).danger(),
            move |_, cx| {
                let (paths, this) = (paths.clone(), this.clone());
                cx.spawn(async move |cx| {
                    for path in paths {
                        let task = cx.background_spawn({
                            let path = path.clone();
                            async move { fuchico_core::fs::mutate::fs_delete(path) }
                        });
                        if let Err(e) = task.await {
                            log::warn!("delete failed: {e}");
                            continue;
                        }
                        this.update(cx, |this, cx| {
                            cx.emit(ViewEvent::PathDeleted(path.clone()));
                            if let Some(parent) = tree::parent_dir(&path) {
                                this.refetch(parent, cx);
                            }
                        })
                        .ok();
                    }
                })
                .detach();
            },
        );
    }

    /// `moveAll`: one at a time, in order; skipped when not allowed.
    fn move_all(&mut self, paths: Vec<String>, to_dir: String, cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .into_iter()
            .filter(|p| tree::can_move(p, &to_dir))
            .collect();
        if paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            for from in paths {
                let to = tree::join(&to_dir, tree::basename(&from));
                let task = cx.background_spawn({
                    let (from, to) = (from.clone(), to.clone());
                    async move { fuchico_core::fs::mutate::fs_rename(from, to) }
                });
                let res = task.await;
                let ok = this.update(cx, |this, cx| {
                    match res {
                        Ok(_) => cx.emit(ViewEvent::PathRenamed {
                            from: from.clone(),
                            to,
                        }),
                        Err(e) => log::warn!("move failed: {e}"),
                    }
                    if let Some(parent) = tree::parent_dir(&from) {
                        this.refetch(parent, cx);
                    }
                    this.refetch(to_dir.clone(), cx);
                });
                if ok.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn paste_into(&mut self, dir: String, cx: &mut Context<Self>) {
        let cut = std::mem::take(&mut self.cut);
        self.move_all(cut, dir, cx);
        cx.notify();
    }

    fn run_import(&mut self, files: Vec<PathBuf>, to_dir: String, cx: &mut Context<Self>) {
        let ts = fuchico_core::attachments::timestamp(&chrono::Local::now().naive_local());
        let task = cx.background_spawn({
            let to_dir = to_dir.clone();
            async move { tree::import_files(&files, &to_dir, &ts) }
        });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                if !this.is_root(&to_dir) && !this.expanded.contains(&to_dir) {
                    this.toggle(&to_dir, cx);
                } else {
                    this.refetch(to_dir.clone(), cx);
                }
                let folder = tree::basename(&to_dir).to_string();
                let notice = if !res.errors.is_empty() {
                    (
                        crate::t!(ExplorerImportFailed, errors = res.errors.join("; ")),
                        true,
                    )
                } else if res.imported.len() == 1 {
                    (crate::t!(ExplorerImportedOne, folder = folder), false)
                } else {
                    (
                        crate::t!(
                            ExplorerImportedMany,
                            count = res.imported.len(),
                            folder = folder
                        ),
                        false,
                    )
                };
                this.show_notice(notice, cx);
            })
            .ok();
        })
        .detach();
    }

    fn show_notice(&mut self, notice: (String, bool), cx: &mut Context<Self>) {
        self.notice = Some(notice);
        self.notice_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(tree::NOTICE_MS))
                .await;
            this.update(cx, |this, cx| {
                this.notice = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    // ----- drag and drop -----------------------------------------------

    /// The folder a drag at `pos` would land in (`dragged` = internal
    /// paths; `None` for OS files, which are always accepted).
    fn drop_target_at(
        &self,
        pos: Point<Pixels>,
        bounds: gpui_kit::Bounds<Pixels>,
        dragged: Option<&[String]>,
        window: &Window,
    ) -> DropTarget {
        let root = self.root.clone()?;
        if !bounds.contains(&pos) {
            return None;
        }
        let valid = |dir: &str| {
            dragged.is_none_or(|ps| !ps.is_empty() && ps.iter().all(|p| tree::can_move(p, dir)))
        };
        let root_target = valid(&root).then(|| root.clone());
        let list = self.scroll.0.borrow().base_handle.bounds();
        let offset = self.scroll.0.borrow().base_handle.offset();
        if !list.contains(&pos) {
            return root_target;
        }
        let row_h = f32::from(window.rem_size()) * ROW_HEIGHT / 16.;
        let y = f32::from(pos.y - list.top() - offset.y);
        if y < 0. {
            return root_target;
        }
        match self.rows.get((y / row_h) as usize) {
            Some(Row::Entry { entry, .. }) => {
                let dir = tree::drop_dir_for(&entry.path, entry.is_dir);
                if valid(&dir) { Some(dir) } else { root_target }
            }
            _ => root_target,
        }
    }

    fn set_drop_target(&mut self, target: DropTarget, cx: &mut Context<Self>) {
        if self.drop_target == target {
            return;
        }
        self.drop_target = target.clone();
        self.spring = None;
        // Spring-loaded folders: a collapsed non-root folder opens after
        // `DROP_EXPAND_MS` of hovering.
        if let Some(dir) = target.filter(|d| !self.is_root(d) && !self.expanded.contains(d)) {
            self.spring = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(tree::DROP_EXPAND_MS))
                    .await;
                this.update(cx, |this, cx| {
                    if this.drop_target.as_deref() == Some(dir.as_str())
                        && !this.expanded.contains(&dir)
                    {
                        this.toggle(&dir, cx);
                    }
                })
                .ok();
            }));
        }
        cx.notify();
    }

    fn drop_paths(&mut self, paths: &DraggedPaths, cx: &mut Context<Self>) {
        if let Some(dir) = self.drop_target.take() {
            self.move_all(paths.0.clone(), dir, cx);
        }
        self.spring = None;
        cx.notify();
    }

    fn drop_external(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        if let Some(dir) = self.drop_target.take() {
            self.run_import(paths.paths().to_vec(), dir, cx);
        }
        self.spring = None;
        cx.notify();
    }

    // ----- menu ----------------------------------------------------------

    fn row_menu(&self, entry: &Entry, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let this = cx.entity().downgrade();
        let path = entry.path.clone();
        let targets = self.sel.targets_for(&path);
        let n = targets.len();
        let mut items = Vec::new();
        let act = |f: MenuAction| {
            let t = this.clone();
            move |w: &mut Window, cx: &mut App| {
                t.update(cx, |this, cx| f(this, w, cx)).ok();
            }
        };
        if entry.is_dir {
            let p = path.clone();
            items.push(
                MenuEntry::item(
                    crate::t!(ExplorerNewFile),
                    act(Rc::new(move |this, w, cx| {
                        this.begin_create(p.clone(), PendingKind::File, None, None, w, cx)
                    })),
                )
                .icon(AppIcon::FilePlus),
            );
            let p = path.clone();
            items.push(
                MenuEntry::item(
                    crate::t!(ExplorerNewFolder),
                    act(Rc::new(move |this, w, cx| {
                        this.begin_create(p.clone(), PendingKind::Dir, None, None, w, cx)
                    })),
                )
                .icon(AppIcon::FolderPlus),
            );
            let p = path.clone();
            items.push(
                MenuEntry::item(
                    crate::t!(ExplorerNewDrawing),
                    act(Rc::new(move |this, w, cx| {
                        let ph = Some(crate::t!(ExplorerNewDrawing));
                        this.begin_create(
                            p.clone(),
                            PendingKind::File,
                            Some("excalidraw"),
                            ph,
                            w,
                            cx,
                        )
                    })),
                )
                .icon(AppIcon::PenTool),
            );
            items.push(MenuEntry::Separator);
        } else {
            let p = path.clone();
            items.push(
                MenuEntry::item(
                    crate::t!(TabsOpenInNewWindow),
                    act(Rc::new(move |_, _, cx| {
                        cx.emit(ViewEvent::OpenInNewWindow(p.clone()))
                    })),
                )
                .icon(AppIcon::AppWindow),
            );
        }
        let p = path.clone();
        items.push(
            MenuEntry::item(
                crate::t!(CommonRename),
                act(Rc::new(move |this, w, cx| {
                    this.begin_rename(p.clone(), w, cx)
                })),
            )
            .icon(AppIcon::Pencil),
        );
        let cut_label = if n > 1 {
            crate::t!(ExplorerCutItems, count = n)
        } else {
            crate::t!(ExplorerCut)
        };
        let t = targets.clone();
        items.push(
            MenuEntry::item(
                cut_label,
                act(Rc::new(move |this, _, cx| {
                    this.cut = t.clone();
                    cx.notify();
                })),
            )
            .icon(AppIcon::Scissors),
        );
        if !self.cut.is_empty() {
            let name = if self.cut.len() == 1 {
                tree::basename(&self.cut[0]).to_string()
            } else {
                crate::t!(ExplorerItemsCount, count = self.cut.len())
            };
            let dir = tree::drop_dir_for(&path, entry.is_dir);
            items.push(
                MenuEntry::item(
                    crate::t!(ExplorerPasteHere, name = name),
                    act(Rc::new(move |this, _, cx| this.paste_into(dir.clone(), cx))),
                )
                .icon(IconName::Copy),
            );
        }
        items.push(MenuEntry::Separator);
        let p = path.clone();
        items.push(
            MenuEntry::item(crate::t!(CommonRevealInFileManager), move |_, cx| {
                cx.reveal_path(std::path::Path::new(&p))
            })
            .icon(IconName::Eye),
        );
        let tool = crate::settings::SettingsStore::get(cx)
            .external_tool
            .trim()
            .to_string();
        let open_label = if tool.is_empty() {
            crate::t!(ExplorerOpenWithDefault)
        } else {
            crate::t!(ExplorerOpenWith, tool = tool.clone())
        };
        let p = path.clone();
        items.push(
            MenuEntry::item(open_label, move |w, cx| {
                open_with_external_tool(p.clone(), tool.clone(), w, cx)
            })
            .icon(AppIcon::ExternalLink),
        );
        let copy_label = if n > 1 {
            crate::t!(ExplorerCopyPaths, count = n)
        } else {
            crate::t!(CommonCopyPath)
        };
        let joined = targets.join("\n");
        items.push(
            MenuEntry::item(copy_label, move |_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(joined.clone()))
            })
            .icon(IconName::Copy),
        );
        items.push(MenuEntry::Separator);
        let del_label = if n > 1 {
            crate::t!(ExplorerDeleteItems, count = n)
        } else {
            crate::t!(CommonDelete)
        };
        items.push(
            MenuEntry::item(
                del_label,
                act(Rc::new(move |this, w, cx| {
                    this.delete_all(targets.clone(), w, cx)
                })),
            )
            .icon(AppIcon::Trash)
            .danger(),
        );
        items
    }

    // ----- rendering -------------------------------------------------------

    /// Re-runs the original's effects when the visible entries or the
    /// active file change: drop vanished rows from the selection and cut
    /// list, then auto-select the active file if visible.
    fn reconcile(&mut self, rows: &[Row]) {
        let entries = tree::entry_paths(rows);
        if entries == self.last_entries && !self.active_dirty {
            return;
        }
        let visible: HashSet<&str> = entries.iter().map(String::as_str).collect();
        self.sel.prune(&visible);
        self.cut.retain(|p| visible.contains(p.as_str()));
        if let Some(a) = self.active.as_deref().filter(|a| visible.contains(a)) {
            self.sel.select_only(a);
            self.active_dirty = false;
        } else if self.active.is_none() {
            self.active_dirty = false;
        }
        self.last_entries = entries;
    }

    fn render_header(
        &self,
        root: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let title: SharedString = tree::basename(root).to_string().into();
        let full: SharedString = root.to_string().into();
        let root_s = root.to_string();
        let targeted = self.drop_target.as_deref() == Some(root);
        let _ = window;
        let r1 = root_s.clone();
        let r2 = root_s.clone();
        let r3 = root_s;
        div()
            .id("explorer-header")
            .debug_selector(|| "ex-header".to_string())
            .flex()
            .flex_none()
            .items_center()
            .gap(u(4.))
            .h(u(32.))
            .px(u(6.))
            .border_b_1()
            .border_color(alpha(th.colors.border, 0.6))
            .when(targeted, |d| {
                d.bg(th.colors.accent)
                    .border_1()
                    .border_color(th.colors.ring)
            })
            .child(
                div()
                    .id("explorer-title")
                    .flex_1()
                    .min_w_0()
                    .pl(u(4.))
                    .text_size(u(12.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(th.fg_alpha(0.8))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(title)
                    .tooltip(move |w, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(w, cx)
                    }),
            )
            .child(header_button(
                "ex-open",
                IconName::FolderOpen.into(),
                crate::t!(ExplorerOpenFolderAction),
                &th,
                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ViewEvent::OpenFolder)),
            ))
            .child(header_button(
                "ex-new-file",
                AppIcon::FilePlus.into(),
                crate::t!(ExplorerNewFile),
                &th,
                cx.listener(move |this, _: &ClickEvent, w, cx| {
                    this.begin_create(r1.clone(), PendingKind::File, None, None, w, cx)
                }),
            ))
            .child(header_button(
                "ex-new-folder",
                AppIcon::FolderPlus.into(),
                crate::t!(ExplorerNewFolder),
                &th,
                cx.listener(move |this, _: &ClickEvent, w, cx| {
                    this.begin_create(r2.clone(), PendingKind::Dir, None, None, w, cx)
                }),
            ))
            .child(header_button(
                "ex-new-drawing",
                AppIcon::PenTool.into(),
                crate::t!(ExplorerNewDrawing),
                &th,
                cx.listener(move |this, _: &ClickEvent, w, cx| {
                    let ph = Some(crate::t!(ExplorerNewDrawing));
                    this.begin_create(r3.clone(), PendingKind::File, Some("excalidraw"), ph, w, cx)
                }),
            ))
            .child(header_button(
                "ex-refresh",
                IconName::RefreshCw.into(),
                crate::t!(ExplorerRefresh),
                &th,
                cx.listener(|this, _: &ClickEvent, _, cx| this.refresh_root(cx)),
            ))
    }

    fn render_input_row(
        &self,
        depth: usize,
        icon: Icon,
        th: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((state, _)) = &self.input else {
            return div().into_any_element();
        };
        div()
            .id("explorer-input-row")
            .debug_selector(|| "ex-input".to_string())
            .h(u(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(u(6.))
            .pl(u(6. + depth as f32 * 12.))
            .pr(u(6.))
            .text_size(u(13.))
            .child(div().flex_none().size(u(14.)))
            .child(
                icon.size(u(14.))
                    .flex_none()
                    .text_color(th.colors.muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(u(4.))
                    .border_1()
                    .border_color(th.colors.ring)
                    .rounded(u(th.radius.sm))
                    .bg(th.colors.background)
                    .text_color(th.colors.foreground)
                    .on_action(
                        cx.listener(|this, _: &Escape, window, cx| this.cancel_input(window, cx)),
                    )
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                        // Don't stop propagation: that would also suppress text
                        // insertion. The tree ignores keys while the input is open.
                        if ev.keystroke.key == "escape" {
                            this.cancel_input(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        Input::new(state)
                            .appearance(false)
                            .h(u(20.))
                            .text_size(u(13.)),
                    ),
            )
            .into_any_element()
    }
}

fn header_button(
    id: &'static str,
    icon: Icon,
    tip: String,
    th: &AppTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (hover_bg, hover_fg) = (th.fg_alpha(0.06), th.colors.foreground);
    let tip: SharedString = tip.into();
    div()
        .id(id)
        .debug_selector(|| id.to_string())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(24.))
        .rounded(u(th.radius.sm))
        .text_color(th.colors.muted_foreground)
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .cursor_pointer()
        .child(icon.size(u(14.)))
        .tooltip(move |w, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(w, cx))
        .on_click(on_click)
}

/// `openWithExternalTool(path, tool)`: the named app, or the OS default
/// handler when no tool is configured. Failures surface as a toast.
fn open_with_external_tool(path: String, tool: String, window: &mut Window, cx: &mut App) {
    let task = cx.background_spawn({
        let (path, tool) = (path.clone(), tool.clone());
        async move { spawn_open(&path, &tool) }
    });
    window
        .spawn(cx, async move |cx| {
            if let Err(e) = task.await {
                let app = if tool.is_empty() {
                    crate::t!(ExplorerTheDefaultApp)
                } else {
                    tool
                };
                cx.update(|window, cx| {
                    crate::ui::toast::toast_error(
                        window,
                        cx,
                        crate::t!(ExplorerOpenWithFailed, app = app, error = e),
                    )
                })
                .ok();
            }
        })
        .detach();
}

fn spawn_open(path: &str, tool: &str) -> Result<(), String> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        if !tool.is_empty() {
            c.arg("-a").arg(tool);
        }
        c.arg(path);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        if !tool.is_empty() {
            c.arg(tool);
        }
        c.arg(path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new(if tool.is_empty() { "xdg-open" } else { tool });
        c.arg(path);
        c
    };
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            out.status.to_string()
        } else {
            err
        })
    }
}

impl Render for Explorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let Some(root) = self.root.clone() else {
            return div()
                .p(u(12.))
                .text_size(u(12.))
                .text_color(th.colors.muted_foreground)
                .child(crate::t!(ExplorerNoFolderOpen))
                .into_any_element();
        };
        if !cx.has_active_drag() && (self.drop_target.is_some() || self.spring.is_some()) {
            self.drop_target = None;
            self.spring = None;
        }
        let rows = tree::flatten(
            &self.tree,
            &root,
            &self.expanded,
            self.renaming.as_deref(),
            self.pending.as_ref(),
        );
        self.reconcile(&rows);
        let rows = Rc::new(rows);
        self.rows = rows.clone();

        let root_pending = self
            .pending
            .as_ref()
            .filter(|p| p.parent == root)
            .map(|p| p.kind);
        let body = match self.tree.get(&root) {
            Some(DirState::Loaded(_)) => {
                let view = cx.entity();
                let rows2 = rows.clone();
                uniform_list("explorer-rows", rows.len(), move |range, _window, cx| {
                    view.update(cx, |this, cx| {
                        range
                            .map(|i| this.render_row(&rows2[i], i, cx))
                            .collect::<Vec<_>>()
                    })
                })
                .track_scroll(&self.scroll)
                .flex_1()
                .into_any_element()
            }
            Some(DirState::Error(e)) => {
                status_line(e.clone(), th.colors.destructive).into_any_element()
            }
            _ => {
                status_line(crate::t!(CommonLoading), th.colors.muted_foreground).into_any_element()
            }
        };
        let notice = self.notice.clone().map(|(text, err)| {
            div()
                .flex_none()
                .px(u(8.))
                .py(u(4.))
                .text_size(u(11.))
                .border_b_1()
                .border_color(th.colors.border)
                .text_color(if err {
                    th.colors.destructive
                } else {
                    th.colors.muted_foreground
                })
                .child(text)
        });
        let pending_root =
            root_pending.map(|kind| self.render_input_row(0, kind_icon(kind), &th, cx));

        div()
            .id("explorer")
            .track_focus(&self.focus)
            .key_context("Explorer")
            .flex()
            .flex_col()
            .size_full()
            .font_family(th.ui_font.clone())
            .text_size(u(13.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.input_open() {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .on_key_down(cx.listener(Self::key_down))
            .on_drag_move(
                cx.listener(|this, ev: &DragMoveEvent<DraggedPaths>, window, cx| {
                    let paths = ev.drag(cx).0.clone();
                    let t = this.drop_target_at(ev.event.position, ev.bounds, Some(&paths), window);
                    this.set_drop_target(t, cx);
                }),
            )
            .on_drag_move(
                cx.listener(|this, ev: &DragMoveEvent<ExternalPaths>, window, cx| {
                    let t = this.drop_target_at(ev.event.position, ev.bounds, None, window);
                    this.set_drop_target(t, cx);
                }),
            )
            .on_drop(cx.listener(|this, p: &DraggedPaths, _, cx| this.drop_paths(p, cx)))
            .on_drop(cx.listener(|this, p: &ExternalPaths, _, cx| this.drop_external(p, cx)))
            .child(self.render_header(&root, window, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .pt(u(4.))
                    .children(notice)
                    .children(pending_root)
                    .child(body),
            )
            .into_any_element()
    }
}

impl Explorer {
    fn render_row(&self, row: &Row, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let th = AppTheme::get(cx).clone();
        match row {
            Row::Status { depth, text, error } => div()
                .h(u(ROW_HEIGHT))
                .flex()
                .items_center()
                .pl(u(6. + *depth as f32 * 12. + 18.))
                .pr(u(12.))
                .text_size(u(11.))
                .text_color(if *error {
                    th.colors.destructive
                } else {
                    th.colors.muted_foreground
                })
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text.clone())
                .into_any_element(),
            Row::Pending { depth, kind, .. } => {
                self.render_input_row(*depth, kind_icon(*kind), &th, cx)
            }
            Row::Rename { entry, depth } => {
                self.render_input_row(*depth, entry_icon(entry), &th, cx)
            }
            Row::Entry {
                entry,
                depth,
                expanded,
            } => self.render_entry(entry, *depth, *expanded, ix, &th, cx),
        }
    }

    fn render_entry(
        &self,
        entry: &Entry,
        depth: usize,
        expanded: bool,
        ix: usize,
        th: &AppTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_sel = self.sel.contains(&entry.path);
        let is_cut = self.cut.contains(&entry.path);
        let is_drop = entry.is_dir && self.drop_target.as_deref() == Some(entry.path.as_str());
        let chevron = div()
            .flex_none()
            .size(u(14.))
            .flex()
            .items_center()
            .justify_center()
            .when(entry.is_dir, |d| {
                d.child(
                    Icon::new(IconName::ChevronRight)
                        .size(u(12.))
                        .when(expanded, |i| {
                            i.rotate(gpui_kit::radians(std::f32::consts::FRAC_PI_2))
                        }),
                )
            });
        let muted = if is_sel {
            th.colors.accent_foreground
        } else {
            th.colors.muted_foreground
        };
        let hover = alpha(th.colors.accent, 0.7);
        let view: WeakEntity<Explorer> = cx.entity().downgrade();
        let (v1, v2, v3) = (view.clone(), view.clone(), view);
        let (e1, e3) = (entry.clone(), entry.clone());
        let path_sel = entry.path.clone();
        let tip: SharedString = entry.path.clone().into();
        let row = div()
            .id(("ex-row", ix))
            .debug_selector(|| format!("ex-row:{}", entry.name))
            .h(u(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(u(6.))
            .pl(u(6. + depth as f32 * 12.))
            .pr(u(6.))
            .rounded(u(th.radius.sm))
            .text_size(u(13.))
            .font_family(th.ui_font.clone())
            .text_color(th.fg_alpha(0.85))
            .cursor_pointer()
            .when(is_sel || is_drop, |d| {
                d.bg(th.colors.accent)
                    .text_color(th.colors.accent_foreground)
            })
            .when(!is_sel && !is_drop, |d| d.hover(move |s| s.bg(hover)))
            .when(is_drop, |d| d.border_1().border_color(th.colors.ring))
            .when(is_cut, |d| d.opacity(0.55))
            .child(chevron.text_color(muted))
            .child(entry_icon_el(entry, expanded, muted, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(entry.name.clone()),
            )
            .tooltip(move |w, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(w, cx)
            })
            .on_click(move |ev, window, cx| {
                v1.update(cx, |this, cx| this.click_entry(&e1, ev, window, cx))
                    .ok();
            })
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                v2.update(cx, |this, cx| this.right_click_entry(&path_sel, cx))
                    .ok();
            })
            .when(!self.input_open(), |d| {
                let targets = self.sel.targets_for(&entry.path);
                let label: SharedString = if targets.len() > 1 {
                    crate::t!(ExplorerItemsCount, count = targets.len()).into()
                } else {
                    entry.name.clone().into()
                };
                d.on_drag(DraggedPaths(targets), move |_, offset, _, cx| {
                    cx.new(|_| DragGhost {
                        label: label.clone(),
                        offset,
                    })
                })
            })
            .context_menu(move |menu, _, cx| {
                let items = v3
                    .update(cx, |this, cx| this.row_menu(&e3, cx))
                    .unwrap_or_default();
                build_menu(menu, items)
            });
        row.into_any_element()
    }
}

fn kind_icon(kind: PendingKind) -> Icon {
    match kind {
        PendingKind::Dir => IconName::Folder.into(),
        PendingKind::File => IconName::File.into(),
    }
}

/// The row icon: the opt-in Zed icon theme's coloured SVG when one is set
/// (`crate::dev_marks`), else [`entry_icon`].
fn entry_icon_el(entry: &Entry, expanded: bool, muted: gpui_kit::Hsla, cx: &App) -> AnyElement {
    let themed = crate::dev_marks::icon_theme(cx).and_then(|t| {
        if entry.is_dir {
            t.dir_icon(&entry.name, expanded)
        } else {
            t.file_icon(&entry.name)
        }
    });
    match themed {
        Some(path) => gpui_kit::img(path.to_path_buf())
            .size(u(14.))
            .flex_none()
            .into_any_element(),
        None => entry_icon(entry)
            .size(u(14.))
            .flex_none()
            .text_color(muted)
            .into_any_element(),
    }
}

/// `Folder` / `PenTool` (`.excalidraw`, case-insensitive) / `File`.
fn entry_icon(entry: &Entry) -> Icon {
    if entry.is_dir {
        IconName::Folder.into()
    } else if entry.name.to_lowercase().ends_with(".excalidraw") {
        AppIcon::PenTool.into()
    } else {
        IconName::File.into()
    }
}

fn status_line(text: String, color: gpui_kit::Hsla) -> impl IntoElement {
    div()
        .py(u(8.))
        .pl(u(12.))
        .pr(u(12.))
        .text_size(u(11.))
        .text_color(color)
        .child(text)
}
