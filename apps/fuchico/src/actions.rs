//! App-wide GPUI actions. Key bindings live in [`crate::keymap`]; handlers
//! for global actions are registered in [`crate::windows::register_global_actions`]
//! and on the workspace view.

gpui_kit::actions!(
    fuchico,
    [
        // App / windows
        Quit,
        About,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        OpenFolder,
        CloseWindow,
        // Palette / switcher (main window only)
        CommandPalette,
        QuickOpen,
        // Zoom (spec 5 §2.4)
        EditorZoomIn,
        EditorZoomOut,
        EditorZoomReset,
        UiZoomIn,
        UiZoomOut,
        UiZoomReset,
        // Theme
        ToggleTheme,
        // Main views
        ShowFiles,
        ShowAgenda,
        ShowSearch,
        ShowTags,
        ShowSettings,
        ShowShortcuts,
        // Tabs / dock
        CloseTab,
        CloseOtherTabs,
        CloseAllTabs,
        NextTab,
        PreviousTab,
        ToggleDock,
        // Editor
        Save,
        // Edit menu (forwarded to the focused editor / input)
        MenuUndo,
        MenuRedo,
        MenuCut,
        MenuCopy,
        MenuPaste,
        MenuSelectAll,
    ]
);

// Editor-targeted commands run from the command palette (spec 4 §4.3 rows
// 5, 9 and the `:<n>` row). The palette focuses the active pane and then
// dispatches these with `window.dispatch_action`, so the editor pane handles
// them with `.on_action(...)` on its focused element (a pane that does not
// handle one, e.g. a drawing, makes it a no-op, as in the original).
// `Editor*` map 1:1 to `SHORTCUT_ACTIONS` ids, see [`editor_action_for`];
// saving reuses [`Save`].
// Defined by the editor crate (same `fuchico::` names); re-exported so the
// palette and the editor view share one action type.
pub use fuchico_editor::{
    EditorInsertDate, EditorInsertDateTime, EditorInsertRegion, EditorInsertTable,
    EditorOpenOutline, EditorPickDueDate, EditorPickRecurrence, EditorToggleCheckbox,
    EditorToggleTaskLine, GoToLine,
};

/// The editor action for a `SHORTCUT_ACTIONS` id (`"insertDate"`, ...).
pub fn editor_action_for(id: &str) -> Option<Box<dyn gpui_kit::Action>> {
    Some(match id {
        "openOutline" => Box::new(EditorOpenOutline),
        "toggleCheckboxAtCursor" => Box::new(EditorToggleCheckbox),
        "insertDate" => Box::new(EditorInsertDate),
        "insertDateTime" => Box::new(EditorInsertDateTime),
        "insertRegion" => Box::new(EditorInsertRegion),
        "insertTable" => Box::new(EditorInsertTable),
        "toggleTaskLine" => Box::new(EditorToggleTaskLine),
        "pickDueDate" => Box::new(EditorPickDueDate),
        "pickRecurrence" => Box::new(EditorPickRecurrence),
        _ => return None,
    })
}
