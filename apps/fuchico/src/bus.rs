//! In-process event bus replacing Tauri's `emit`/`listen` (PARITY §0.4).
//!
//! * Any thread: [`send`] or the [`sink`] (a `fuchico_core::EventSink` for
//!   the watcher and core fs calls). Events are queued on an unbounded
//!   channel and pumped onto the main thread.
//! * Main thread: [`emit`] delivers synchronously.
//! * Views in every window subscribe with [`subscribe`] / [`subscribe_in`].

use std::sync::{Arc, OnceLock};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui_kit::{App, AppContext, Context, Entity, EventEmitter, Global, Subscription, Window};

use fuchico_core::events::{CoreEvent, EventSink};

/// Everything that travels between windows.
#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    /// `fs:file-written`, `fs:changed`, `caldav:*`, `app:open-files`.
    Core(CoreEvent),
    /// `mermaid:update {blockKey, text|null}` — editor → diagram dock.
    MermaidUpdate { block_key: String, text: Option<String> },
    /// `mermaid:request {blockKey}` — a diagram view asks the editor to re-send.
    MermaidRequest { block_key: String },
    /// `mermaid:closed {blockKey}` — the diagram dock/window went away.
    MermaidClosed { block_key: String },
    /// `mermaid:dock-request {blockKey, label}` — detached diagram → dock.
    MermaidDockRequest { block_key: String, label: String },
    /// `export:request {key}`.
    ExportRequest { key: String },
    /// `export:html {key, html}`.
    ExportHtml { key: String, html: String },
}

impl AppEvent {
    /// The original event name (for logs).
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::Core(e) => e.name(),
            AppEvent::MermaidUpdate { .. } => "mermaid:update",
            AppEvent::MermaidRequest { .. } => "mermaid:request",
            AppEvent::MermaidClosed { .. } => "mermaid:closed",
            AppEvent::MermaidDockRequest { .. } => "mermaid:dock-request",
            AppEvent::ExportRequest { .. } => "export:request",
            AppEvent::ExportHtml { .. } => "export:html",
        }
    }
}

/// The entity every subscriber listens to.
pub struct EventBus;

impl EventEmitter<AppEvent> for EventBus {}

struct BusGlobal(Entity<EventBus>);

impl Global for BusGlobal {}

static TX: OnceLock<UnboundedSender<AppEvent>> = OnceLock::new();

/// Creates the bus and starts the main-thread pump. Call once at startup.
pub fn init(cx: &mut App) {
    let bus = cx.new(|_| EventBus);
    cx.set_global(BusGlobal(bus));
    let (tx, mut rx) = unbounded::<AppEvent>();
    if TX.set(tx).is_err() {
        log::warn!("bus::init called twice");
        return;
    }
    cx.spawn(async move |cx| {
        while let Some(ev) = rx.next().await {
            cx.update(|cx| emit(cx, ev));
        }
    })
    .detach();
}

/// Delivers `ev` to every subscriber now (main thread).
pub fn emit(cx: &mut App, ev: AppEvent) {
    log::trace!("bus: {}", ev.name());
    let bus = cx.global::<BusGlobal>().0.clone();
    bus.update(cx, |_, cx| cx.emit(ev));
}

/// Queues `ev` from any thread.
pub fn send(ev: AppEvent) {
    match TX.get() {
        Some(tx) => {
            let _ = tx.unbounded_send(ev);
        }
        None => log::warn!("bus: dropped {} before init", ev.name()),
    }
}

/// A `fuchico_core` sink feeding the bus (watcher, fs writes, CalDAV).
pub fn sink() -> Arc<dyn EventSink> {
    Arc::new(|ev: CoreEvent| send(AppEvent::Core(ev)))
}

/// Subscribes a view; the subscription lives as long as it is held.
pub fn subscribe<V: 'static>(
    cx: &mut Context<V>,
    mut f: impl FnMut(&mut V, &AppEvent, &mut Context<V>) + 'static,
) -> Subscription {
    let bus = cx.global::<BusGlobal>().0.clone();
    cx.subscribe(&bus, move |this, _, ev, cx| f(this, ev, cx))
}

/// Like [`subscribe`] but with the view's window.
pub fn subscribe_in<V: 'static>(
    window: &Window,
    cx: &mut Context<V>,
    mut f: impl FnMut(&mut V, &AppEvent, &mut Window, &mut Context<V>) + 'static,
) -> Subscription {
    let bus = cx.global::<BusGlobal>().0.clone();
    cx.subscribe_in(&bus, window, move |this, _, ev, window, cx| f(this, ev, window, cx))
}
