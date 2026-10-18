//! HTML / PDF export — PLACEHOLDER.
//!
//! Contract: [`export_html`] renders the note with
//! `fuchico_core::render_html` and asks for a save path; [`export_pdf`]
//! opens the print window. The bus already carries `ExportRequest` /
//! `ExportHtml` for the cross-window handshake.

use gpui_kit::{App, Window};

pub fn export_html(path: &str, window: &mut Window, cx: &mut App) {
    log::info!("export_html({path}) not implemented yet");
    crate::ui::toast_info(window, cx, "Export is not implemented yet");
}

pub fn export_pdf(path: &str, window: &mut Window, cx: &mut App) {
    log::info!("export_pdf({path}) not implemented yet");
    crate::ui::toast_info(window, cx, "Export is not implemented yet");
}
