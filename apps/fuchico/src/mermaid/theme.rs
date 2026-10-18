//! AppTheme -> fuchico-mermaid theme / strings.

use fuchico_mermaid::{MermaidStrings, MermaidTheme};

use crate::theme::{AppTheme, hex};

/// The `--pele-*` mapping of the original reads the CSS variables of the
/// current theme; here the resolved AppTheme tokens are handed over.
pub fn mermaid_theme(th: &AppTheme) -> MermaidTheme {
    let c = &th.colors;
    let mut t = if th.appearance.is_dark() { MermaidTheme::dark() } else { MermaidTheme::light() };
    t.background = hex(c.background);
    t.foreground = hex(c.foreground);
    t.card = hex(c.card);
    t.primary = hex(c.primary);
    t.secondary = hex(c.secondary);
    t.muted_foreground = hex(c.muted_foreground);
    t.destructive = hex(c.destructive);
    t.border = hex(c.border);
    t.ring = hex(c.ring);
    t.accent = hex(c.accent);
    t.syntax_keyword = hex(c.syntax_keyword);
    t.syntax_string = hex(c.syntax_string);
    t.syntax_number = hex(c.syntax_number);
    t.syntax_function = hex(c.syntax_function);
    t.syntax_type = hex(c.syntax_type);
    t.syntax_comment = hex(c.syntax_comment);
    t.font_sans = format!("{}, {}", th.ui_font, t.font_sans);
    t.font_mono = format!("{}, {}", th.mono_font, t.font_mono);
    t.radius_sm = th.radius.sm.round().max(0.0) as u32;
    t.dark = th.appearance.is_dark();
    t
}

/// The view's UI strings in the current locale.
pub fn mermaid_strings() -> MermaidStrings {
    use crate::t;
    MermaidStrings {
        loading: t!(CommonLoading).into(),
        removed: t!(MermaidRemoved).into(),
        zoom_out: t!(MermaidZoomOut).into(),
        zoom_in: t!(MermaidZoomIn).into(),
        reset_view: t!(MermaidResetView).into(),
        open_in_window: t!(MermaidOpenInWindow).into(),
        close_preview: t!(MermaidClosePreview).into(),
        dock_tooltip: t!(MermaidDockTooltip).into(),
        dock: t!(MermaidDock).into(),
    }
}
