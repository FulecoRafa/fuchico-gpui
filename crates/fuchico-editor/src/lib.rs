//! Native GPUI Markdown editor for Fuchico.
//!
//! [`EditorView`] glues three pieces together:
//! - [`fuchico_helix::Editor`]: the modal (Helix) engine, which owns the
//!   text, selections, modes, registers and undo history (char offsets);
//! - [`fuchico_markdown::MarkdownModel`]: the live-preview model (parse,
//!   decorations, folds, tables, completions; byte offsets);
//! - a custom gpui element that lays out and paints **only the visible
//!   lines** (gutter, folds, rulers, cursors, widgets) plus div overlays
//!   (command line, search, find bar, completion popup, outline, mode chip).
//!
//! The host (the app's pane) owns the file: it listens to [`EditorEvent`]s
//! (dirty/save/quit/links/tags/mermaid/zoom/...) and drives the view through
//! its public methods. Call [`init`] once to install the key bindings.

pub mod actions;
pub mod commands;
pub mod display;
mod element;
mod host;
mod images;
pub mod style;
mod view;

pub use actions::*;
pub use commands::{CommandEdit, TokenKind};
pub use style::EditorStyle;
pub use view::{
    EditorEvent, EditorOptions, EditorView, EngineKind, InsertCursor, LineNumbers, MermaidBlock, ZoomDirection,
    NoteReader, prompt_grid,
};

/// Key context of the editor (`key_context("FuchicoEditor")`).
pub const KEY_CONTEXT: &str = "FuchicoEditor";

/// Installs the editor key bindings (context [`KEY_CONTEXT`]). `secondary`
/// is Cmd on macOS and Ctrl elsewhere (CodeMirror's `Mod`).
pub fn init(cx: &mut gpui_kit::App) {
    use gpui_kit::KeyBinding;
    let c = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-o", EditorOpenOutline, c),
        KeyBinding::new("secondary-enter", EditorToggleCheckbox, c),
        KeyBinding::new("secondary-shift-d", EditorInsertDate, c),
        KeyBinding::new("secondary-alt-d", EditorInsertDateTime, c),
        KeyBinding::new("secondary-shift-r", EditorInsertRegion, c),
        KeyBinding::new("secondary-alt-t", EditorInsertTable, c),
        KeyBinding::new("secondary-shift-enter", EditorToggleTaskLine, c),
        KeyBinding::new("secondary-shift-.", EditorPickDueDate, c),
        KeyBinding::new("secondary-shift-,", EditorPickRecurrence, c),
        KeyBinding::new("secondary-f", OpenFind, c),
        KeyBinding::new("secondary-s", SaveBuffer, c),
        KeyBinding::new("secondary-=", ZoomIn, c),
        KeyBinding::new("secondary-+", ZoomIn, c),
        KeyBinding::new("secondary--", ZoomOut, c),
        KeyBinding::new("secondary-0", ZoomReset, c),
        KeyBinding::new("secondary-alt-[", FoldAtCursor, c),
        KeyBinding::new("secondary-alt-]", UnfoldAtCursor, c),
        KeyBinding::new("ctrl-alt-[", FoldAll, c),
        KeyBinding::new("ctrl-alt-]", UnfoldAll, c),
        KeyBinding::new("secondary-c", Copy, c),
        KeyBinding::new("secondary-x", Cut, c),
        KeyBinding::new("secondary-v", Paste, c),
        KeyBinding::new("tab", EditorTab, c),
        KeyBinding::new("shift-tab", EditorBacktab, c),
        KeyBinding::new("secondary-z", Undo, c),
        KeyBinding::new("secondary-shift-z", Redo, c),
        KeyBinding::new("secondary-a", SelectAll, c),
    ]);
}
