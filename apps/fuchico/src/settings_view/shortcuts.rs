//! Keyboard Shortcuts (spec 5 §3.11): title + description above the
//! searchable [`crate::palette::ShortcutsList`] (search, key recorder,
//! rebindable + fixed groups). The original has no per-row reset button.

use gpui_kit::prelude::*;
use gpui_kit::{Context, Window, div};

use super::SettingsView;
use super::widgets::Section;

impl SettingsView {
    pub(super) fn shortcuts_section(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Section {
        let list = div()
            .debug_selector(|| "settings-shortcuts-slot".into())
            .child(self.shortcuts.clone());
        Section::new(crate::t!(ShortcutsTitle), crate::t!(ShortcutsDesc)).other(list)
    }
}
