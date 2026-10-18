//! Small persisted UI state outside the settings blob (spec 5 §1.3, spec 1):
//! `helix.lastRootPath`, `helix.scrollPositions`, `helix.settingsSection`.

use std::time::Duration;

use gpui_kit::{App, BorrowAppContext, Global, Task};
use serde::{Deserialize, Serialize};

use crate::persist;

/// Settings view sections, in nav order (`SETTINGS_SECTIONS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsSection {
    #[default]
    Appearance,
    Editor,
    Shortcuts,
    Vault,
    Integrations,
    About,
}

impl SettingsSection {
    pub const ALL: [SettingsSection; 6] = [
        SettingsSection::Appearance,
        SettingsSection::Editor,
        SettingsSection::Shortcuts,
        SettingsSection::Vault,
        SettingsSection::Integrations,
        SettingsSection::About,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SettingsSection::Appearance => "appearance",
            SettingsSection::Editor => "editor",
            SettingsSection::Shortcuts => "shortcuts",
            SettingsSection::Vault => "vault",
            SettingsSection::Integrations => "integrations",
            SettingsSection::About => "about",
        }
    }

    pub fn from_id(id: &str) -> Option<SettingsSection> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// Max remembered scroll positions (most recent last).
pub const SCROLL_POSITIONS_MAX: usize = 500;
const SCROLL_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

/// Most-recently-used list of `(path, scrollTop)`, serialized as the
/// original's JSON array of `[path, number]` pairs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScrollPositions(Vec<(String, f64)>);

impl ScrollPositions {
    pub fn get(&self, path: &str) -> Option<f64> {
        self.0
            .iter()
            .rev()
            .find(|(p, _)| p == path)
            .map(|(_, v)| *v)
    }

    /// Moves `path` to the most-recent end and trims to the cap.
    pub fn set(&mut self, path: &str, value: f64) {
        self.0.retain(|(p, _)| p != path);
        self.0.push((path.to_string(), value));
        let excess = self.0.len().saturating_sub(SCROLL_POSITIONS_MAX);
        self.0.drain(..excess);
    }

    /// Rename propagation (`from` and `from/...`).
    pub fn rename(&mut self, from: &str, to: &str) {
        for (p, _) in &mut self.0 {
            if let Some(rest) = crate::workspace::tabs::renamed(p, from, to) {
                *p = rest;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Global holder of the three keys.
pub struct UiState {
    pub last_root: Option<String>,
    pub scroll: ScrollPositions,
    pub settings_section: SettingsSection,
    scroll_save: Option<Task<()>>,
}

impl Global for UiState {}

impl UiState {
    /// Startup read (before any window).
    pub fn load() -> UiState {
        let section = persist::load::<String>(persist::KEY_SETTINGS_SECTION)
            .and_then(|s| SettingsSection::from_id(&s))
            .unwrap_or_default();
        UiState {
            last_root: persist::load::<String>(persist::KEY_LAST_ROOT).filter(|s| !s.is_empty()),
            scroll: persist::load(persist::KEY_SCROLL_POSITIONS).unwrap_or_default(),
            settings_section: section,
            scroll_save: None,
        }
    }

    pub fn get(cx: &App) -> &UiState {
        cx.global::<UiState>()
    }

    pub fn set_last_root(cx: &mut App, root: Option<String>) {
        match &root {
            Some(r) => persist::save(persist::KEY_LAST_ROOT, r),
            None => persist::remove(persist::KEY_LAST_ROOT),
        }
        cx.global_mut::<UiState>().last_root = root;
    }

    /// `settingsNav.set(id)`: no-op if unchanged.
    pub fn set_settings_section(cx: &mut App, section: SettingsSection) {
        if cx.global::<UiState>().settings_section == section {
            return;
        }
        persist::save(persist::KEY_SETTINGS_SECTION, section.id());
        cx.update_global::<UiState, _>(|s, _| s.settings_section = section);
    }

    /// Records a scroll offset; the file write is debounced by 400ms.
    pub fn set_scroll(cx: &mut App, path: &str, value: f64) {
        cx.global_mut::<UiState>().scroll.set(path, value);
        let task = cx.spawn(async move |cx| {
            cx.background_executor().timer(SCROLL_SAVE_DEBOUNCE).await;
            cx.update(|cx| {
                persist::save(
                    persist::KEY_SCROLL_POSITIONS,
                    &cx.global::<UiState>().scroll,
                )
            });
        });
        cx.global_mut::<UiState>().scroll_save = Some(task);
    }

    pub fn rename_path(cx: &mut App, from: &str, to: &str) {
        cx.global_mut::<UiState>().scroll.rename(from, to);
        persist::save(
            persist::KEY_SCROLL_POSITIONS,
            &cx.global::<UiState>().scroll,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_positions_are_mru_and_capped() {
        let mut s = ScrollPositions::default();
        for i in 0..(SCROLL_POSITIONS_MAX + 5) {
            s.set(&format!("/f{i}"), i as f64);
        }
        assert_eq!(s.len(), SCROLL_POSITIONS_MAX);
        assert_eq!(s.get("/f0"), None);
        s.set("/f10", 1.5);
        assert_eq!(s.get("/f10"), Some(1.5));
        assert_eq!(
            serde_json::to_value(&s)
                .unwrap()
                .as_array()
                .unwrap()
                .last()
                .unwrap(),
            &serde_json::json!(["/f10", 1.5])
        );
        let parsed: ScrollPositions = serde_json::from_str(r#"[["/a", 3], ["/b", 4.5]]"#).unwrap();
        assert_eq!(parsed.get("/b"), Some(4.5));
    }

    #[test]
    fn rename_moves_prefixes() {
        let mut s = ScrollPositions::default();
        s.set("/v/dir/a.md", 1.);
        s.set("/v/dirx.md", 2.);
        s.rename("/v/dir", "/v/new");
        assert_eq!(s.get("/v/new/a.md"), Some(1.));
        assert_eq!(s.get("/v/dirx.md"), Some(2.));
    }

    #[test]
    fn section_ids() {
        assert_eq!(
            SettingsSection::from_id("vault"),
            Some(SettingsSection::Vault)
        );
        assert_eq!(SettingsSection::from_id("nope"), None);
        assert_eq!(
            serde_json::to_string(&SettingsSection::About).unwrap(),
            "\"about\""
        );
    }
}
