//! Backlinks dock panel — PLACEHOLDER.
//!
//! Contract: `BacklinksPane::new(target, window, cx)`,
//! [`BacklinksPane::set_target`] whenever the active tab changes (`None` for
//! no tab or a `.excalidraw` path, case-sensitive), and [`DockEvent::Close`]
//! from its close button.

use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window};

use crate::workspace::dock::dock_frame;
use crate::workspace::tabs::basename;
use crate::workspace::view::{DockEvent, ViewEvent};

pub struct BacklinksPane {
    target: Option<String>,
}

impl BacklinksPane {
    pub fn new(target: Option<String>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        BacklinksPane { target }
    }

    pub fn set_target(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        if self.target != target {
            self.target = target;
            cx.notify();
        }
    }
}

impl EventEmitter<DockEvent> for BacklinksPane {}
impl EventEmitter<ViewEvent> for BacklinksPane {}

impl Render for BacklinksPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.target {
            None => crate::t!(BacklinksNoFile),
            Some(t) => crate::t!(BacklinksEmpty, name = basename(t).trim_end_matches(".md")),
        };
        let close = cx.listener(|_, _, _, cx| cx.emit(DockEvent::Close));
        dock_frame("backlinks", crate::t!(BacklinksTitle), body, close, cx)
    }
}
