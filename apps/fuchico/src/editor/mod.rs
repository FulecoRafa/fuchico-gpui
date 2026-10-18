//! Markdown editor pane — PLACEHOLDER.
//!
//! [`EditorPane`] currently shows the file read-only in the editor font with
//! the status bar. The real implementation (fuchico-helix modal engine +
//! fuchico-markdown live preview) replaces the body and implements
//! [`PaneItem::save`] / [`PaneItem::reveal_line`] / dirty tracking; the
//! constructor signature is the contract used by `workspace::pane::open_pane`.

use fuchico_core::doc_stats::{DocStats, compute_doc_stats};
use fuchico_core::fs::file::{ReadResult, fs_read_file};
use gpui_kit::{
    AppContext, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement,
    Render, ScrollHandle, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::pane::{PaneContext, PaneEvent, PaneItem};
use crate::workspace::status_bar::StatusBar;

/// Characters shown by the preview (keeps layout cheap on huge files).
const PREVIEW_LIMIT: usize = 200_000;

enum Content {
    Loading,
    Text(String),
    Message(String),
}

pub struct EditorPane {
    path: String,
    #[allow(dead_code)]
    pcx: PaneContext,
    content: Content,
    stats: DocStats,
    focus: FocusHandle,
    scroll: ScrollHandle,
}

impl EditorPane {
    pub fn new(path: String, pcx: PaneContext, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut pane = EditorPane {
            path,
            pcx,
            content: Content::Loading,
            stats: DocStats { words: 0, reading_time_min: 0 },
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        };
        pane.load(cx);
        pane
    }

    /// Reads the file off the UI thread.
    fn load(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let read = cx.background_spawn(async move {
            let res = fs_read_file(path);
            let stats = match &res {
                Ok(ReadResult::Text { content, .. }) => compute_doc_stats(content),
                _ => DocStats { words: 0, reading_time_min: 0 },
            };
            (res, stats)
        });
        cx.spawn(async move |this, cx| {
            let (res, stats) = read.await;
            let content = match res {
                Ok(ReadResult::Text { mut content, .. }) => {
                    if content.len() > PREVIEW_LIMIT {
                        let cut = (0..=PREVIEW_LIMIT).rev().find(|i| content.is_char_boundary(*i)).unwrap_or(0);
                        content.truncate(cut);
                        content.push_str("\n…");
                    }
                    Content::Text(content)
                }
                Ok(ReadResult::Binary { size }) => Content::Message(format!("Binary file ({size} bytes)")),
                Ok(ReadResult::TooLarge { size, limit }) => {
                    Content::Message(format!("File too large ({size} bytes, limit {limit})"))
                }
                Err(e) => Content::Message(e),
            };
            this.update(cx, |this, cx| {
                this.content = content;
                this.stats = stats;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl EventEmitter<PaneEvent> for EditorPane {}

impl Focusable for EditorPane {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for EditorPane {
    fn path(&self) -> &str {
        &self.path
    }

    fn set_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.path = path;
        cx.notify();
    }
}

impl Render for EditorPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let body = match &self.content {
            Content::Loading => div().text_color(th.colors.muted_foreground).child(crate::t!(CommonLoading)),
            Content::Text(t) => div()
                .font_family(th.mono_font.clone())
                .text_size(px(th.editor_font_size))
                .line_height(px(th.editor_font_size * 1.6))
                .whitespace_normal()
                .child(t.clone()),
            Content::Message(m) => div().text_color(th.colors.muted_foreground).child(m.clone()),
        };
        div()
            .track_focus(&self.focus)
            .key_context("EditorPane")
            .flex()
            .flex_col()
            .size_full()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            .child(
                div()
                    .id("editor-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px(u(24.))
                    .py(u(16.))
                    .child(body),
            )
            .child(StatusBar { stats: self.stats })
    }
}
