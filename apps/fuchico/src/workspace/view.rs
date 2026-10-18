//! Main-view and dock extension points.
//!
//! The four non-editor main views (agenda, search, tags, settings) and the
//! dock panels are long-lived entities owned by the workspace. They receive
//! the vault root through [`MainView::set_root`] and talk back with
//! [`ViewEvent`]s / [`DockEvent`]s.

use gpui_kit::{Context, EventEmitter, Render};

/// Requests from a main view or the explorer to the shell.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewEvent {
    /// `openFile(path, focusLine?)` — opens a tab and forces the editor view.
    OpenFile { path: String, line: Option<u32> },
    /// Open `path` in its own editor window.
    OpenInNewWindow(String),
    /// Native folder picker → new vault.
    OpenFolder,
    /// `onPathRenamed(from, to)` — remap tabs and scroll positions.
    PathRenamed { from: String, to: String },
    /// `onPathDeleted(path)` — close affected tabs.
    PathDeleted(String),
}

/// A main view (`agenda` / `search` / `tags` / `settings`).
pub trait MainView: Render + EventEmitter<ViewEvent> + Sized + 'static {
    /// Called on creation and whenever the vault changes (`None` = no vault).
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>);
}

/// Dock panel → shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockEvent {
    /// The panel's own close button: `setDock(null)`.
    Close,
}
