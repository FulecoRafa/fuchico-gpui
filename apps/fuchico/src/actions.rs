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
    ]
);
