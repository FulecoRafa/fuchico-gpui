//! Tasks & Calendar — PLACEHOLDER main view.
//!
//! Contract: `AgendaView::new(root, window, cx)`, [`MainView::set_root`] on vault
//! change, and [`ViewEvent::OpenFile`] to open a match/item in the editor.

use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window};

use crate::ui::{empty_state, placeholder_view};
use crate::workspace::view::{MainView, ViewEvent};

pub struct AgendaView {
    root: Option<String>,
}

impl AgendaView {
    pub fn new(root: Option<String>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        AgendaView { root }
    }
}

impl EventEmitter<ViewEvent> for AgendaView {}

impl MainView for AgendaView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.root = root;
        cx.notify();
    }
}

impl Render for AgendaView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.root {
            None => empty_state(crate::t!(AgendaOpenFolder), cx),
            Some(_) => placeholder_view(crate::t!(AppTasksCalendar), "Tasks, routines and the calendar (fuchico_core::agenda / tasks / caldav) will live here.", cx),
        }
    }
}
