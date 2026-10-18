//! The macOS application menu (other platforms get the same actions via
//! key bindings only). It extends the original's default Tauri menu
//! (App / File / Edit / View / Window) with the port's own entries.

use gpui_kit::{App, Menu, MenuItem, SystemMenuType};

use crate::actions::*;

pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("Fuchico").items([
            MenuItem::action("About Fuchico", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", ShowSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Fuchico", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Fuchico", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Open Folder…", OpenFolder),
            MenuItem::action("Save", Save),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        super::edit_menu::menu(),
        Menu::new("View").items([
            MenuItem::action("Command Palette", CommandPalette),
            MenuItem::action("Go to File", QuickOpen),
            MenuItem::separator(),
            MenuItem::action("Files", ShowFiles),
            MenuItem::action("Tasks & Calendar", ShowAgenda),
            MenuItem::action("Search", ShowSearch),
            MenuItem::action("Tags", ShowTags),
            MenuItem::action("Backlinks", ToggleDock),
            MenuItem::separator(),
            MenuItem::action("Zoom In", EditorZoomIn),
            MenuItem::action("Zoom Out", EditorZoomOut),
            MenuItem::action("Actual Size", EditorZoomReset),
            MenuItem::action("Toggle Light/Dark", ToggleTheme),
            // AppKit appends "Enter Full Screen" to the View menu itself
            // (the original's Fullscreen item).
        ]),
        // The original's Window menu: Minimize, Maximize, separator, Close Window.
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
    ]
}

pub fn install(cx: &mut App) {
    cx.set_menus(app_menus());
}
