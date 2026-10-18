//! The editor side of the diagram protocol (`mermaidPreviewExtension`):
//! tracks the mermaid blocks whose preview is open and keeps their views
//! in sync over the bus.
//!
//! * [`open_block`] — "▶ Preview diagram" was clicked: start tracking
//!   `block_key` with its current text (the caller then raises
//!   `PaneEvent::OpenMermaid`, which docks the pane).
//! * [`update_block`] — the block's text after an edit (`None` once the
//!   block is gone). After a 400 ms quiet period (`UPDATE_DEBOUNCE_MS`) a
//!   changed text goes out as `mermaid:update {blockKey, text|null}`.
//! * [`close_block`] — stop tracking (the editor went away).
//!
//! `mermaid:request {blockKey}` force-emits the current text and
//! `mermaid:closed {blockKey}` stops tracking, as in the original.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{App, AppContext, Context, Entity, Global, Subscription, Task};

use crate::bus::{self, AppEvent};

pub const UPDATE_DEBOUNCE_MS: u64 = 400;

#[derive(Default)]
struct Tracked {
    /// Text last emitted (or handed over on open).
    sent: Option<String>,
    /// Text last reported by the editor.
    latest: Option<String>,
    debounce: Option<Task<()>>,
}

pub struct MermaidSources {
    blocks: HashMap<String, Tracked>,
    _bus: Subscription,
}

struct SourcesGlobal(Entity<MermaidSources>);
impl Global for SourcesGlobal {}

fn sources(cx: &mut App) -> Entity<MermaidSources> {
    if let Some(g) = cx.try_global::<SourcesGlobal>() {
        return g.0.clone();
    }
    let e = cx.new(|cx| MermaidSources { blocks: HashMap::new(), _bus: bus::subscribe(cx, MermaidSources::on_bus) });
    cx.set_global(SourcesGlobal(e.clone()));
    e
}

impl MermaidSources {
    fn on_bus(&mut self, ev: &AppEvent, cx: &mut Context<Self>) {
        match ev {
            AppEvent::MermaidRequest { block_key } => {
                if let Some(b) = self.blocks.get_mut(block_key) {
                    b.debounce = None;
                    b.sent = b.latest.clone();
                    let ev = AppEvent::MermaidUpdate { block_key: block_key.clone(), text: b.latest.clone() };
                    bus::emit(cx, ev);
                }
            }
            AppEvent::MermaidClosed { block_key } => {
                self.blocks.remove(block_key);
            }
            _ => {}
        }
    }

    fn update(&mut self, key: &str, text: Option<String>, cx: &mut Context<Self>) {
        let Some(b) = self.blocks.get_mut(key) else { return };
        b.latest = text;
        let key = key.to_string();
        b.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(UPDATE_DEBOUNCE_MS)).await;
            let _ = this.update(cx, |this, cx| this.flush(&key, cx));
        }));
    }

    fn flush(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(b) = self.blocks.get_mut(key) else { return };
        b.debounce = None;
        if b.latest != b.sent {
            b.sent = b.latest.clone();
            bus::emit(cx, AppEvent::MermaidUpdate { block_key: key.to_string(), text: b.latest.clone() });
        }
    }
}

/// A fresh `blockKey` (`crypto.randomUUID()`).
pub fn new_block_key() -> String {
    let b: [u8; 16] = rand::random();
    let h = |r: std::ops::Range<usize>| b[r].iter().map(|x| format!("{x:02x}")).collect::<String>();
    format!("{}-{}-4{}-{:x}{}-{}", h(0..4), h(4..6), &h(6..8)[1..], 8 | (b[8] & 3), &h(8..10)[1..], h(10..16))
}

/// Starts tracking `block_key` with its current `text`.
pub fn open_block(block_key: &str, text: &str, cx: &mut App) {
    let s = sources(cx);
    s.update(cx, |s, _| {
        let t = Some(text.to_string());
        s.blocks.insert(block_key.to_string(), Tracked { sent: t.clone(), latest: t, debounce: None });
    });
}

/// The block's text after an edit; `None` when it no longer exists.
/// Debounced; untracked keys are ignored.
pub fn update_block(block_key: &str, text: Option<String>, cx: &mut App) {
    let s = sources(cx);
    s.update(cx, |s, cx| s.update(block_key, text, cx));
}

/// Stops tracking `block_key` (e.g. its editor was closed).
pub fn close_block(block_key: &str, cx: &mut App) {
    let s = sources(cx);
    s.update(cx, |s, _| {
        s.blocks.remove(block_key);
    });
}

pub fn is_tracked(block_key: &str, cx: &mut App) -> bool {
    sources(cx).read(cx).blocks.contains_key(block_key)
}

/// Every tracked key, so an editor can re-extract them after a change.
pub fn tracked_keys(cx: &mut App) -> Vec<String> {
    sources(cx).read(cx).blocks.keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_keys_look_like_uuids() {
        let k = new_block_key();
        assert_eq!(k.len(), 36);
        let parts: Vec<_> = k.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert_eq!(&k[14..15], "4");
        assert!(matches!(&k[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(k, new_block_key());
    }
}
