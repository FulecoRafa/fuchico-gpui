//! Pane extension point: what a tab (or an editor window) hosts.
//!
//! A pane is any GPUI view implementing [`PaneItem`]. The shell only talks
//! to it through the type-erased [`PaneHandle`] and listens to its
//! [`PaneEvent`]s. [`open_pane`] picks the implementation for a path
//! (`.excalidraw`, case-insensitive, → Excalidraw; everything else → the
//! markdown editor), so the real editor/canvas crates plug in by replacing
//! the constructors in `crate::editor` / `crate::excalidraw`.

use gpui_kit::{
    AnyView, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Subscription,
    Window,
};

/// Events a pane raises toward its host (workspace tab or editor window).
#[derive(Clone, Debug, PartialEq)]
pub enum PaneEvent {
    /// `onDirtyChange(dirty)`.
    DirtyChanged(bool),
    /// Navigate (wikilink, link click): open `path`, optionally at `line`.
    OpenFile { path: String, line: Option<u32> },
    /// Tag click → tags view with that tag selected.
    OpenTag(String),
    /// `onOpenMermaid({blockKey, text})` → mermaid dock.
    OpenMermaid { block_key: String, initial_text: String },
    /// `onOpenCommandPalette(query)`.
    OpenCommandPalette(String),
    /// `onClose` (e.g. Helix `:q`): close the tab / window.
    Close,
}

/// Construction context shared by every pane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaneContext {
    /// Vault root (`None` in an editor window opened outside a vault).
    pub root: Option<String>,
}

/// A view that can live in a tab.
pub trait PaneItem: Render + Focusable + EventEmitter<PaneEvent> + Sized + 'static {
    fn path(&self) -> &str;
    /// The file was renamed/moved on disk; keep editing it under `path`.
    fn set_path(&mut self, path: String, cx: &mut Context<Self>);
    /// `focusLine`/`focusToken`: reveal 1-based `line` (re-applied on each
    /// new token even if the line is unchanged).
    fn reveal_line(&mut self, _line: u32, _window: &mut Window, _cx: &mut Context<Self>) {}
    /// `editorRef.save()`.
    fn save(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn is_dirty(&self) -> bool {
        false
    }
}

trait ErasedPane {
    fn view(&self) -> AnyView;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn path(&self, cx: &App) -> String;
    fn set_path(&self, path: String, cx: &mut App);
    fn reveal_line(&self, line: u32, window: &mut Window, cx: &mut App);
    fn save(&self, window: &mut Window, cx: &mut App);
    fn is_dirty(&self, cx: &App) -> bool;
}

impl<T: PaneItem> ErasedPane for Entity<T> {
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }
    fn path(&self, cx: &App) -> String {
        self.read(cx).path().to_string()
    }
    fn set_path(&self, path: String, cx: &mut App) {
        self.update(cx, |p, cx| p.set_path(path, cx));
    }
    fn reveal_line(&self, line: u32, window: &mut Window, cx: &mut App) {
        self.update(cx, |p, cx| p.reveal_line(line, window, cx));
    }
    fn save(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |p, cx| p.save(window, cx));
    }
    fn is_dirty(&self, cx: &App) -> bool {
        self.read(cx).is_dirty()
    }
}

/// A live pane plus the host's subscription to its events.
pub struct PaneHandle {
    inner: Box<dyn ErasedPane>,
    _events: Subscription,
}

impl PaneHandle {
    /// Wraps an existing pane entity; `on_event` runs on the host view.
    pub fn new<T: PaneItem, V: 'static>(
        pane: Entity<T>,
        window: &mut Window,
        cx: &mut Context<V>,
        mut on_event: impl FnMut(&mut V, &PaneEvent, &mut Window, &mut Context<V>) + 'static,
    ) -> PaneHandle {
        let sub = cx.subscribe_in(&pane, window, move |host, _, ev, window, cx| on_event(host, ev, window, cx));
        PaneHandle { inner: Box::new(pane), _events: sub }
    }

    pub fn view(&self) -> AnyView {
        self.inner.view()
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.inner.focus_handle(cx), cx);
    }
    pub fn path(&self, cx: &App) -> String {
        self.inner.path(cx)
    }
    pub fn set_path(&self, path: String, cx: &mut App) {
        self.inner.set_path(path, cx);
    }
    pub fn reveal_line(&self, line: u32, window: &mut Window, cx: &mut App) {
        self.inner.reveal_line(line, window, cx);
    }
    pub fn save(&self, window: &mut Window, cx: &mut App) {
        self.inner.save(window, cx);
    }
    pub fn is_dirty(&self, cx: &App) -> bool {
        self.inner.is_dirty(cx)
    }
}

/// `activeTab.path.toLowerCase().endsWith(".excalidraw")` (pane switch).
pub fn is_excalidraw_path(path: &str) -> bool {
    path.to_lowercase().ends_with(".excalidraw")
}

/// The pane factory: Excalidraw for drawings, the editor for everything else.
pub fn open_pane<V: 'static>(
    path: &str,
    pcx: PaneContext,
    window: &mut Window,
    cx: &mut Context<V>,
    on_event: impl FnMut(&mut V, &PaneEvent, &mut Window, &mut Context<V>) + 'static,
) -> PaneHandle {
    let path = path.to_string();
    if is_excalidraw_path(&path) {
        let pane = cx.new(|cx| crate::excalidraw::ExcalidrawPane::new(path, pcx, window, cx));
        PaneHandle::new(pane, window, cx, on_event)
    } else {
        let pane = cx.new(|cx| crate::editor::EditorPane::new(path, pcx, window, cx));
        PaneHandle::new(pane, window, cx, on_event)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn excalidraw_switch_is_case_insensitive() {
        assert!(super::is_excalidraw_path("/v/a.Excalidraw"));
        assert!(!super::is_excalidraw_path("/v/a.md"));
    }
}
