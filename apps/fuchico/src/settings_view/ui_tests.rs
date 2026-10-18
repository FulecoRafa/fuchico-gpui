//! Headless interaction tests for [`SettingsView`] through gpui's test
//! harness (real hit-testing and key dispatch; no OS input, no focus).
//!
//! One test drives everything: the bus and the persist directory are
//! process-wide.

use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Modifiers, Pixels, Point, TestAppContext,
    VisualTestContext, WindowBounds, WindowOptions, px, size,
};

use super::SettingsView;
use crate::i18n::Language;
use crate::settings::ui_state::{SettingsSection, UiState};
use crate::settings::{PaletteId, SettingsStore, parse_rulers};

struct Ui {
    cx: VisualTestContext,
}

impl Ui {
    fn frame(&mut self) {
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    fn bounds(&mut self, sel: &str) -> Option<Bounds<Pixels>> {
        self.frame();
        self.cx.debug_bounds(sel.to_string().leak())
    }

    fn has(&mut self, sel: &str) -> bool {
        self.bounds(sel).is_some()
    }

    fn click(&mut self, sel: &str) {
        let at = self
            .bounds(sel)
            .unwrap_or_else(|| panic!("no element {sel}"))
            .center();
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.frame();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
        self.frame();
    }

    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
        self.frame();
    }

    fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
        self.frame();
    }

    fn settings(&mut self) -> std::sync::Arc<crate::settings::Settings> {
        self.cx.update(|_, cx| SettingsStore::get(cx))
    }

    async fn wait(&mut self, mut pred: impl FnMut(&gpui_kit::App) -> bool) {
        for _ in 0..300 {
            self.frame();
            if self.cx.update(|_, cx| pred(cx)) {
                return;
            }
            self.cx.executor().timer(Duration::from_millis(10)).await;
        }
        panic!("condition not reached");
    }
}

#[gpui_kit::test]
async fn settings_nav_search_toggles_rulers_themes_and_caldav(cx: &mut TestAppContext) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "fuchico-settings-test-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    crate::persist::set_test_dir(dir.clone());

    cx.update(|cx| {
        gpui_kit::init(cx);
        SettingsStore::init(SettingsStore::load(), cx);
        cx.set_global(UiState::load());
        crate::theme::apply::apply(
            crate::theme::resolve(&crate::settings::Settings::default(), true),
            cx,
        );
        crate::bus::init(cx);
        crate::caldav::init(cx);
        crate::i18n::set_language(Language::En);
    });
    let root = dir.to_string_lossy().to_string();
    let (handle, _view): (AnyWindowHandle, Entity<SettingsView>) = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1100.), px(1300.)),
        }; // room for the 360px CSS editor
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| SettingsView::new(Some(root.clone()), window, cx)),
        )
        .expect("open test window")
    });
    let mut ui = Ui {
        cx: VisualTestContext::from_window(handle, cx),
    };

    // ---- nav + remembered section --------------------------------------------
    for s in SettingsSection::ALL {
        assert!(ui.has(&format!("settings-nav-{}", s.id())));
    }
    ui.click("settings-nav-editor");
    assert_eq!(
        ui.cx.update(|_, cx| UiState::get(cx).settings_section),
        SettingsSection::Editor
    );
    assert!(ui.has("settings-tab-2"));

    // ---- toggles write settings ----------------------------------------------
    ui.click("settings-tab-4");
    assert_eq!(ui.settings().tab_size, 4);
    ui.click("settings-lines-relative");
    assert!(ui.settings().relative_line_numbers);
    ui.click("settings-keymap-vim");
    assert_eq!(
        ui.settings().keybinding_mode,
        crate::settings::KeybindingMode::Vim
    );

    // ---- rulers commit on Enter (parsed, sorted) -----------------------------
    ui.click("settings-rulers");
    ui.type_text("120, 80");
    assert!(
        ui.settings().rulers.is_empty(),
        "rulers must not commit while typing"
    );
    ui.press("enter");
    assert_eq!(ui.settings().rulers, parse_rulers("80, 120"));

    // ---- appearance: palette, dracula disables mode, new custom theme --------
    ui.click("settings-nav-appearance");
    ui.click("settings-palette-Dracula");
    assert_eq!(ui.settings().palette, PaletteId::Dracula);
    let mode = ui.settings().mode;
    ui.click("settings-mode-light");
    assert_eq!(ui.settings().mode, mode, "mode is disabled under Dracula");
    ui.click("settings-palette-Ayu");
    ui.click("settings-mode-light");
    assert_eq!(ui.settings().mode, crate::settings::ModePref::Light);
    assert!(!ui.has("settings-theme-css"));
    ui.click("settings-theme-new");
    let s = ui.settings();
    assert_eq!(s.palette, PaletteId::Custom);
    assert_eq!(s.custom_themes.len(), 1);
    assert!(s.custom_themes[0].css.contains("--background"));
    assert!(ui.has("settings-theme-css"));
    assert!(ui.has(&format!(
        "settings-palette-custom-{}",
        s.custom_themes[0].id
    )));
    // Duplicate adds a second one and selects it.
    ui.click("settings-theme-duplicate");
    let s = ui.settings();
    assert_eq!(s.custom_themes.len(), 2);
    assert_eq!(
        s.custom_theme_id.as_deref(),
        Some(s.custom_themes[1].id.as_str())
    );
    assert_ne!(s.custom_themes[0].name, s.custom_themes[1].name);

    // ---- CSS draft: edits apply only on Apply ----------------------------------
    ui.click("settings-theme-css");
    ui.press("cmd-end");
    ui.type_text("\n/* tweak */");
    let id = ui.settings().custom_theme_id.clone().unwrap();
    let saved = |s: &crate::settings::Settings| {
        s.custom_themes
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .css
            .clone()
    };
    assert!(
        !saved(&ui.settings()).contains("tweak"),
        "draft is not saved before Apply"
    );
    ui.click("settings-theme-apply");
    assert!(saved(&ui.settings()).contains("/* tweak */"));

    // ---- language -------------------------------------------------------------
    ui.click("settings-language-pt-BR");
    assert_eq!(ui.settings().language, Language::PtBr);
    ui.click("settings-language-en");
    assert_eq!(ui.settings().language, Language::En);

    // ---- search across sections ----------------------------------------------
    ui.click("settings-search");
    ui.type_text("rulers");
    assert!(ui.has("settings-group-editor"));
    assert!(!ui.has("settings-group-appearance"));
    assert!(ui.has("settings-rulers"));
    assert!(!ui.has("settings-tab-2"), "non-matching fields are hidden");
    ui.type_text("zzqq");
    assert!(ui.has("settings-no-results"));
    // A nav click clears the search.
    ui.click("settings-nav-integrations");
    assert!(!ui.has("settings-no-results"));

    // ---- integrations (no accounts) -------------------------------------------
    ui.wait(|cx| crate::caldav::ctx(cx).is_some()).await;
    assert!(ui.has("caldav-test"));
    assert!(ui.has("caldav-save"));
    assert!(ui.has("caldav-sync-now"));

    // ---- shortcuts slot / about ------------------------------------------------
    ui.click("settings-nav-shortcuts");
    assert!(ui.has("settings-shortcuts-slot"));
    // The palette's ShortcutsList is mounted there, with its rebindable chips.
    assert!(ui.has("shortcut-key-insertTable"));
    ui.click("settings-nav-about");
    assert!(ui.has("settings-about-github"));
}
