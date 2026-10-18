//! Excalidraw pane — PLACEHOLDER.
//!
//! Shows the drawing's element count until the `fuchico-excalidraw` canvas
//! crate is wired in. Keep the constructor signature: it is what
//! `workspace::pane::open_pane` calls for `.excalidraw` paths.

use fuchico_core::fs::file::{ReadResult, fs_read_file};
use gpui_kit::{
    AppContext, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Window, div,
};

use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::pane::{PaneContext, PaneEvent, PaneItem};
use crate::workspace::tabs::basename;

pub struct ExcalidrawPane {
    path: String,
    #[allow(dead_code)]
    pcx: PaneContext,
    summary: Option<String>,
    focus: FocusHandle,
}

impl ExcalidrawPane {
    pub fn new(path: String, pcx: PaneContext, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = path.clone();
        let read = cx.background_spawn(async move { summarize(fs_read_file(p)) });
        cx.spawn(async move |this, cx| {
            let summary = read.await;
            this.update(cx, |this, cx| {
                this.summary = Some(summary);
                cx.notify();
            })
            .ok();
        })
        .detach();
        ExcalidrawPane { path, pcx, summary: None, focus: cx.focus_handle() }
    }
}

fn summarize(res: Result<ReadResult, String>) -> String {
    match res {
        Ok(ReadResult::Text { content, .. }) if content.trim().is_empty() => "Empty drawing".into(),
        Ok(ReadResult::Text { content, .. }) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(v) => {
                let n = v
                    .get("elements")
                    .and_then(|e| e.as_array())
                    .map(|a| a.iter().filter(|e| e.get("isDeleted") != Some(&true.into())).count())
                    .unwrap_or(0);
                format!("{n} elements")
            }
            Err(e) => format!("Invalid drawing: {e}"),
        },
        Ok(_) => "Not a drawing".into(),
        Err(e) => e,
    }
}

impl EventEmitter<PaneEvent> for ExcalidrawPane {}

impl Focusable for ExcalidrawPane {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for ExcalidrawPane {
    fn path(&self) -> &str {
        &self.path
    }
    fn set_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.path = path;
        cx.notify();
    }
}

impl Render for ExcalidrawPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(u(6.))
            .bg(th.colors.background)
            .text_color(th.colors.muted_foreground)
            .text_size(u(13.))
            .child(div().text_color(th.colors.foreground).child(basename(&self.path).to_string()))
            .child(self.summary.clone().unwrap_or_else(|| crate::t!(CommonLoading)))
            .child(div().text_size(u(11.)).child("Excalidraw canvas pending (fuchico-excalidraw)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_live_elements() {
        let text = r#"{"elements":[{"id":"a"},{"id":"b","isDeleted":true}]}"#;
        let r = Ok(ReadResult::Text { content: text.into(), size: 10 });
        assert_eq!(summarize(r), "1 elements");
    }
}
