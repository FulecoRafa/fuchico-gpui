//! The main-window shell (spec 01-shell): vault lifecycle, activity bar,
//! sidebar, the five main views, tabs, the right dock and open-request
//! routing.
//!
//! * [`Workspace`] is the main window's root view (`key_context
//!   "Workspace"`). It owns the vault root, the tab list, the single mounted
//!   pane (only the active tab's pane is mounted, as in the original), the
//!   lazily created main views and the dock.
//! * Extension points: [`pane::PaneItem`] (what a tab hosts),
//!   [`view::MainView`] (agenda/search/tags/settings), [`view::ViewEvent`]
//!   and [`view::DockEvent`].

pub mod activity_bar;
pub mod dock;
pub mod pane;
pub mod status_bar;
pub mod tab_bar;
pub mod tabs;
pub mod view;

use fuchico_core::events::CoreEvent;
use fuchico_core::fs::file::{StatKind, fs_stat};
use fuchico_core::open_files::{OpenRequestHandlers, route_open_request};
use fuchico_core::watcher::VaultWatcher;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, MouseDownEvent, MouseMoveEvent,
    PathPromptOptions, Subscription, Window, div, px,
};

use crate::actions::*;
use crate::agenda::AgendaView;
use crate::backlinks::BacklinksPane;
use crate::bus::{self, AppEvent};
use crate::explorer::Explorer;
use crate::mermaid::MermaidPane;
use crate::palette::{self, PaletteKind};
use crate::search::SearchView;
use crate::settings::ui_state::{SettingsSection, UiState};
use crate::settings_view::SettingsView;
use crate::tags::TagsView;
use crate::theme::AppTheme;
use crate::ui::u;
use dock::{DOCK_DEFAULT_WIDTH, DOCK_HANDLE_WIDTH, Dock, DockDrag, backlinks_target};
use pane::{PaneContext, PaneEvent, PaneHandle, open_pane};
use tabs::Tabs;
use view::{DockEvent, MainView, ViewEvent};

pub const SIDEBAR_WIDTH: f32 = 260.;

/// `mainView`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MainViewKind {
    #[default]
    Editor,
    Agenda,
    Search,
    Tags,
    Settings,
}

/// The mounted pane and the tab path it was created for.
struct ActivePane {
    path: String,
    handle: PaneHandle,
}

pub struct Workspace {
    root: Option<String>,
    /// Startup `fs_stat` of `helix.lastRootPath` still running.
    restoring: bool,
    tabs: Tabs,
    pane: Option<ActivePane>,
    view: MainViewKind,
    explorer: Entity<Explorer>,
    agenda: Option<Entity<AgendaView>>,
    search: Option<Entity<SearchView>>,
    tags: Option<Entity<TagsView>>,
    settings: Option<Entity<SettingsView>>,
    tag_token: u32,
    dock: Option<Dock>,
    /// Subscription to the current dock panel(s); replaced with the panel.
    dock_subs: Vec<Subscription>,
    dock_width: f32,
    dock_drag: Option<DockDrag>,
    watcher: Option<VaultWatcher>,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
    /// Command palette / quick switcher overlays.
    pub palette: palette::PaletteHost,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let explorer = cx.new(|cx| Explorer::new(None, window, cx));
        let subs = vec![
            cx.subscribe_in(&explorer, window, |this, _, ev: &ViewEvent, window, cx| {
                this.on_view_event(ev, window, cx)
            }),
            bus::subscribe_in(window, cx, |this, ev, window, cx| this.on_bus(ev, window, cx)),
            cx.observe_window_appearance(window, |_, _, cx| crate::theme::apply::refresh(cx)),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Workspace {
            root: None,
            restoring: false,
            tabs: Tabs::default(),
            pane: None,
            view: MainViewKind::Editor,
            explorer,
            agenda: None,
            search: None,
            tags: None,
            settings: None,
            tag_token: 0,
            dock: None,
            dock_subs: Vec::new(),
            dock_width: DOCK_DEFAULT_WIDTH,
            dock_drag: None,
            watcher: None,
            focus,
            _subs: subs,
            palette: Default::default(),
        };
        this.restore_vault(window, cx);
        this
    }

    /// Keeps a subscription alive as long as the workspace (geometry, ...).
    pub fn hold(&mut self, sub: Subscription) {
        self._subs.push(sub);
    }

    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn tabs(&self) -> &Tabs {
        &self.tabs
    }

    pub fn main_view(&self) -> MainViewKind {
        self.view
    }

    #[cfg(test)]
    pub fn explorer(&self) -> &Entity<Explorer> {
        &self.explorer
    }

    // ----- vault lifecycle -------------------------------------------------

    /// `fs_stat(lastRootPath)` off the UI thread; renders an empty layout
    /// until it answers, then drains queued open requests.
    fn restore_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(last) = UiState::get(cx).last_root.clone() else {
            cx.defer_in(window, |this, window, cx| this.drain_open_requests(window, cx));
            return;
        };
        self.restoring = true;
        let probe = last.clone();
        let stat = cx.background_spawn(async move { fs_stat(probe) });
        cx.spawn_in(window, async move |this, cx| {
            let ok = matches!(stat.await, Ok(s) if matches!(s.kind, StatKind::Dir));
            this.update_in(cx, |this, window, cx| {
                this.restoring = false;
                if ok {
                    this.set_root(Some(last), window, cx);
                } else {
                    log::info!("last vault {last} is gone; forgetting it");
                    UiState::set_last_root(cx, None);
                }
                this.drain_open_requests(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Adopts a vault: closes every tab, persists `helix.lastRootPath`,
    /// restarts the watcher (off-thread) and hands the root to every view.
    /// The dock and main view are kept.
    pub fn set_root(&mut self, root: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        UiState::set_last_root(cx, root.clone());
        self.close_all_tabs(window, cx);
        if self.root == root {
            return;
        }
        self.root = root.clone();
        self.watcher = None;
        // No watcher under `cargo test`: its thread would trip gpui's
        // deterministic test scheduler (tests emit fs:changed themselves).
        if let Some(dir) = root.clone().filter(|_| !cfg!(test)) {
            let start = cx.background_spawn(async move {
                let res = VaultWatcher::start(&dir, bus::sink());
                (dir, res)
            });
            cx.spawn(async move |this, cx| {
                let (dir, res) = start.await;
                this.update(cx, |this, _| match res {
                    Ok(w) if this.root.as_deref() == Some(dir.as_str()) => this.watcher = Some(w),
                    Ok(_) => {}
                    Err(e) => log::warn!("watcher for {dir} failed: {e}"),
                })
                .ok();
            })
            .detach();
        }
        self.explorer.update(cx, |e, cx| e.set_root(root.clone(), cx));
        fn push<V: MainView>(v: &Option<Entity<V>>, root: &Option<String>, cx: &mut App) {
            if let Some(v) = v {
                v.update(cx, |v, cx| v.set_root(root.clone(), cx));
            }
        }
        push(&self.agenda, &root, cx);
        push(&self.search, &root, cx);
        push(&self.tags, &root, cx);
        push(&self.settings, &root, cx);
        if let Some(Dock::Backlinks(b)) = &self.dock {
            b.update(cx, |b, cx| b.set_root(root.clone(), cx));
        }
        cx.notify();
    }

    /// The native folder picker; cancel does nothing.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::t!(CommonOpenFolder).into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = rx.await
                && let Some(dir) = paths.pop()
            {
                let dir = dir.to_string_lossy().into_owned();
                this.update_in(cx, |this, window, cx| this.set_root(Some(dir), window, cx)).ok();
            }
        })
        .detach();
    }

    /// Routes every queued OS/CLI open request (issue #38 rules).
    pub fn drain_open_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.restoring {
            return;
        }
        let paths = crate::windows::OPEN_REQUESTS.take();
        if paths.is_empty() {
            return;
        }
        let plan = plan_open_requests(&paths, self.root.as_deref());
        for step in plan {
            match step {
                Step::SetRoot(dir) => self.set_root(Some(dir), window, cx),
                Step::Tab(p) => self.open_file(p, None, window, cx),
                Step::Window(p) => crate::windows::open_editor_window(p, self.root.clone(), cx),
            }
        }
    }

    // ----- tabs and panes --------------------------------------------------

    /// `openFile(path, focusLine?)`: open/activate the tab and force the
    /// editor view.
    pub fn open_file(&mut self, path: String, line: Option<u32>, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.open(&path, line);
        self.set_view(MainViewKind::Editor, window, cx);
        self.sync_pane(window, cx);
        if let (Some(line), Some(p)) = (line, &self.pane) {
            p.handle.reveal_line(line, window, cx);
        }
        cx.notify();
    }

    pub fn activate_tab(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.activate(path);
        self.sync_pane(window, cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.close(path);
        self.sync_pane(window, cx);
        cx.notify();
    }

    pub fn close_other_tabs(&mut self, keep: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.close_others(keep);
        self.sync_pane(window, cx);
        cx.notify();
    }

    pub fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.close_all();
        self.sync_pane(window, cx);
        cx.notify();
    }

    /// Focuses the mounted pane (editor-targeted palette commands dispatch
    /// from there). False without one.
    pub fn focus_active_pane(&self, window: &mut Window, cx: &mut App) -> bool {
        match &self.pane {
            Some(p) => {
                p.handle.focus(window, cx);
                true
            }
            None => false,
        }
    }

    /// Mounts the active tab's pane (remounting when the active path
    /// changed) and pushes the active file to the explorer and backlinks.
    fn sync_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.tabs.active.clone();
        let current = self.pane.as_ref().map(|p| p.path.clone());
        if active != current {
            self.pane = active.as_ref().map(|path| {
                let pcx = PaneContext { root: self.root.clone() };
                let handle = open_pane(path, pcx, window, cx, |this: &mut Workspace, ev, window, cx| {
                    this.on_pane_event(ev, window, cx)
                });
                if self.view == MainViewKind::Editor {
                    handle.focus(window, cx);
                }
                ActivePane { path: path.clone(), handle }
            });
            if self.pane.is_none() {
                window.focus(&self.focus, cx);
            }
        }
        self.explorer.update(cx, |e, cx| e.set_active_file(active.clone(), cx));
        if let Some(Dock::Backlinks(b)) = &self.dock {
            b.update(cx, |b, cx| b.set_target(backlinks_target(active.as_deref()), cx));
        }
    }

    fn on_pane_event(&mut self, ev: &PaneEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.pane.as_ref().map(|p| p.path.clone()) else { return };
        match ev {
            PaneEvent::DirtyChanged(d) => {
                if self.tabs.set_dirty(&path, *d) {
                    cx.notify();
                }
            }
            PaneEvent::OpenFile { path, line } => self.open_file(path.clone(), *line, window, cx),
            PaneEvent::OpenTag(tag) => self.open_tag(tag.clone(), window, cx),
            PaneEvent::OpenMermaid { block_key, initial_text } => {
                self.open_mermaid(block_key.clone(), crate::t!(AppDiagram), Some(initial_text.clone()), window, cx)
            }
            PaneEvent::OpenCommandPalette(q) => palette::open(self, PaletteKind::Commands { query: q.clone() }, window, cx),
            PaneEvent::Close => self.close_tab(&path, window, cx),
        }
    }

    fn on_view_event(&mut self, ev: &ViewEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            ViewEvent::OpenFile { path, line } => self.open_file(path.clone(), *line, window, cx),
            ViewEvent::OpenInNewWindow(p) => crate::windows::open_editor_window(p.clone(), self.root.clone(), cx),
            ViewEvent::OpenFolder => self.open_folder(window, cx),
            ViewEvent::PathRenamed { from, to } => {
                self.tabs.rename(from, to);
                UiState::rename_path(cx, from, to);
                if let Some(p) = &mut self.pane
                    && let Some(new) = tabs::renamed(&p.path, from, to)
                {
                    p.path = new.clone();
                    p.handle.set_path(new, cx);
                }
                self.sync_pane(window, cx);
                cx.notify();
            }
            ViewEvent::PathDeleted(p) => {
                self.tabs.delete(p);
                self.sync_pane(window, cx);
                cx.notify();
            }
        }
    }

    fn on_bus(&mut self, ev: &AppEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            AppEvent::Core(CoreEvent::OpenFiles(_)) => {
                self.drain_open_requests(window, cx);
                window.activate_window();
            }
            AppEvent::MermaidDockRequest { block_key, label } => {
                self.open_mermaid(block_key.clone(), label.clone(), None, window, cx)
            }
            _ => {}
        }
    }

    // ----- main views ------------------------------------------------------

    /// Switches `mainView`, creating the view on first use.
    pub fn set_view(&mut self, view: MainViewKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        fn hide<V: MainView>(v: &Option<Entity<V>>, window: &mut Window, cx: &mut App) {
            if let Some(v) = v {
                v.update(cx, |v, cx| v.on_hide(window, cx));
            }
        }
        match self.view {
            MainViewKind::Agenda => hide(&self.agenda, window, cx),
            MainViewKind::Search => hide(&self.search, window, cx),
            MainViewKind::Tags => hide(&self.tags, window, cx),
            MainViewKind::Settings => hide(&self.settings, window, cx),
            MainViewKind::Editor => {}
        }
        self.view = view;
        let root = self.root.clone();
        let sub = match view {
            MainViewKind::Editor => {
                match &self.pane {
                    Some(p) => p.handle.focus(window, cx),
                    None => window.focus(&self.focus, cx),
                }
                None
            }
            MainViewKind::Agenda => ensure(&mut self.agenda, root, window, cx, AgendaView::new),
            MainViewKind::Search => ensure(&mut self.search, root, window, cx, SearchView::new),
            MainViewKind::Tags => ensure(&mut self.tags, root, window, cx, TagsView::new),
            MainViewKind::Settings => ensure(&mut self.settings, root, window, cx, SettingsView::new),
        };
        self._subs.extend(sub);
        cx.notify();
    }

    /// `openTagFilter(tag)`.
    pub fn open_tag(&mut self, tag: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(MainViewKind::Tags, window, cx);
        self.tag_token += 1;
        let token = self.tag_token;
        if let Some(t) = &self.tags {
            t.update(cx, |t, cx| t.select_tag(tag, token, cx));
        }
    }

    /// Palette `openSettings(section)`.
    pub fn open_settings(&mut self, section: SettingsSection, window: &mut Window, cx: &mut Context<Self>) {
        UiState::set_settings_section(cx, section);
        self.set_view(MainViewKind::Settings, window, cx);
        if let Some(s) = &self.settings {
            s.update(cx, |_, cx| cx.notify());
        }
    }

    // ----- dock ------------------------------------------------------------

    pub fn open_mermaid(
        &mut self,
        block_key: String,
        label: String,
        initial_text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Dock::Mermaid { block_key: k, .. }) = &self.dock
            && *k == block_key
        {
            return;
        }
        let key = block_key.clone();
        let pane = cx.new(|cx| MermaidPane::new(key, label, initial_text, window, cx));
        self.dock_subs = vec![self.watch_dock(&pane, window, cx)];
        self.dock = Some(Dock::Mermaid { block_key, pane });
        cx.notify();
    }

    /// `toggleBacklinks`.
    pub fn toggle_backlinks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.dock, Some(Dock::Backlinks(_))) {
            self.dock = None;
            self.dock_subs.clear();
        } else {
            let target = backlinks_target(self.tabs.active.as_deref());
            let pane = cx.new(|cx| BacklinksPane::new(self.root.clone(), target, window, cx));
            let open = cx.subscribe_in(&pane, window, |this, _, ev: &ViewEvent, window, cx| {
                this.on_view_event(ev, window, cx)
            });
            self.dock_subs = vec![self.watch_dock(&pane, window, cx), open];
            self.dock = Some(Dock::Backlinks(pane));
        }
        cx.notify();
    }

    pub fn backlinks_open(&self) -> bool {
        matches!(self.dock, Some(Dock::Backlinks(_)))
    }

    fn watch_dock<T: EventEmitter<DockEvent>>(
        &self,
        pane: &Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(pane, window, |this, _, ev: &DockEvent, _, cx| match ev {
            DockEvent::Close => {
                this.dock = None;
                this.dock_subs.clear();
                cx.notify();
            }
        })
    }

    fn on_dock_drag(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(drag) = self.dock_drag {
            if ev.pressed_button != Some(MouseButton::Left) {
                self.dock_drag = None;
            } else {
                self.dock_width = drag.width_at(f32::from(ev.position.x));
            }
            cx.notify();
        }
    }

    // ----- actions ---------------------------------------------------------

    fn register_actions<E: InteractiveElement>(&self, el: E, cx: &mut Context<Self>) -> E {
        el.on_action(cx.listener(|this, _: &OpenFolder, w, cx| this.open_folder(w, cx)))
            .on_action(cx.listener(|this, _: &ShowFiles, w, cx| this.set_view(MainViewKind::Editor, w, cx)))
            .on_action(cx.listener(|this, _: &ShowAgenda, w, cx| this.set_view(MainViewKind::Agenda, w, cx)))
            .on_action(cx.listener(|this, _: &ShowSearch, w, cx| this.set_view(MainViewKind::Search, w, cx)))
            .on_action(cx.listener(|this, _: &ShowTags, w, cx| this.set_view(MainViewKind::Tags, w, cx)))
            .on_action(cx.listener(|this, _: &ShowSettings, w, cx| this.set_view(MainViewKind::Settings, w, cx)))
            .on_action(
                cx.listener(|this, _: &ShowShortcuts, w, cx| this.open_settings(SettingsSection::Shortcuts, w, cx)),
            )
            .on_action(cx.listener(|this, _: &CloseTab, w, cx| {
                if let Some(p) = this.tabs.active.clone() {
                    this.close_tab(&p, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CloseOtherTabs, w, cx| {
                if let Some(p) = this.tabs.active.clone() {
                    this.close_other_tabs(&p, w, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CloseAllTabs, w, cx| this.close_all_tabs(w, cx)))
            .on_action(cx.listener(|this, _: &NextTab, w, cx| {
                this.tabs.cycle(1);
                this.sync_pane(w, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, w, cx| {
                this.tabs.cycle(-1);
                this.sync_pane(w, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleDock, w, cx| this.toggle_backlinks(w, cx)))
            .on_action(cx.listener(|this, _: &Save, w, cx| {
                if let Some(p) = &this.pane {
                    p.handle.save(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CommandPalette, w, cx| {
                palette::open(this, PaletteKind::Commands { query: String::new() }, w, cx)
            }))
            .on_action(cx.listener(|this, _: &QuickOpen, w, cx| palette::open(this, PaletteKind::QuickOpen, w, cx)))
    }

    // ----- rendering -------------------------------------------------------

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let base = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(u(SIDEBAR_WIDTH))
            .h_full()
            .overflow_hidden()
            .border_r_1()
            .border_color(th.colors.border);
        if self.root.is_some() {
            return base.child(self.explorer.clone());
        }
        let (bg, fg) = (th.colors.primary, th.colors.primary_foreground);
        let radius = u(th.radius.md);
        base.items_center().justify_center().child(
            div()
                .id("no-vault-open-folder")
                .flex()
                .items_center()
                .gap(u(6.))
                .px(u(12.))
                .py(u(6.))
                .rounded(radius)
                .bg(bg)
                .text_color(fg)
                .text_size(u(13.))
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .on_click(cx.listener(|this, _, w, cx| this.open_folder(w, cx)))
                .child(gpui_kit::component::Icon::new(gpui_kit::component::IconName::FolderOpen).size(u(14.)))
                .child(crate::t!(CommonOpenFolder)),
        )
    }

    fn render_main(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        match self.view {
            MainViewKind::Agenda => self.agenda.clone().map(|v| v.into_any_element()),
            MainViewKind::Search => self.search.clone().map(|v| v.into_any_element()),
            MainViewKind::Tags => self.tags.clone().map(|v| v.into_any_element()),
            MainViewKind::Settings => self.settings.clone().map(|v| v.into_any_element()),
            MainViewKind::Editor => Some(self.render_editor_area(cx).into_any_element()),
        }
        .unwrap_or_else(|| div().into_any_element())
    }

    fn render_editor_area(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let area = div().flex().flex_col().size_full().overflow_hidden();
        match &self.pane {
            None => area.child(crate::ui::empty_state(crate::t!(AppNoFileOpen), cx)),
            Some(p) => {
                let view = p.handle.view();
                // Explorer rows dropped on the editor insert links.
                let drop_bg = crate::theme::alpha(AppTheme::get(cx).colors.accent, 0.25);
                let drop = cx.listener(|this, paths: &crate::explorer::DraggedPaths, window, cx| {
                    if let Some(p) = &this.pane {
                        p.handle.insert_dropped_paths(&paths.0, window, cx);
                    }
                });
                area.child(self.render_tab_bar(cx)).child(
                    div()
                        .id("editor-drop")
                        .flex_1()
                        .min_h_0()
                        .drag_over::<crate::explorer::DraggedPaths>(move |s, _, _, _| s.bg(drop_bg))
                        .on_drop(drop)
                        .child(view),
                )
            }
        }
    }

    fn render_dock(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let dock = self.dock.as_ref()?;
        let th = AppTheme::get(cx);
        let (border, bg, hover) = (th.colors.border, th.panel_bg(), crate::theme::alpha(th.colors.accent, 0.3));
        let handle = div()
            .id("dock-resizer")
            .absolute()
            .left_0()
            .top_0()
            .h_full()
            .w(u(DOCK_HANDLE_WIDTH))
            .cursor_col_resize()
            .hover(move |s| s.bg(hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    this.dock_drag = Some(DockDrag { start_x: f32::from(ev.position.x), start_width: this.dock_width });
                    cx.stop_propagation();
                    cx.notify();
                }),
            );
        Some(
            div()
                .relative()
                .flex_none()
                .h_full()
                .w(px(self.dock_width))
                .border_l_1()
                .border_color(border)
                .bg(bg)
                .child(dock.view())
                .child(handle),
        )
    }
}

/// Creates a main view on first use; returns the shell's subscription to it.
fn ensure<V: MainView>(
    slot: &mut Option<Entity<V>>,
    root: Option<String>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
    new: fn(Option<String>, &mut Window, &mut Context<V>) -> V,
) -> Option<Subscription> {
    if slot.is_some() {
        return None;
    }
    let view = cx.new(|cx| new(root, window, cx));
    let sub = cx.subscribe_in(&view, window, |this, _, ev: &ViewEvent, window, cx| this.on_view_event(ev, window, cx));
    *slot = Some(view);
    Some(sub)
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let root = div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            .font_family(th.ui_font.clone())
            .text_size(u(13.));
        if self.restoring {
            // The original renders an empty `.app-layout` while restoring.
            return root;
        }
        let root = self.register_actions(root, cx);
        let main = self.render_main(cx);
        root.when(self.dock_drag.is_some(), |d| {
            d.cursor_col_resize().on_mouse_move(cx.listener(Self::on_dock_drag)).on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.dock_drag = None;
                    cx.notify();
                }),
            )
        })
        .child(self.render_activity_bar(cx))
        .child(self.render_sidebar(cx))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(div().flex_1().min_w_0().h_full().overflow_hidden().child(main))
                .children(self.render_dock(cx)),
        )
        .children(palette::render_overlays(self))
    }
}

/// One routing decision of [`route_open_request`].
#[derive(Debug, PartialEq)]
enum Step {
    SetRoot(String),
    Tab(String),
    Window(String),
}

#[derive(Default)]
struct Plan(Vec<Step>);

impl OpenRequestHandlers for Plan {
    fn set_root(&mut self, dir: &str) {
        self.0.push(Step::SetRoot(dir.to_string()));
    }
    fn open_in_vault(&mut self, path: &str) {
        self.0.push(Step::Tab(path.to_string()));
    }
    fn open_elsewhere(&mut self, path: &str) {
        self.0.push(Step::Window(path.to_string()));
    }
}

/// Routes a batch in order, threading the root returned by each step.
fn plan_open_requests(paths: &[String], root: Option<&str>) -> Vec<Step> {
    let mut plan = Plan::default();
    let mut root = root.map(str::to_string);
    for p in paths {
        root = route_open_request(p, root.as_deref(), &mut plan);
    }
    plan.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_request_batch_uses_the_root_from_each_step() {
        let paths: Vec<String> = ["/v/a.md", "/v/sub/b.md", "/elsewhere/c.md"].map(String::from).into();
        assert_eq!(
            plan_open_requests(&paths, None),
            vec![
                Step::SetRoot("/v".into()),
                Step::Tab("/v/a.md".into()),
                Step::Tab("/v/sub/b.md".into()),
                Step::Window("/elsewhere/c.md".into()),
            ]
        );
    }

    #[test]
    fn inside_vault_is_a_prefix_with_slash() {
        let paths = vec!["/vault/a.md".to_string()];
        assert_eq!(plan_open_requests(&paths, Some("/v")), vec![Step::Window("/vault/a.md".into())]);
    }
}
