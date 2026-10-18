//! File explorer (spec 01-shell "File explorer") — PARTIAL.
//!
//! Implemented: header (vault name + 5 buttons), lazy per-directory loading
//! off the UI thread, expand/collapse, open on click, active-file selection,
//! refresh on `fs:changed` from any window, virtualized 24px rows, the file
//! row context menu (open in new window, reveal, copy path, delete with
//! confirmation).
//!
//! Placeholders: inline create/rename inputs, multi-select, keyboard
//! navigation/type-ahead, cut/paste, drag-and-drop and OS-file import.

mod tree;

use std::collections::HashSet;
use std::rc::Rc;

use fuchico_core::events::CoreEvent;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, MouseButton, SharedString, Subscription,
    UniformListScrollHandle, Window, div, uniform_list,
};

pub use tree::{DirState, Entry, Row, flatten};

use crate::assets::AppIcon;
use crate::bus::{self, AppEvent};
use crate::theme::AppTheme;
use crate::ui::{ConfirmOptions, MenuEntry, build_menu, confirm, icon_button, u};
use crate::workspace::tabs::basename;
use crate::workspace::view::ViewEvent;

pub const ROW_HEIGHT: f32 = 24.;

pub struct Explorer {
    root: Option<String>,
    tree: tree::Tree,
    expanded: HashSet<String>,
    selected: Option<String>,
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
            selected: None,
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
        self.selected = None;
        if let Some(r) = self.root.clone() {
            self.refetch(r, cx);
        }
        cx.notify();
    }

    /// Auto-select the active tab's file.
    pub fn set_active_file(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if path.is_some() && self.selected != path {
            self.selected = path;
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

    fn click_row(&mut self, row: &Row, cx: &mut Context<Self>) {
        let Row::Entry { entry, .. } = row else { return };
        self.selected = Some(entry.path.clone());
        if entry.is_dir {
            self.toggle(&entry.path.clone(), cx);
        } else {
            cx.emit(ViewEvent::OpenFile { path: entry.path.clone(), line: None });
        }
        cx.notify();
    }

    fn refresh_root(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else { return };
        self.refetch(root.clone(), cx);
        // `notifyFsChanged(root)`: other windows and indexes refresh too.
        bus::emit(cx, AppEvent::Core(CoreEvent::FsChanged { dir: root, from: Some("main".into()) }));
    }

    fn delete(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let name = format!("\"{}\"", basename(&path));
        let this = cx.entity().downgrade();
        confirm(window, cx, ConfirmOptions::new(crate::t!(ExplorerConfirmDelete, name = name)).danger(), move |_, cx| {
            let path = path.clone();
            let this = this.clone();
            let task = cx.background_spawn({
                let path = path.clone();
                async move { fuchico_core::fs::mutate::fs_delete(path) }
            });
            cx.spawn(async move |cx| {
                if let Err(e) = task.await {
                    log::warn!("delete failed: {e}");
                    return;
                }
                this.update(cx, |this, cx| {
                    cx.emit(ViewEvent::PathDeleted(path.clone()));
                    if let Some(parent) = tree::parent_dir(&path) {
                        this.refetch(parent, cx);
                    }
                })
                .ok();
            })
            .detach();
        });
    }

    fn row_menu(&self, entry: &Entry, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let this = cx.entity().downgrade();
        let path = entry.path.clone();
        let mut items = Vec::new();
        if !entry.is_dir {
            let (t, p) = (this.clone(), path.clone());
            items.push(
                MenuEntry::item(crate::t!(TabsOpenInNewWindow), move |_, cx| {
                    t.update(cx, |_, cx| cx.emit(ViewEvent::OpenInNewWindow(p.clone()))).ok();
                })
                .icon(AppIcon::AppWindow),
            );
        }
        let p = path.clone();
        items.push(MenuEntry::item(crate::t!(CommonRevealInFileManager), move |_, _| reveal(&p)).icon(IconName::Eye));
        let p = path.clone();
        items.push(
            MenuEntry::item(crate::t!(CommonCopyPath), move |_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(p.clone()))
            })
            .icon(IconName::Copy),
        );
        items.push(MenuEntry::Separator);
        let (t, p) = (this, path);
        items.push(
            MenuEntry::item(crate::t!(CommonDelete), move |window, cx| {
                t.update(cx, |this, cx| this.delete(p.clone(), window, cx)).ok();
            })
            .icon(AppIcon::Trash),
        );
        items
    }

    fn render_header(&self, root: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let title: SharedString = basename(root).to_string().into();
        let full: SharedString = root.to_string().into();
        let open_folder = cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ViewEvent::OpenFolder));
        let refresh = cx.listener(|this, _: &ClickEvent, _, cx| this.refresh_root(cx));
        let todo = |what: &'static str| move |_: &ClickEvent, _: &mut Window, _: &mut App| log::info!("explorer: {what} not implemented yet");
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(4.))
            .h(u(32.))
            .px(u(6.))
            .border_b_1()
            .border_color(th.colors.border)
            .child(
                div()
                    .id("explorer-title")
                    .flex_1()
                    .min_w_0()
                    .pl(u(4.))
                    .text_size(u(12.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(title)
                    .tooltip(move |w, cx| gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(w, cx)),
            )
            .child(icon_button("ex-open", IconName::FolderOpen, 24., 14., crate::t!(ExplorerOpenFolderAction), false, cx, open_folder))
            .child(icon_button("ex-new-file", AppIcon::FilePlus, 24., 14., crate::t!(ExplorerNewFile), false, cx, todo("new file")))
            .child(icon_button("ex-new-folder", AppIcon::FolderPlus, 24., 14., crate::t!(ExplorerNewFolder), false, cx, todo("new folder")))
            .child(icon_button("ex-new-drawing", AppIcon::PenTool, 24., 14., crate::t!(ExplorerNewDrawing), false, cx, todo("new drawing")))
            .child(icon_button("ex-refresh", IconName::RefreshCw, 24., 14., crate::t!(ExplorerRefresh), false, cx, refresh))
    }
}

fn reveal(path: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open")
        .arg(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("/")))
        .spawn();
    if let Err(e) = r {
        log::warn!("reveal failed: {e}");
    }
}

impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let Some(root) = self.root.clone() else {
            return div()
                .p(u(12.))
                .text_size(u(11.))
                .text_color(th.colors.muted_foreground)
                .child(crate::t!(ExplorerNoFolderOpen))
                .into_any_element();
        };
        let rows: Rc<Vec<Row>> = Rc::new(flatten(&self.tree, &root, &self.expanded));
        let body = match self.tree.get(&root) {
            Some(DirState::Loaded(_)) => {
                let view = cx.entity().downgrade();
                let rows2 = rows.clone();
                let selected = self.selected.clone();
                uniform_list("explorer-rows", rows.len(), move |range, _window, cx| {
                    let Some(view) = view.upgrade() else { return Vec::new() };
                    range
                        .map(|i| render_row(&view, &rows2[i], i, selected.as_deref(), cx))
                        .collect::<Vec<_>>()
                })
                .track_scroll(&self.scroll)
                .flex_1()
                .into_any_element()
            }
            Some(DirState::Error(e)) => status_line(e.clone(), th.colors.destructive, 12.).into_any_element(),
            _ => status_line(crate::t!(CommonLoading), th.colors.muted_foreground, 12.).into_any_element(),
        };
        div()
            .id("explorer")
            .track_focus(&self.focus)
            .key_context("Explorer")
            .flex()
            .flex_col()
            .size_full()
            .child(self.render_header(&root, cx))
            .child(div().flex().flex_col().flex_1().min_h_0().pt(u(4.)).child(body))
            .into_any_element()
    }
}

fn status_line(text: String, color: gpui_kit::Hsla, pad_left: f32) -> impl IntoElement {
    div().py(u(8.)).pl(u(pad_left)).pr(u(12.)).text_size(u(11.)).text_color(color).child(text)
}

fn render_row(view: &gpui_kit::Entity<Explorer>, row: &Row, ix: usize, selected: Option<&str>, cx: &mut App) -> gpui_kit::AnyElement {
    let th = AppTheme::get(cx).clone();
    match row {
        Row::Status { depth, text, error } => div()
            .h(u(ROW_HEIGHT))
            .flex()
            .items_center()
            .pl(u(6. + (*depth as f32 + 1.) * 12. + 18.))
            .text_size(u(11.))
            .text_color(if *error { th.colors.destructive } else { th.colors.muted_foreground })
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(text.clone())
            .into_any_element(),
        Row::Entry { entry, depth, expanded } => {
            let is_sel = selected == Some(entry.path.as_str());
            let icon: Icon = if entry.is_dir {
                IconName::Folder.into()
            } else if entry.name.to_lowercase().ends_with(".excalidraw") {
                AppIcon::PenTool.into()
            } else {
                IconName::File.into()
            };
            let chevron = div().flex_none().size(u(14.)).flex().items_center().justify_center().when(entry.is_dir, |d| {
                d.child(
                    Icon::new(IconName::ChevronRight)
                        .size(u(12.))
                        .when(*expanded, |i| i.rotate(gpui_kit::radians(std::f32::consts::FRAC_PI_2))),
                )
            });
            let hover = th.colors.accent.opacity(0.7);
            let row_c = row.clone();
            let menu_entry = entry.clone();
            let v1 = view.clone();
            let v2 = view.clone();
            let v3 = view.clone();
            let path_sel = entry.path.clone();
            let tip: SharedString = entry.path.clone().into();
            div()
                .id(("ex-row", ix))
                .h(u(ROW_HEIGHT))
                .flex()
                .items_center()
                .gap(u(6.))
                .pl(u(6. + *depth as f32 * 12.))
                .pr(u(6.))
                .text_size(u(13.))
                .cursor_pointer()
                .when(is_sel, |d| d.bg(th.colors.accent).text_color(th.colors.accent_foreground))
                .when(!is_sel, |d| d.hover(move |s| s.bg(hover)))
                .child(chevron)
                .child(icon.size(u(14.)).flex_none())
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(entry.name.clone()))
                .tooltip(move |w, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(w, cx))
                .on_click(move |_, _, cx| v1.update(cx, |this, cx| this.click_row(&row_c, cx)))
                .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                    v2.update(cx, |this, cx| {
                        this.selected = Some(path_sel.clone());
                        cx.notify();
                    })
                })
                .context_menu(move |menu, _, cx| {
                    let items = v3.update(cx, |this, cx| this.row_menu(&menu_entry, cx));
                    build_menu(menu, items)
                })
                .into_any_element()
        }
    }
}
