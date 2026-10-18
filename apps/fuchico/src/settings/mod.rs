//! The settings model (port of `lib/editorSettings.ts`, spec 5 §1).
//!
//! [`Settings`] serializes to exactly the `helix.editorSettings` JSON shape
//! (camelCase keys); unknown keys survive a round trip through
//! [`Settings::extra`]. [`Settings::from_json`] implements the load rules:
//! defaults merged with the stored object, shortcuts merged per action, the
//! `customThemeCss` migration and the dangling-id repair.
//!
//! Deviation (documented in the parity notes): the original used a
//! wrongly-typed stored value verbatim; a typed port cannot, so such a field
//! falls back to its default individually instead of failing the whole load.

pub mod store;
pub mod ui_state;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use store::SettingsStore;

use crate::i18n::Language;

pub const EDITOR_FONT_SIZE_DEFAULT: f32 = 13.;
pub const EDITOR_FONT_SIZE_MIN: f32 = 8.;
pub const EDITOR_FONT_SIZE_MAX: f32 = 32.;
pub const UI_SCALE_DEFAULT: f32 = 1.;
pub const UI_SCALE_MIN: f32 = 0.7;
pub const UI_SCALE_MAX: f32 = 1.6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaletteId {
    #[default]
    Ayu,
    Dracula,
    Catppuccin,
    Custom,
}

impl PaletteId {
    pub const BUILT_IN: [PaletteId; 3] = [PaletteId::Ayu, PaletteId::Dracula, PaletteId::Catppuccin];

    /// Proper-noun display name (not translated).
    pub fn display_name(self) -> &'static str {
        match self {
            PaletteId::Ayu => "Ayu",
            PaletteId::Dracula => "Dracula",
            PaletteId::Catppuccin => "Catppuccin",
            PaletteId::Custom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeybindingMode {
    #[default]
    Helix,
    Vim,
    Normal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub css: String,
}

/// Default bindings of the rebindable editor actions (spec 5 §2.2).
pub const DEFAULT_SHORTCUTS: [(&str, &str); 9] = [
    ("openOutline", "Mod-o"),
    ("toggleCheckboxAtCursor", "Mod-Enter"),
    ("insertDate", "Mod-Shift-d"),
    ("insertDateTime", "Mod-Alt-d"),
    ("insertRegion", "Mod-Shift-r"),
    ("insertTable", "Mod-Alt-t"),
    ("toggleTaskLine", "Mod-Shift-Enter"),
    ("pickDueDate", "Mod-Shift-."),
    ("pickRecurrence", "Mod-Shift-,"),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub language: Language,
    pub palette: PaletteId,
    pub mode: ModePref,
    pub custom_themes: Vec<CustomTheme>,
    pub custom_theme_id: Option<String>,
    pub keybinding_mode: KeybindingMode,
    /// Action id → binding. Keys outside [`DEFAULT_SHORTCUTS`] are preserved.
    pub shortcuts: BTreeMap<String, String>,
    pub fold_start_marker: String,
    pub fold_end_marker: String,
    pub ui_font: String,
    pub editor_font: String,
    pub relative_line_numbers: bool,
    pub tab_size: u32,
    pub rulers: Vec<u32>,
    pub editor_font_size: f32,
    pub ui_scale: f32,
    pub daily_notes_folder: String,
    pub templates_folder: String,
    pub external_tool: String,
    /// Unknown keys, kept as-is.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: Language::System,
            palette: PaletteId::Ayu,
            mode: ModePref::System,
            custom_themes: Vec::new(),
            custom_theme_id: None,
            keybinding_mode: KeybindingMode::Helix,
            shortcuts: DEFAULT_SHORTCUTS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            fold_start_marker: ":::fold".into(),
            fold_end_marker: ":::endfold".into(),
            ui_font: String::new(),
            editor_font: String::new(),
            relative_line_numbers: false,
            tab_size: 2,
            rulers: Vec::new(),
            editor_font_size: EDITOR_FONT_SIZE_DEFAULT,
            ui_scale: UI_SCALE_DEFAULT,
            daily_notes_folder: "daily".into(),
            templates_folder: "templates".into(),
            external_tool: String::new(),
            extra: Map::new(),
        }
    }
}

impl Settings {
    /// Load rules of spec 5 §1.1. `None`/unparsable → defaults.
    pub fn from_json(raw: Option<&str>) -> Settings {
        let Some(Value::Object(parsed)) = raw.and_then(|r| serde_json::from_str::<Value>(r).ok()) else {
            return Settings::default();
        };
        let Value::Object(defaults) = serde_json::to_value(Settings::default()).expect("defaults serialize") else {
            unreachable!("settings serialize to an object")
        };

        let mut merged = defaults.clone();
        for (k, v) in &parsed {
            if k == "shortcuts"
                && let (Some(Value::Object(dst)), Value::Object(src)) = (merged.get_mut("shortcuts"), v)
            {
                for (a, b) in src {
                    dst.insert(a.clone(), b.clone());
                }
                continue;
            }
            merged.insert(k.clone(), v.clone());
        }

        let mut settings = match serde_json::from_value::<Settings>(Value::Object(merged.clone())) {
            Ok(s) => s,
            Err(_) => Self::lenient(&defaults, merged),
        };
        settings.migrate();
        settings
    }

    /// Per-field fallback: a stored key whose value doesn't fit its type is
    /// replaced by its default (shortcut entries are checked one by one).
    fn lenient(defaults: &Map<String, Value>, mut merged: Map<String, Value>) -> Settings {
        if let Some(Value::Object(sc)) = merged.get_mut("shortcuts") {
            sc.retain(|_, v| v.is_string());
        } else {
            merged.insert("shortcuts".into(), defaults["shortcuts"].clone());
        }
        for (key, default) in defaults {
            let mut probe = defaults.clone();
            probe.insert(key.clone(), merged[key].clone());
            if serde_json::from_value::<Settings>(Value::Object(probe)).is_err() {
                merged.insert(key.clone(), default.clone());
            }
        }
        serde_json::from_value(Value::Object(merged)).unwrap_or_default()
    }

    /// `customThemeCss` → one "Custom" theme; repair a dangling `customThemeId`.
    fn migrate(&mut self) {
        if let Some(Value::String(css)) = self.extra.remove("customThemeCss")
            && !css.trim().is_empty()
            && self.custom_themes.is_empty()
        {
            let theme = CustomTheme { id: new_custom_theme_id(), name: "Custom".into(), css };
            self.custom_theme_id = Some(theme.id.clone());
            self.custom_themes.push(theme);
        }
        if let Some(id) = &self.custom_theme_id
            && !self.custom_themes.iter().any(|t| &t.id == id)
        {
            self.custom_theme_id = self.custom_themes.first().map(|t| t.id.clone());
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("settings serialize")
    }

    /// `activeCustomTheme(s)`.
    pub fn active_custom_theme(&self) -> Option<&CustomTheme> {
        let id = self.custom_theme_id.as_deref()?;
        self.custom_themes.iter().find(|t| t.id == id)
    }

    /// Current binding of a rebindable action (falls back to its default).
    pub fn shortcut(&self, action: &str) -> &str {
        self.shortcuts
            .get(action)
            .map(String::as_str)
            .or_else(|| DEFAULT_SHORTCUTS.iter().find(|(a, _)| *a == action).map(|(_, b)| *b))
            .unwrap_or("")
    }
}

/// `theme-<Date.now() base36>-<5 random base36 chars>`.
pub fn new_custom_theme_id() -> String {
    use rand::Rng;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut rng = rand::rng();
    let suffix: String = (0..5).map(|_| base36_digit(rng.random_range(0..36))).collect();
    format!("theme-{}-{}", base36(ms), suffix)
}

fn base36_digit(d: u32) -> char {
    char::from_digit(d, 36).expect("digit < 36")
}

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(base36_digit((n % 36) as u32));
        n /= 36;
    }
    out.iter().rev().collect()
}

/// `parseRulers`: split on `[\s,;]+`, keep integers `1..=1000`, dedupe, sort.
pub fn parse_rulers(input: &str) -> Vec<u32> {
    let mut out: Vec<u32> = input
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .filter(|n| n.fract() == 0. && (1. ..=1000.).contains(n))
        .map(|n| n as u32)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Canonical `a, b` rendering of rulers.
pub fn format_rulers(rulers: &[u32]) -> String {
    rulers.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

/// `clampEditorFontSize`: non-finite → 13; clamp `[8, 32]` (no rounding).
pub fn clamp_editor_font_size(v: f32) -> f32 {
    if !v.is_finite() {
        return EDITOR_FONT_SIZE_DEFAULT;
    }
    v.clamp(EDITOR_FONT_SIZE_MIN, EDITOR_FONT_SIZE_MAX)
}

/// `clampUiScale`: non-finite → 1; clamp `[0.7, 1.6]`, round to 2 decimals.
pub fn clamp_ui_scale(v: f32) -> f32 {
    if !v.is_finite() {
        return UI_SCALE_DEFAULT;
    }
    (v.clamp(UI_SCALE_MIN, UI_SCALE_MAX) * 100.).round() / 100.
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS_JSON: &str = r#"{"language":"system","palette":"ayu","mode":"system","customThemes":[],"customThemeId":null,
 "keybindingMode":"helix","shortcuts":{"openOutline":"Mod-o","toggleCheckboxAtCursor":"Mod-Enter",
 "insertDate":"Mod-Shift-d","insertDateTime":"Mod-Alt-d","insertRegion":"Mod-Shift-r","insertTable":"Mod-Alt-t",
 "toggleTaskLine":"Mod-Shift-Enter","pickDueDate":"Mod-Shift-.","pickRecurrence":"Mod-Shift-,"},
 "foldStartMarker":":::fold","foldEndMarker":":::endfold","uiFont":"","editorFont":"",
 "relativeLineNumbers":false,"tabSize":2,"rulers":[],"editorFontSize":13,"uiScale":1,
 "dailyNotesFolder":"daily","templatesFolder":"templates","externalTool":""}"#;

    #[test]
    fn defaults_match_the_original_json() {
        let expected: Value = serde_json::from_str(DEFAULTS_JSON).unwrap();
        let mut actual = Settings::default().to_json();
        // f32 serializes as 13.0 / 1.0; compare numerically.
        for k in ["editorFontSize", "uiScale"] {
            assert_eq!(actual[k].as_f64(), expected[k].as_f64());
            actual[k] = expected[k].clone();
        }
        assert_eq!(actual, expected);
        assert_eq!(Settings::from_json(Some(DEFAULTS_JSON)), Settings::default());
    }

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        assert_eq!(Settings::from_json(None), Settings::default());
        assert_eq!(Settings::from_json(Some("{not json")), Settings::default());
        assert_eq!(Settings::from_json(Some("[1,2]")), Settings::default());
    }

    #[test]
    fn merges_partial_and_keeps_unknown_keys() {
        let s = Settings::from_json(Some(
            r#"{"palette":"dracula","shortcuts":{"insertDate":"Mod-d","future":"x"},"someFutureKey":[1]}"#,
        ));
        assert_eq!(s.palette, PaletteId::Dracula);
        assert_eq!(s.shortcut("insertDate"), "Mod-d");
        assert_eq!(s.shortcut("openOutline"), "Mod-o");
        assert_eq!(s.shortcuts.get("future").map(String::as_str), Some("x"));
        assert_eq!(s.extra.get("someFutureKey"), Some(&serde_json::json!([1])));
        assert_eq!(s.to_json()["someFutureKey"], serde_json::json!([1]));
    }

    #[test]
    fn wrong_typed_field_falls_back_individually() {
        let s = Settings::from_json(Some(r#"{"tabSize":"four","palette":"catppuccin","mode":"sepia","shortcuts":{"insertDate":5}}"#));
        assert_eq!(s.tab_size, 2);
        assert_eq!(s.mode, ModePref::System);
        assert_eq!(s.palette, PaletteId::Catppuccin);
        assert_eq!(s.shortcut("insertDate"), "Mod-Shift-d");
    }

    #[test]
    fn migrates_custom_theme_css() {
        let s = Settings::from_json(Some(r#"{"customThemeCss":"--background: #000;","palette":"custom"}"#));
        assert_eq!(s.custom_themes.len(), 1);
        let t = &s.custom_themes[0];
        assert_eq!(t.name, "Custom");
        assert_eq!(t.css, "--background: #000;");
        assert!(t.id.starts_with("theme-"));
        assert_eq!(s.custom_theme_id.as_deref(), Some(t.id.as_str()));
        assert!(!s.extra.contains_key("customThemeCss"));
        assert_eq!(s.active_custom_theme().unwrap().id, t.id);

        let blank = Settings::from_json(Some(r#"{"customThemeCss":"   "}"#));
        assert!(blank.custom_themes.is_empty());
    }

    #[test]
    fn repairs_dangling_theme_id() {
        let s = Settings::from_json(Some(
            r#"{"customThemes":[{"id":"a","name":"A","css":""}],"customThemeId":"gone"}"#,
        ));
        assert_eq!(s.custom_theme_id.as_deref(), Some("a"));
        let s = Settings::from_json(Some(r#"{"customThemeId":"gone"}"#));
        assert_eq!(s.custom_theme_id, None);
    }

    #[test]
    fn theme_ids_look_right() {
        let id = new_custom_theme_id();
        let parts: Vec<_> = id.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2].len(), 5);
        assert!(parts[1].chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }

    #[test]
    fn rulers() {
        assert_eq!(parse_rulers("120, 80;80  0 1001 7.5 abc 1000"), vec![80, 120, 1000]);
        assert_eq!(parse_rulers(""), Vec::<u32>::new());
        assert_eq!(format_rulers(&[80, 120]), "80, 120");
    }

    #[test]
    fn clamps() {
        assert_eq!(clamp_editor_font_size(f32::NAN), 13.);
        assert_eq!(clamp_editor_font_size(4.), 8.);
        assert_eq!(clamp_editor_font_size(40.), 32.);
        assert_eq!(clamp_editor_font_size(13.5), 13.5);
        assert_eq!(clamp_ui_scale(f32::INFINITY), 1.);
        assert_eq!(clamp_ui_scale(0.1), 0.7);
        assert_eq!(clamp_ui_scale(9.), 1.6);
        assert_eq!(clamp_ui_scale(1.0 + 0.1 + 0.1), 1.2);
        assert_eq!(clamp_ui_scale(1.234), 1.23);
    }
}
