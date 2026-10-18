//! Port of `src/lib/fileRowMenu.ts` + `fileActions.ts`: the shared
//! right-click items for any list row pointing at a vault file (search
//! results, tag file lists, backlinks, agenda items).

use gpui_kit::component::IconName;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClipboardItem, Context, ElementId, Window, div};

use crate::ui::MenuEntry;

/// "Open", `extra…`, separator, "Reveal in file manager", "Copy path".
pub fn file_row_menu(
    path: &str,
    on_open: impl Fn(&mut Window, &mut App) + 'static,
    extra: Vec<MenuEntry>,
) -> Vec<MenuEntry> {
    let mut items = vec![MenuEntry::item(crate::t!(CommonOpen), on_open).icon(IconName::FileText)];
    items.extend(extra);
    items.push(MenuEntry::Separator);
    let p = path.to_string();
    items.push(MenuEntry::item(crate::t!(CommonRevealInFileManager), move |_, _| reveal_in_file_manager(&p)).icon(IconName::Eye));
    let p = path.to_string();
    items.push(
        MenuEntry::item(crate::t!(CommonCopyPath), move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(p.clone())))
            .icon(IconName::Copy),
    );
    items
}

/// A full-width box owning `row`'s right-click menu. The row itself (the
/// click target) stays a plain stateful element so UI tests can observe it
/// with `.test_support()`, which cannot wrap a context-menu element.
pub fn with_context_menu(
    id: impl Into<ElementId>,
    row: impl IntoElement,
    menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> AnyElement {
    div().id(id).w_full().child(row).context_menu(menu).into_any_element()
}

/// `revealInFileManager(path)` — selects the file in Finder / Explorer, or
/// opens its folder elsewhere. Spawned, never blocks the UI thread.
pub fn reveal_in_file_manager(path: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open")
        .arg(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("/")))
        .spawn();
    if let Err(e) = r {
        log::warn!("reveal failed: {e}");
    }
}
