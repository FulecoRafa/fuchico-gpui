//! Backlinks dock panel (spec 03 "Backlinks", original
//! `src/modules/backlinks/BacklinksPane.tsx` + `lib/useBacklinks.ts`).
//!
//! * Index: `fuchico_core::links::links_scan` + the vault's markdown files,
//!   scanned off the UI thread on open/root change and on every
//!   `fs:file-written` / `fs:changed` (overlapping triggers coalesce).
//! * Groups: `fuchico_core::backlinks::backlinks_for(refs, target, files)`,
//!   recomputed when the index or the target (active tab) changes.
//! * File row → open; line row → open at that line; right-click → the shared
//!   file-row menu. Header: Link2 icon, title, count badge, close button
//!   ([`DockEvent::Close`]).
//!
//! Contract: `BacklinksPane::new(root, target, window, cx)`,
//! [`BacklinksPane::set_root`], [`BacklinksPane::set_target`] whenever the
//! active tab changes (`None` for no tab or a `.excalidraw` path).

use std::rc::Rc;

use fuchico_core::backlinks::{BacklinkGroup, backlinks_for};
use fuchico_core::events::CoreEvent;
use fuchico_core::fs::tree::fs_list_markdown_files;
use fuchico_core::links::{LinkRef, links_scan};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, EventEmitter, FontWeight, SharedString, Subscription, WeakEntity, Window, div};

use crate::assets::AppIcon;
use crate::bus::{self, AppEvent};
use crate::theme::{AppTheme, alpha};
use crate::ui::{build_menu, file_row_menu, u, with_context_menu};
use crate::workspace::view::{DockEvent, ViewEvent};

/// `useLinkIndex` state (plus the vault file list `backlinksFor` needs).
pub enum LinkIndex {
    Idle,
    Loading,
    Loaded { refs: Rc<Vec<LinkRef>>, files: Rc<Vec<String>> },
    Error(String),
}

pub struct BacklinksPane {
    root: Option<String>,
    target: Option<String>,
    index: LinkIndex,
    groups: Rc<Vec<BacklinkGroup>>,
    scanning: bool,
    rescan: bool,
    generation: u64,
    _sub: Subscription,
}

impl BacklinksPane {
    pub fn new(root: Option<String>, target: Option<String>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = bus::subscribe(cx, |this: &mut BacklinksPane, ev, cx| {
            if let AppEvent::Core(CoreEvent::FileWritten { .. } | CoreEvent::FsChanged { .. }) = ev {
                this.scan(cx);
            }
        });
        let mut this = BacklinksPane {
            root: None,
            target,
            index: LinkIndex::Idle,
            groups: Rc::default(),
            scanning: false,
            rescan: false,
            generation: 0,
            _sub: sub,
        };
        this.set_root(root, cx);
        this
    }

    pub fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        if self.root == root && !matches!(self.index, LinkIndex::Idle) {
            return;
        }
        self.root = root;
        self.generation += 1;
        self.scanning = false;
        self.rescan = false;
        self.index = LinkIndex::Idle;
        self.regroup();
        self.scan(cx);
        cx.notify();
    }

    pub fn set_target(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        if self.target != target {
            self.target = target;
            self.regroup();
            cx.notify();
        }
    }

    pub fn groups(&self) -> &[BacklinkGroup] {
        &self.groups
    }

    fn regroup(&mut self) {
        self.groups = Rc::new(match (&self.index, &self.target) {
            (LinkIndex::Loaded { refs, files }, Some(t)) => backlinks_for(refs, t, files),
            _ => Vec::new(),
        });
    }

    /// `scan(root)`: "loading" only before the first result.
    fn scan(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else { return };
        if self.scanning {
            self.rescan = true;
            return;
        }
        self.scanning = true;
        if !matches!(self.index, LinkIndex::Loaded { .. }) {
            self.index = LinkIndex::Loading;
            cx.notify();
        }
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_spawn(async move {
                    let refs = links_scan(root.clone())?;
                    let files = fs_list_markdown_files(root)?;
                    Ok::<_, String>((refs, files))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.scanning = false;
                this.index = match res {
                    Ok((refs, files)) => LinkIndex::Loaded { refs: Rc::new(refs), files: Rc::new(files) },
                    Err(e) => LinkIndex::Error(e),
                };
                this.regroup();
                if std::mem::take(&mut this.rescan) {
                    this.scan(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open(&self, path: &str, line: Option<usize>, cx: &mut Context<Self>) {
        cx.emit(ViewEvent::OpenFile { path: path.to_string(), line: line.map(|l| l as u32) });
    }

    fn render_header(&self, total: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let th = AppTheme::get(cx);
        let close_tip: SharedString = crate::t!(CommonClose).into();
        let (hover_bg, hover_fg) = (alpha(th.colors.accent, 0.12), th.colors.accent_foreground);
        let badge = (self.target.is_some() && total > 0).then(|| {
            div()
                .ml(u(6.))
                .px(u(6.))
                .rounded_full()
                .bg(th.colors.secondary)
                .text_color(th.colors.secondary_foreground)
                .text_size(u(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(total.to_string())
        });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .px(u(10.))
            .py(u(6.))
            .border_b_1()
            .border_color(th.colors.border)
            .bg(th.colors.card)
            .child(Icon::new(AppIcon::Link2).size(u(14.)).text_color(th.colors.foreground))
            .child(
                div()
                    .mr_auto()
                    .flex()
                    .items_center()
                    .text_size(u(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(th.colors.foreground)
                    .child(crate::t!(BacklinksTitle))
                    .children(badge),
            )
            .child(
                div()
                    .id("backlinks-close")
                    .flex()
                    .items_center()
                    .px(u(7.))
                    .py(u(3.))
                    .border_1()
                    .border_color(th.colors.border)
                    .rounded(u(th.radius.sm))
                    .text_color(th.colors.muted_foreground)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                    .child(Icon::new(AppIcon::X).size(u(14.)))
                    .tooltip(move |w, cx| Tooltip::new(close_tip.clone()).build(w, cx))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DockEvent::Close)))
                    .debug_selector(|| "backlinks-close".into()),
            )
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let th = AppTheme::get(cx).clone();
        let empty = |text: String| {
            div().px(u(16.)).py(u(24.)).text_center().text_color(th.colors.muted_foreground).child(text).into_any_element()
        };
        let mut children: Vec<AnyElement> = Vec::new();
        match (&self.target, &self.index) {
            (None, _) => children.push(empty(crate::t!(BacklinksNoFile))),
            (Some(_), LinkIndex::Loading) => children.push(empty(crate::t!(CommonScanning))),
            (Some(_), LinkIndex::Error(e)) => children.push(empty(e.clone())),
            (Some(t), _) if self.groups.is_empty() => {
                children.push(empty(crate::t!(BacklinksEmpty, name = note_name(t))))
            }
            (Some(_), _) => {
                let view = cx.entity().downgrade();
                for (gi, g) in self.groups.iter().enumerate() {
                    children.push(render_group(&view, gi, g, &th));
                }
            }
        }
        div()
            .id("backlinks-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .py(u(6.))
            .text_size(u(12.))
            .children(children)
            .into_any_element()
    }
}

fn render_group(view: &WeakEntity<BacklinksPane>, gi: usize, g: &BacklinkGroup, th: &AppTheme) -> AnyElement {
    let (hover_bg, hover_fg) = (alpha(th.colors.accent, 0.6), th.colors.accent_foreground);
    let menu = |view: WeakEntity<BacklinksPane>, path: String| {
        move |menu, _: &mut Window, _: &mut Context<PopupMenu>| {
            let (v, p) = (view.clone(), path.clone());
            build_menu(
                menu,
                file_row_menu(
                    &path,
                    move |_, cx| {
                        v.update(cx, |this, cx| this.open(&p, None, cx)).ok();
                    },
                    Vec::new(),
                ),
            )
        }
    };
    let tip: SharedString = g.path.clone().into();
    let (p, v) = (g.path.clone(), view.clone());
    let file_row = div()
        .id(("backlinks-file", gi))
        .flex()
        .items_center()
        .gap(u(6.))
        .w_full()
        .px(u(12.))
        .py(u(4.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(th.colors.foreground)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .child(Icon::new(IconName::File).size(u(13.)).flex_none())
        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(note_name(&g.path)))
        .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
        .on_click(move |_, _, cx| {
            v.update(cx, |this, cx| this.open(&p, None, cx)).ok();
        })
        .debug_selector(|| format!("backlinks-file:{gi}"));
    let file_row = with_context_menu(("backlinks-file-menu", gi), file_row, menu(view.clone(), g.path.clone()));
    let rows = g.refs.iter().enumerate().map(|(ri, r)| {
        let (p, v, line) = (g.path.clone(), view.clone(), r.line);
        let row = div()
            .id(SharedString::from(format!("backlinks-row-{gi}-{ri}")))
            .flex()
            .items_center()
            .gap(u(6.))
            .w_full()
            .pl(u(30.))
            .pr(u(12.))
            .py(u(3.))
            .text_color(th.colors.muted_foreground)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .child(
                div()
                    .flex_none()
                    .min_w(u(12.))
                    .font_family(th.mono_font.clone())
                    .text_size(u(10.))
                    .opacity(0.7)
                    .child(r.line.to_string()),
            )
            .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(r.context.clone()))
            .on_click(move |_, _, cx| {
                v.update(cx, |this, cx| this.open(&p, Some(line), cx)).ok();
            })
            .debug_selector(|| format!("backlinks-row-{gi}-{ri}"));
        with_context_menu(SharedString::from(format!("backlinks-row-menu-{gi}-{ri}")), row, menu(view.clone(), g.path.clone()))
    });
    div().py(u(4.)).child(file_row).children(rows).into_any_element()
}

/// The pane's `basename`: last `/` segment without `.md`/`.markdown`
/// (case-insensitive).
pub fn note_name(path: &str) -> String {
    let base = &path[path.rfind('/').map(|i| i + 1).unwrap_or(0)..];
    let lower = base.to_ascii_lowercase();
    for ext in [".md", ".markdown"] {
        if lower.ends_with(ext) {
            return base[..base.len() - ext.len()].to_string();
        }
    }
    base.to_string()
}

impl EventEmitter<DockEvent> for BacklinksPane {}
impl EventEmitter<ViewEvent> for BacklinksPane {}

impl Render for BacklinksPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let total: usize = self.groups.iter().map(|g| g.refs.len()).sum();
        let th = AppTheme::get(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .text_color(th.colors.foreground)
            .child(self.render_header(total, cx))
            .child(self.render_body(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_name_strips_markdown_extensions() {
        assert_eq!(note_name("/v/sub/Note.md"), "Note");
        assert_eq!(note_name("/v/Note.MARKDOWN"), "Note");
        assert_eq!(note_name("/v/Note.txt"), "Note.txt");
        assert_eq!(note_name("Note"), "Note");
        assert_eq!(note_name("/v/a.md.md"), "a.md");
    }
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use fuchico_core::events::CoreEvent;
    use gpui_kit::TestAppContext;

    use super::{BacklinksPane, LinkIndex};
    use crate::bus::{self, AppEvent};
    use crate::test_ui;
    use crate::workspace::view::{DockEvent, ViewEvent};

    #[gpui_kit::test]
    async fn groups_open_rows_follow_target_and_rescan(cx: &mut TestAppContext) {
        cx.update(test_ui::init);
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("target.md"), "# Target\n").unwrap();
        std::fs::write(root.join("a.md"), "intro\nsee [[target]]\nand [[Target|again]]\n").unwrap();
        std::fs::write(root.join("b.md"), "[link](target.md)\n").unwrap();
        std::fs::write(root.join("c.md"), "no links\n").unwrap();
        let p = |n: &str| root.join(n).to_string_lossy().into_owned();
        let (root_s, target) = (root.to_string_lossy().into_owned(), p("target.md"));
        let t2 = target.clone();
        let (mut ui, pane) =
            test_ui::open(cx, (900., 640.), move |window, cx| BacklinksPane::new(Some(root_s), Some(t2), window, cx));
        let opens: Rc<RefCell<Vec<ViewEvent>>> = Rc::default();
        let closed = Rc::new(RefCell::new(false));
        let (o, c) = (opens.clone(), closed.clone());
        cx.update(|cx| {
            cx.subscribe(&pane, move |_, e: &ViewEvent, _| o.borrow_mut().push(e.clone())).detach();
            cx.subscribe(&pane, move |_, _: &DockEvent, _| *c.borrow_mut() = true).detach();
        });

        let pv = pane.clone();
        ui.wait("link index", move |cx| matches!(pv.read(cx).index, LinkIndex::Loaded { .. })).await;
        let groups = cx.update(|cx| pane.read(cx).groups().to_vec());
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(groups[0].path, p("a.md"));
        assert_eq!(groups[0].refs.iter().map(|r| r.line).collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(groups[1].path, p("b.md"));

        ui.click("backlinks-row-0-1");
        ui.click("backlinks-file:1");
        ui.click("backlinks-close");
        assert_eq!(
            *opens.borrow(),
            vec![
                ViewEvent::OpenFile { path: p("a.md"), line: Some(3) },
                ViewEvent::OpenFile { path: p("b.md"), line: None },
            ]
        );
        assert!(*closed.borrow());

        // Following the active tab: none, then a note nobody links to.
        cx.update(|cx| pane.update(cx, |b, cx| b.set_target(None, cx)));
        assert!(!ui.has("backlinks-file:0"));
        cx.update(|cx| {
            pane.update(cx, |b, cx| b.set_target(Some(p("c.md")), cx));
            assert!(pane.read(cx).groups().is_empty());
            pane.update(cx, |b, cx| b.set_target(Some(target.clone()), cx));
            assert_eq!(pane.read(cx).groups().len(), 2);
        });

        // A new linking note shows up after fs:changed.
        std::fs::write(root.join("d.md"), "[[target]]\n").unwrap();
        let d = root.to_string_lossy().into_owned();
        cx.update(|cx| bus::emit(cx, AppEvent::Core(CoreEvent::FsChanged { dir: d, from: None })));
        let pv = pane.clone();
        ui.wait("rescan", move |cx| pv.read(cx).groups().len() == 3).await;
    }
}
