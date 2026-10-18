//! Command palette and quick switcher — PLACEHOLDER.
//!
//! Contract: [`open`] is called by the workspace for `CommandPalette`
//! (Mod-Shift-P, empty or preset query) and `QuickOpen` (Mod-P), main window
//! only. The real implementation will be a gpui-component dialog fed by
//! `fuchico_core::fuzzy` and the command list built from the workspace
//! context.

use gpui_kit::{App, Window};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    Commands { query: String },
    QuickOpen,
}

pub fn open(kind: PaletteKind, window: &mut Window, cx: &mut App) {
    let title = match &kind {
        PaletteKind::Commands { .. } => crate::t!(PaletteRunCommand),
        PaletteKind::QuickOpen => crate::t!(PaletteGoToFile),
    };
    log::info!("palette {kind:?} not implemented yet");
    crate::ui::toast_info(window, cx, title);
}
