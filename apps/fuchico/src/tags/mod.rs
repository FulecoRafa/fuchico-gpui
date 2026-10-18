//! Tags view — PLACEHOLDER main view.
//!
//! Contract: `TagsView::new(root, window, cx)`, [`MainView::set_root`],
//! [`TagsView::select_tag`] (a tag click elsewhere: `selectedTag` +
//! `selectedTagToken`), and [`ViewEvent::OpenFile`] for a file in the list.

use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window};

use crate::ui::{empty_state, placeholder_view};
use crate::workspace::view::{MainView, ViewEvent};

pub struct TagsView {
    root: Option<String>,
    selected: Option<String>,
    /// Bumped on every selection so re-selecting the same tag re-triggers.
    token: u32,
}

impl TagsView {
    pub fn new(root: Option<String>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        TagsView { root, selected: None, token: 0 }
    }

    pub fn select_tag(&mut self, tag: String, token: u32, cx: &mut Context<Self>) {
        self.selected = Some(tag);
        self.token = token;
        cx.notify();
    }
}

impl EventEmitter<ViewEvent> for TagsView {}

impl MainView for TagsView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.root = root;
        cx.notify();
    }
}

impl Render for TagsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match (&self.root, &self.selected) {
            (None, _) => empty_state(crate::t!(TagsOpenFolder), cx),
            (Some(_), Some(tag)) => placeholder_view(format!("#{tag}"), crate::t!(TagsSelectTag), cx),
            (Some(_), None) => placeholder_view(
                crate::t!(AppTags),
                "The tag index (fuchico_core::tags) and per-tag file lists will live here.",
                cx,
            ),
        }
    }
}
