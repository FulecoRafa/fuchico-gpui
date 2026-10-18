//! Search panel — PLACEHOLDER main view.
//!
//! Contract: `SearchView::new(root, window, cx)`, [`MainView::set_root`] on vault
//! change, and [`ViewEvent::OpenFile`] to open a match/item in the editor.

use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window};

use crate::ui::{empty_state, placeholder_view};
use crate::workspace::view::{MainView, ViewEvent};

pub struct SearchView {
    root: Option<String>,
}

impl SearchView {
    pub fn new(root: Option<String>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        SearchView { root }
    }
}

impl EventEmitter<ViewEvent> for SearchView {}

impl MainView for SearchView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.root = root;
        cx.notify();
    }
}

impl Render for SearchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.root {
            None => empty_state(crate::t!(SearchOpenFolderToSearch), cx),
            Some(_) => placeholder_view(crate::t!(AppSearch), "Vault-wide search and replace (fuchico_core::search) will live here.", cx),
        }
    }
}
