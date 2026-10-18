//! Tags view (spec 03 "Tags view", original `src/modules/tags/TagsView.tsx`
//! + `lib/useTagIndex.ts`).
//!
//! * Left: one chip per tag (`#tag` + count) from `fuchico_core::tags::tags_scan`
//!   (already sorted by count desc, then name). Clicking a chip toggles it.
//! * Right: the active tag's notes (name + full path); click opens, right-click
//!   is the shared file-row menu.
//! * Re-scans off the UI thread on `fs:file-written` (original trigger) and
//!   on `fs:changed` (watcher superset, see DECISIONS); overlapping triggers
//!   coalesce into one follow-up scan.
//! * [`TagsView::select_tag`] is the `selectedTag` + `selectedTagToken` prop.

use std::rc::Rc;

use fuchico_core::events::CoreEvent;
use fuchico_core::tags::{TagEntry, tags_scan};
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, EventEmitter, SharedString, Subscription, Window, div};

use crate::assets::AppIcon;
use crate::bus::{self, AppEvent};
use crate::theme::{AppTheme, alpha};
use crate::ui::{build_menu, empty_state, file_row_menu, u};
use crate::workspace::tabs::basename;
use crate::workspace::view::{MainView, ViewEvent};

/// `useTagIndex` state.
pub enum TagIndex {
    Idle,
    Loading,
    Loaded(Rc<Vec<TagEntry>>),
    Error(String),
}

pub struct TagsView {
    root: Option<String>,
    index: TagIndex,
    /// The highlighted chip (component state in the original).
    active: Option<String>,
    /// The last `selectedTag` prop: re-applied when the view is shown again.
    selected: Option<String>,
    /// Bumped on every selection so re-selecting the same tag re-triggers.
    token: u32,
    scanning: bool,
    rescan: bool,
    /// Bumped on root change so a stale scan's result is dropped.
    generation: u64,
    _sub: Subscription,
}

impl TagsView {
    pub fn new(root: Option<String>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = bus::subscribe(cx, |this: &mut TagsView, ev, cx| {
            if let AppEvent::Core(CoreEvent::FileWritten { .. } | CoreEvent::FsChanged { .. }) = ev {
                this.scan(cx);
            }
        });
        let mut this = TagsView {
            root: None,
            index: TagIndex::Idle,
            active: None,
            selected: None,
            token: 0,
            scanning: false,
            rescan: false,
            generation: 0,
            _sub: sub,
        };
        this.set_root(root, cx);
        this
    }

    /// `selectedTag` / `selectedTagToken`: a truthy tag becomes active.
    pub fn select_tag(&mut self, tag: String, token: u32, cx: &mut Context<Self>) {
        self.token = token;
        if !tag.is_empty() {
            self.active = Some(tag.clone());
            self.selected = Some(tag);
        }
        cx.notify();
    }

    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn index(&self) -> &TagIndex {
        &self.index
    }

    /// `scan(root)`: "loading" only before the first result; errors replace
    /// the list.
    fn scan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else { return };
        if self.scanning {
            self.rescan = true;
            return;
        }
        self.scanning = true;
        if !matches!(self.index, TagIndex::Loaded(_)) {
            self.index = TagIndex::Loading;
            cx.notify();
        }
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let res = cx.background_spawn(async move { tags_scan(root) }).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.scanning = false;
                this.index = match res {
                    Ok(entries) => TagIndex::Loaded(Rc::new(entries)),
                    Err(e) => TagIndex::Error(e),
                };
                if std::mem::take(&mut this.rescan) {
                    this.scan(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle(&mut self, tag: &str, cx: &mut Context<Self>) {
        self.active = toggled(self.active.as_deref(), tag);
        cx.notify();
    }

    fn open(&self, file: &str, cx: &mut Context<Self>) {
        cx.emit(ViewEvent::OpenFile { path: file.to_string(), line: None });
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let th = AppTheme::get(cx).clone();
        let status = |d: gpui_kit::Div| d.px(u(4.)).py(u(8.)).text_size(u(12.)).text_color(th.colors.muted_foreground);
        let mut children: Vec<AnyElement> = Vec::new();
        match &self.index {
            TagIndex::Loading => children.push(status(div()).child(crate::t!(CommonScanning)).into_any_element()),
            TagIndex::Error(e) => {
                children.push(status(div()).text_color(th.colors.destructive).child(e.clone()).into_any_element())
            }
            TagIndex::Loaded(entries) if entries.is_empty() => {
                children.push(status(div()).child(no_tags_message(&th)).into_any_element())
            }
            TagIndex::Loaded(entries) => {
                let view = cx.entity().downgrade();
                for (i, entry) in entries.iter().enumerate() {
                    let on = self.active.as_deref() == Some(entry.tag.as_str());
                    let kw = th.colors.syntax_keyword;
                    let hover = th.fg_alpha(0.05);
                    let (tag, view) = (entry.tag.clone(), view.clone());
                    children.push(
                        div()
                            .id(("tag-chip", i))
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(u(4.))
                            .px(u(10.))
                            .py(u(3.))
                            .border_1()
                            .rounded_full()
                            .text_size(u(12.))
                            .cursor_pointer()
                            .map(|d| {
                                if on {
                                    d.bg(alpha(kw, 0.18)).border_color(alpha(kw, 0.45)).text_color(kw)
                                } else {
                                    d.border_color(th.colors.border).text_color(th.colors.foreground).hover(move |s| s.bg(hover))
                                }
                            })
                            .child(Icon::new(AppIcon::Hash).size(u(11.)).flex_none())
                            .child(entry.tag.clone())
                            .child(
                                div()
                                    .text_size(u(11.))
                                    .map(|d| if on { d.opacity(0.75) } else { d.text_color(th.colors.muted_foreground) })
                                    .child(entry.count.to_string()),
                            )
                            .on_click(move |_, _, cx| {
                                view.update(cx, |this, cx| this.toggle(&tag, cx)).ok();
                            })
                            .into_any_element(),
                    );
                }
            }
            TagIndex::Idle => {}
        }
        div()
            .id("tags-list")
            .w(u(260.))
            .flex_none()
            .flex()
            .flex_wrap()
            .content_start()
            .gap(u(6.))
            .p(u(12.))
            .overflow_y_scroll()
            .border_r_1()
            .border_color(th.colors.border)
            .children(children)
            .into_any_element()
    }

    fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let th = AppTheme::get(cx).clone();
        let entry = match (&self.index, &self.active) {
            (TagIndex::Loaded(entries), Some(a)) => entries.iter().find(|e| &e.tag == a),
            _ => None,
        };
        let mut children: Vec<AnyElement> = Vec::new();
        match entry {
            None => children.push(
                div()
                    .px(u(4.))
                    .py(u(8.))
                    .text_size(u(12.))
                    .text_color(th.colors.muted_foreground)
                    .child(crate::t!(TagsSelectTag))
                    .into_any_element(),
            ),
            Some(entry) => {
                let view = cx.entity().downgrade();
                let hover = th.fg_alpha(0.05);
                for (i, file) in entry.files.iter().enumerate() {
                    let (f1, f2, v1, v2) = (file.clone(), file.clone(), view.clone(), view.clone());
                    let tip: SharedString = file.clone().into();
                    children.push(
                        div()
                            .id(("tag-file", i))
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(u(2.))
                            .w_full()
                            .px(u(8.))
                            .py(u(6.))
                            .rounded(u(th.radius.sm))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .text_size(u(12.))
                            .child(div().text_color(th.colors.foreground).child(basename(file).to_string()))
                            .child(
                                div()
                                    .id(("tag-file-path", i))
                                    .max_w_full()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(u(11.))
                                    .text_color(th.colors.muted_foreground)
                                    .child(file.clone())
                                    .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx)),
                            )
                            .on_click(move |_, _, cx| {
                                v1.update(cx, |this, cx| this.open(&f1, cx)).ok();
                            })
                            .context_menu(move |menu, _, _| {
                                let (v, f) = (v2.clone(), f2.clone());
                                build_menu(
                                    menu,
                                    file_row_menu(
                                        &f2,
                                        move |_, cx| {
                                            v.update(cx, |this, cx| this.open(&f, cx)).ok();
                                        },
                                        Vec::new(),
                                    ),
                                )
                            })
                            .into_any_element(),
                    );
                }
            }
        }
        div()
            .id("tags-files")
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .px(u(12.))
            .py(u(8.))
            .children(children)
            .into_any_element()
    }
}

/// Chip click: select `tag`, or clear it when it is already active.
fn toggled(active: Option<&str>, tag: &str) -> Option<String> {
    if active == Some(tag) { None } else { Some(tag.to_string()) }
}

/// "No tags yet. Use `#tag` in a note or a frontmatter `tags:` list.",
/// laid out as wrapping words with the two code spans in the mono font.
fn no_tags_message(th: &AppTheme) -> AnyElement {
    let code = |s: &'static str| {
        div()
            .font_family(th.mono_font.clone())
            .px(u(3.))
            .rounded(u(3.))
            .bg(th.fg_alpha(0.08))
            .child(s)
            .into_any_element()
    };
    let words = |s: String| s.split_whitespace().map(|w| div().child(w.to_string()).into_any_element()).collect::<Vec<_>>();
    let mut parts = words(crate::t!(TagsNoTagsPrefix));
    parts.push(code("#tag"));
    parts.extend(words(crate::t!(TagsNoTagsMid)));
    parts.push(code("tags:"));
    parts.extend(words(crate::t!(TagsNoTagsSuffix)));
    div().flex().flex_wrap().items_center().gap_x(u(4.)).children(parts).into_any_element()
}

impl EventEmitter<ViewEvent> for TagsView {}

impl MainView for TagsView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        if self.root == root && !matches!(self.index, TagIndex::Idle) {
            return;
        }
        self.root = root;
        self.generation += 1;
        self.scanning = false;
        self.rescan = false;
        self.index = TagIndex::Idle;
        self.scan(cx);
        cx.notify();
    }

    /// The original unmounts; on remount `active` restarts from App's last
    /// `selectedTag`.
    fn on_hide(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.active = self.selected.clone();
        cx.notify();
    }
}

impl Render for TagsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.root.is_none() {
            return empty_state(crate::t!(TagsOpenFolder), cx).into_any_element();
        }
        div()
            .flex()
            .size_full()
            .min_h_0()
            .child(self.render_list(cx))
            .child(self.render_files(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_click_toggles() {
        assert_eq!(toggled(None, "a"), Some("a".into()));
        assert_eq!(toggled(Some("a"), "a"), None);
        assert_eq!(toggled(Some("a"), "b"), Some("b".into()));
    }
}
