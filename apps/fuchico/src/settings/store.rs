//! The process-wide settings store (`editorSettingsStore`).
//!
//! One [`SettingsStore`] global is shared by every window, which replaces the
//! original's per-webview copies synced through the `storage` event. Writers
//! call [`SettingsStore::update`]; it merges, persists the whole object and
//! notifies observers (`cx.observe_global::<SettingsStore>`) synchronously.

use std::sync::Arc;

use gpui_kit::{App, BorrowAppContext, Global};

use super::Settings;
use crate::persist;

pub struct SettingsStore {
    settings: Arc<Settings>,
}

impl Global for SettingsStore {}

impl SettingsStore {
    /// Reads `helix.editorSettings` (startup, before any window).
    pub fn load() -> SettingsStore {
        let raw = persist::load_raw(persist::KEY_SETTINGS);
        SettingsStore {
            settings: Arc::new(Settings::from_json(raw.as_deref())),
        }
    }

    /// A store holding `settings`, not backed by the persisted file (tests).
    #[cfg(test)]
    pub fn with(settings: Settings) -> SettingsStore {
        SettingsStore {
            settings: Arc::new(settings),
        }
    }

    pub fn init(store: SettingsStore, cx: &mut App) {
        crate::i18n::set_language(store.settings.language);
        cx.set_global(store);
    }

    /// Current settings snapshot (cheap clone).
    pub fn get(cx: &App) -> Arc<Settings> {
        cx.global::<SettingsStore>().settings.clone()
    }

    /// [`Self::get`] when the store is installed (views in tests may run
    /// without it).
    pub fn try_get(cx: &App) -> Option<Arc<Settings>> {
        cx.try_global::<SettingsStore>().map(|s| s.settings.clone())
    }

    /// `set(partial)`: edit, persist the whole object, notify every window.
    /// A no-op edit neither writes nor notifies.
    pub fn update(cx: &mut App, edit: impl FnOnce(&mut Settings)) {
        let mut next = (*Self::get(cx)).clone();
        edit(&mut next);
        if next == *Self::get(cx) {
            return;
        }
        persist::save(persist::KEY_SETTINGS, &next.to_json());
        if next.language != Self::get(cx).language {
            crate::i18n::set_language(next.language);
        }
        cx.update_global::<SettingsStore, _>(|store, _| store.settings = Arc::new(next));
        cx.refresh_windows();
    }

    /// `setShortcut(action, binding)`.
    pub fn set_shortcut(cx: &mut App, action: &str, binding: &str) {
        let (action, binding) = (action.to_string(), binding.to_string());
        Self::update(cx, move |s| {
            s.shortcuts.insert(action, binding);
        });
    }
}
