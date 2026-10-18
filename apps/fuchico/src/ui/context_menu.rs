//! Declarative context-menu entries rendered into a gpui-component
//! [`PopupMenu`]. Attach with `.context_menu(move |m, _, _| build_menu(m, entries()))`
//! on an element with an id (`ContextMenuExt`).

use std::rc::Rc;

use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{App, SharedString, Window};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
#[allow(clippy::large_enum_variant)] // a handful per menu, built per right-click
pub enum MenuEntry {
    Item { label: SharedString, icon: Option<Icon>, disabled: bool, on_click: Handler },
    Separator,
}

impl MenuEntry {
    pub fn item(label: impl Into<SharedString>, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        MenuEntry::Item { label: label.into(), icon: None, disabled: false, on_click: Rc::new(on_click) }
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        if let MenuEntry::Item { icon: i, .. } = &mut self {
            *i = Some(icon.into());
        }
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        if let MenuEntry::Item { disabled: d, .. } = &mut self {
            *d = disabled;
        }
        self
    }
}

/// Appends `entries` to `menu` (min width 180px like the original menus).
pub fn build_menu(menu: PopupMenu, entries: Vec<MenuEntry>) -> PopupMenu {
    entries.into_iter().fold(menu.min_w(gpui_kit::px(180.)), |menu, e| match e {
        MenuEntry::Separator => menu.separator(),
        MenuEntry::Item { label, icon, disabled, on_click } => {
            let mut item = PopupMenuItem::new(label).disabled(disabled).on_click(move |_, w, cx| on_click(w, cx));
            if let Some(icon) = icon {
                item = item.icon(icon);
            }
            menu.item(item)
        }
    })
}
