//! Mermaid diagram dock panel — PLACEHOLDER.
//!
//! Contract: `MermaidPane::new(block_key, label, initial_text, window, cx)`;
//! it listens for `AppEvent::MermaidUpdate { block_key, text }` on the bus
//! (and emits `AppEvent::MermaidRequest` when it has no text), and raises
//! [`DockEvent::Close`]. Rendering will come from `fuchico-mermaid`.

pub mod theme;

use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Subscription, Window, div};

use crate::bus::{self, AppEvent};
use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::dock::dock_frame;
use crate::workspace::view::DockEvent;

pub struct MermaidPane {
    block_key: String,
    label: String,
    text: Option<String>,
    _bus: Subscription,
}

impl MermaidPane {
    pub fn new(
        block_key: String,
        label: String,
        initial_text: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sub = bus::subscribe(cx, |this: &mut MermaidPane, ev, cx| {
            if let AppEvent::MermaidUpdate { block_key, text } = ev
                && *block_key == this.block_key
            {
                this.text = text.clone();
                cx.notify();
            }
        });
        if initial_text.is_none() {
            bus::send(AppEvent::MermaidRequest);
        }
        MermaidPane { block_key, label, text: initial_text, _bus: sub }
    }

    pub fn block_key(&self) -> &str {
        &self.block_key
    }
}

impl EventEmitter<DockEvent> for MermaidPane {}

impl Render for MermaidPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let mono = th.mono_font.clone();
        let close = cx.listener(|_, _, _, cx| cx.emit(DockEvent::Close));
        let body = match &self.text {
            Some(t) => div().font_family(mono).text_size(u(12.)).child(t.clone()),
            None => div().child(crate::t!(CommonLoading)),
        };
        dock_frame("mermaid", self.label.clone(), body, close, cx)
    }
}
