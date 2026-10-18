//! Editor actions. The `Editor*` / [`GoToLine`] names match the app's
//! `apps/fuchico/src/actions.rs` (namespace `fuchico`) so the host can
//! re-export these instead of defining its own.

gpui_kit::actions!(
    fuchico,
    [
        EditorOpenOutline,
        EditorToggleCheckbox,
        EditorInsertDate,
        EditorInsertDateTime,
        EditorInsertRegion,
        EditorInsertTable,
        EditorToggleTaskLine,
        EditorPickDueDate,
        EditorPickRecurrence,
    ]
);

/// Jump to a 1-based line (collapsed cursor at its start, centred).
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = fuchico, no_json)]
pub struct GoToLine {
    pub line: u32,
}

// Editor-local actions (namespace `fuchico_editor`).
gpui_kit::actions!(
    fuchico_editor,
    [
        /// Mod-f: the find bar.
        OpenFind,
        /// Mod-s.
        SaveBuffer,
        /// Mod-= / Mod-- / Mod-0 (forwarded to the host as `EditorEvent::Zoom`).
        ZoomIn,
        ZoomOut,
        ZoomReset,
        /// Cmd-Alt-[ / Cmd-Alt-]: fold / unfold at the cursor.
        FoldAtCursor,
        UnfoldAtCursor,
        /// Ctrl-Alt-[ / Ctrl-Alt-]: fold / unfold everything.
        FoldAll,
        UnfoldAll,
        /// Mod-c / Mod-x / Mod-v (system clipboard, like the browser).
        Copy,
        Cut,
        Paste,
        /// Tab / Shift-Tab inside the editor. Bound in the editor context so
        /// they win over the root's focus traversal and reach the engine
        /// (indent, completion cycling, table cells, prompt completion).
        EditorTab,
        EditorBacktab,
        /// Mod-z / Mod-shift-z / Mod-a (CodeMirror `historyKeymap` +
        /// `selectAll`).
        Undo,
        Redo,
        SelectAll,
        /// F3 / Mod-g (`findNext`) and Shift-F3 / Mod-Shift-g
        /// (`findPrevious`); they also work with the panel closed.
        FindNext,
        FindPrevious,
        /// Mod-Shift-l (`selectSelectionMatches`).
        SelectSelectionMatches,
        /// Mod-d (`selectNextOccurrence`).
        SelectNextOccurrence,
        /// Mod-Alt-g (`gotoLine`): the "Go to line" dialog.
        OpenGotoLine,
        /// Mod-u / Mod-Shift-u (Alt-u off macOS): `undoSelection` /
        /// `redoSelection`.
        UndoSelection,
        RedoSelection,
    ]
);
