//! Keyboard Shortcuts (spec 5 §3.11): title + description and a slot for
//! the list.
//!
//! TODO(merge): host `palette::ShortcutsList` (wave2/palette,
//! `apps/fuchico/src/palette/shortcuts.rs`: search, key recorder,
//! rebindable + fixed groups) in [`slot`]. It is not reimplemented here.

use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Window, div};

use super::SettingsView;
use super::widgets::Section;
use crate::theme::AppTheme;
use crate::ui::u;

/// Placeholder where `palette::ShortcutsList` mounts.
fn slot(cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .debug_selector(|| "settings-shortcuts-slot".into())
        .min_h(u(40.))
        .border_1()
        .border_dashed()
        .border_color(th.colors.border)
        .rounded(u(th.radius.sm))
}

impl SettingsView {
    pub(super) fn shortcuts_section(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Section {
        Section::new(crate::t!(ShortcutsTitle), crate::t!(ShortcutsDesc)).other(slot(cx))
    }
}
