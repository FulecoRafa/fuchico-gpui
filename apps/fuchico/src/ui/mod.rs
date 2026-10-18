//! Shared UI helpers: sizing ([`style`]), context menus, confirm dialogs and
//! toasts. Thin wrappers over gpui-component so call sites stay short and
//! consistently styled.

pub mod context_menu;
pub mod dialog;
pub mod file_row_menu;
pub mod style;
pub mod toast;
pub mod widgets;

pub use context_menu::{MenuEntry, build_menu};
pub use dialog::{ConfirmOptions, confirm};
pub use file_row_menu::file_row_menu;
pub use style::u;
pub use toast::toast_info;
pub use widgets::{Segment, empty_state, icon_button, placeholder_view, segmented};
