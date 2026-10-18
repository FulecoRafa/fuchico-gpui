//! Mermaid diagrams (parity §11–§12).
//!
//! * [`MermaidPane`] — the right-dock preview (`Chrome::Pane`): "Open in
//!   new window" pops it out, "Close preview" closes the dock.
//! * [`window::MermaidWindow`] — the pop-out window `mermaid-<blockKey>`
//!   (720×560) with a single "Dock" button.
//! * [`sources`] — the editor side: [`open_block`] / [`update_block`] /
//!   [`close_block`] with the 400 ms debounce.
//!
//! Protocol (bus): `mermaid:update {blockKey, text|null}` editor → views;
//! `mermaid:request {blockKey}` a view without text asks for it;
//! `mermaid:closed {blockKey}` the view went away (not sent when popping
//! out or re-docking); `mermaid:dock-request {blockKey, label}` window →
//! main workspace.

pub mod sources;
pub mod theme;
pub mod window;

#[cfg(test)]
mod ui_tests;

#[allow(unused_imports)] // The editor's API (wired by the editor engineer).
pub use sources::{close_block, is_tracked, new_block_key, open_block, tracked_keys, update_block};

use fuchico_mermaid::{Chrome, MermaidEvent, MermaidView};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Window, div};

use crate::bus::{self, AppEvent};
use crate::theme::AppTheme;
use crate::workspace::view::DockEvent;

/// Builds the shared view, themed and localised.
fn new_view(label: &str, chrome: Chrome, text: Option<String>, cx: &mut App) -> Entity<MermaidView> {
    let th = theme::mermaid_theme(AppTheme::get(cx));
    cx.new(|cx| {
        let mut v = match text {
            Some(t) => MermaidView::with_source(label.to_string(), chrome, th, t, cx),
            None => MermaidView::new(label.to_string(), chrome, th, cx),
        };
        v.set_strings(theme::mermaid_strings(), cx);
        v
    })
}

/// Re-themes `view` whenever the app theme (or language) changes.
fn follow_theme<T: 'static>(view: &Entity<MermaidView>, cx: &mut Context<T>) -> Subscription {
    let view = view.clone();
    cx.observe_global::<AppTheme>(move |_, cx| {
        let th = theme::mermaid_theme(AppTheme::get(cx));
        view.update(cx, |v, cx| {
            v.set_theme(th, cx);
            v.set_strings(theme::mermaid_strings(), cx);
        });
    })
}

/// Applies a `mermaid:update` for `key` to `view`, returning its text.
fn apply_update(key: &str, ev: &AppEvent, view: &Entity<MermaidView>, cx: &mut App) -> Option<Option<String>> {
    match ev {
        AppEvent::MermaidUpdate { block_key, text } if block_key == key => {
            view.update(cx, |v, cx| v.set_source(text.clone(), cx));
            Some(text.clone())
        }
        _ => None,
    }
}

pub struct MermaidPane {
    block_key: String,
    label: String,
    view: Entity<MermaidView>,
    /// Last text received, handed to the window on pop-out.
    text: Option<String>,
    /// Popped out: the window now owns the block, so no `mermaid:closed`.
    popped: bool,
    _subs: Vec<Subscription>,
}

impl MermaidPane {
    pub fn new(
        block_key: String,
        label: String,
        initial_text: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = new_view(&label, Chrome::Pane, initial_text.clone(), cx);
        let subs = vec![
            bus::subscribe(cx, |this: &mut MermaidPane, ev, cx| {
                if let Some(text) = apply_update(&this.block_key, ev, &this.view, cx) {
                    this.text = text;
                }
            }),
            cx.subscribe(&view, |this, _, ev: &MermaidEvent, cx| match ev {
                MermaidEvent::Close => cx.emit(DockEvent::Close),
                MermaidEvent::PopOut => this.pop_out(cx),
                MermaidEvent::Dock => {}
            }),
            follow_theme(&view, cx),
            cx.on_release(|this, _| {
                if !this.popped {
                    // Queued, like the original's deferred emit.
                    bus::send(AppEvent::MermaidClosed { block_key: this.block_key.clone() });
                }
            }),
        ];
        if initial_text.is_none() {
            bus::send(AppEvent::MermaidRequest { block_key: block_key.clone() });
        }
        MermaidPane { block_key, label, view, text: initial_text, popped: false, _subs: subs }
    }

    pub fn block_key(&self) -> &str {
        &self.block_key
    }

    pub fn view(&self) -> &Entity<MermaidView> {
        &self.view
    }

    /// "Open in new window": opens `mermaid-<blockKey>` and closes the dock
    /// without sending `mermaid:closed`.
    fn pop_out(&mut self, cx: &mut Context<Self>) {
        self.popped = true;
        let (key, label, text) = (self.block_key.clone(), self.label.clone(), self.text.clone());
        cx.defer(move |cx| window::open_mermaid_window(key, label, text, cx));
        cx.emit(DockEvent::Close);
    }
}

impl EventEmitter<DockEvent> for MermaidPane {}

impl Render for MermaidPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.view.clone())
    }
}
