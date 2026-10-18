//! Single-file editor window (`EditorWindowApp`): one pane filling the
//! window, no activity bar / explorer / tabs / dock / palette. Following a
//! wikilink replaces the pane in place; the title is `"● " + basename` while
//! dirty; the pane's close action closes the window.

use gpui_kit::prelude::*;
use gpui_kit::{Context, FocusHandle, Focusable, Subscription, Window, div};

use crate::actions::Save;
use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::pane::{PaneContext, PaneEvent, PaneHandle, open_pane};
use crate::workspace::tabs::basename;

pub struct EditorWindow {
    /// Label of the path the window was opened for (kept on navigation).
    label: String,
    root: Option<String>,
    path: String,
    dirty: bool,
    pane: PaneHandle,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

/// `"● " + basename` when dirty.
pub fn window_title(path: &str, dirty: bool) -> String {
    let name = basename(path);
    if dirty { format!("● {name}") } else { name.to_string() }
}

impl EditorWindow {
    pub fn new(path: String, root: Option<String>, label: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pane = Self::mount(&path, &root, window, cx);
        pane.focus(window, cx);
        let subs = vec![
            crate::windows::geometry::track(label.clone(), window, cx),
            cx.observe_window_appearance(window, |_, _, cx| crate::theme::apply::refresh(cx)),
        ];
        let this =
            EditorWindow { label, root, path, dirty: false, pane, focus: cx.focus_handle(), _subs: subs };
        this.update_title(window);
        this
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    fn mount(path: &str, root: &Option<String>, window: &mut Window, cx: &mut Context<Self>) -> PaneHandle {
        open_pane(path, PaneContext { root: root.clone(), in_window: true }, window, cx, |this: &mut EditorWindow, ev, window, cx| {
            this.on_pane_event(ev, window, cx)
        })
    }

    fn update_title(&self, window: &mut Window) {
        window.set_window_title(&window_title(&self.path, self.dirty));
    }

    fn on_pane_event(&mut self, ev: &PaneEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            PaneEvent::DirtyChanged(d) => {
                self.dirty = *d;
                self.update_title(window);
            }
            PaneEvent::OpenFile { path, line } => {
                self.path = path.clone();
                self.dirty = false;
                self.pane = Self::mount(path, &self.root, window, cx);
                if let Some(line) = line {
                    self.pane.reveal_line(*line, window, cx);
                }
                self.pane.focus(window, cx);
                self.update_title(window);
                cx.notify();
            }
            PaneEvent::Close => window.remove_window(),
            // Main-window-only features.
            PaneEvent::OpenTag(_)
            | PaneEvent::OpenMermaid { .. }
            | PaneEvent::OpenCommandPalette(_)
            | PaneEvent::QuickOpen
            | PaneEvent::CycleTab(_)
            | PaneEvent::GlobalSearch(_) => {
                log::debug!("editor window ignores {ev:?}");
            }
        }
    }
}

impl Focusable for EditorWindow {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        div()
            .id("editor-window")
            .key_context("EditorWindow")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Save, w, cx| this.pane.save(w, cx)))
            .size_full()
            .flex()
            .flex_col()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            .font_family(th.ui_font.clone())
            .text_size(u(13.))
            .child(self.pane.view())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_marks_dirty() {
        assert_eq!(super::window_title("/v/a.md", false), "a.md");
        assert_eq!(super::window_title("/v/a.md", true), "● a.md");
    }
}
