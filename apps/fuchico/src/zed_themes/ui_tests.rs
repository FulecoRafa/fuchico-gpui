//! Headless test (gpui's test window; no OS input, no focus): pick a Zed
//! icon theme and a Zed colour theme in Settings → Appearance by clicking
//! the toggles, then check the explorer rows, the editor tabs and the app
//! theme use them, and that "Built-in" brings the Lucide icons back.
//! Themes are imported from the fixtures into the test's scratch config.

use gpui_kit::{Entity, TestAppContext};

use super::test_support::fixtures;
use crate::persist;
use crate::settings::ui_state::SettingsSection;
use crate::settings::{ModePref, PaletteId, SettingsStore};
use crate::test_ui::{self, Ui};
use crate::theme::{AppTheme, hex};
use crate::workspace::Workspace;

fn open_settings(ui: &mut Ui, ws: &Entity<Workspace>) {
    ui.cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_settings(SettingsSection::Appearance, window, cx)
        })
    });
    ui.frame();
}

fn show_editor(ui: &mut Ui, ws: &Entity<Workspace>, file: &str) {
    let file = file.to_string();
    ui.cx
        .update(|window, cx| ws.update(cx, |ws, cx| ws.open_file(file, None, window, cx)));
    ui.frame();
}

#[gpui_kit::test]
async fn settings_pick_zed_icon_and_colour_themes(cx: &mut TestAppContext) {
    let cfg = test_ui::temp_dir("zed-ui-cfg");
    persist::set_test_dir(cfg.clone());
    let dest = super::imported_dir().unwrap();
    assert!(dest.starts_with(&cfg), "imports stay in the scratch config");
    super::import::import_path(&fixtures().join("one"), &dest).unwrap();
    super::import::import_path(&fixtures().join("tiny-icons"), &dest).unwrap();
    cx.update(|cx| {
        test_ui::init_shell(cx);
        super::reload(cx);
        SettingsStore::update(cx, |s| s.mode = ModePref::Dark);
    });

    let vault = test_ui::temp_dir("zed-ui-vault");
    std::fs::create_dir_all(vault.join("templates")).unwrap();
    for f in ["note.md", "README.md", "templates/t.md"] {
        std::fs::write(vault.join(f), "# x\n").unwrap();
    }
    let root = vault.to_string_lossy().to_string();
    let note = format!("{root}/note.md");

    let (mut ui, ws) = test_ui::open(cx, (1200., 2400.), Workspace::new);
    {
        let (root, note) = (root.clone(), note.clone());
        ui.cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_root(Some(root), window, cx);
                ws.open_file(note, None, window, cx);
            })
        });
    }
    ui.cx.run_until_parked();
    ui.frame();

    // Default: built-in Lucide icons, no tab icons (parity with Tauri).
    assert!(ui.has("ex-icon:note.md:lucide"));
    assert!(ui.has("ex-icon:templates:lucide"));
    assert!(!ui.has("tab-icon-0:lucide"));

    // Settings → Appearance lists the imported themes.
    open_settings(&mut ui, &ws);
    assert!(ui.has("settings-zed-theme-one-dark"));
    assert!(ui.has("settings-zed-theme-one-light"));
    assert!(ui.has("settings-zed-import"));
    assert!(ui.has("settings-icon-theme-none"));

    ui.click("settings-icon-theme-tiny-icons");
    // Clicking a light theme while dark paints exactly it: the mode follows.
    ui.click("settings-zed-theme-one-light");
    ui.cx.read(|cx| {
        let s = SettingsStore::get(cx);
        assert_eq!(s.icon_theme.as_deref(), Some("Tiny Icons"));
        assert_eq!(s.palette, PaletteId::Zed);
        assert_eq!(s.zed_theme.as_deref(), Some("One Light"));
        assert_eq!(s.mode, ModePref::Light);
        let th = AppTheme::get(cx);
        assert_eq!(th.zed_theme.as_deref(), Some("One Light"));
        assert!(!th.appearance.is_dark());
        assert_eq!(hex(th.colors.background), "#fafafa");
    });
    // Persisted through the settings store.
    persist::flush();
    let saved = persist::load_raw(persist::KEY_SETTINGS).unwrap_or_default();
    assert!(saved.contains("\"zedTheme\""), "{saved}");
    assert!(saved.contains("\"iconTheme\""), "{saved}");

    // The mode still switches within the family.
    ui.click("settings-mode-dark");
    ui.cx.read(|cx| {
        let th = AppTheme::get(cx);
        assert_eq!(th.zed_theme.as_deref(), Some("One Dark"));
        assert_eq!(hex(th.colors.background), "#282c33");
    });

    // Explorer rows and tabs use the icon theme.
    show_editor(&mut ui, &ws, &note);
    assert!(ui.has("ex-icon:note.md:markdown"));
    assert!(ui.has("ex-icon:README.md:readme"));
    assert!(ui.has("ex-icon:templates:folder-templates"));
    assert!(ui.has("tab-icon-0:markdown"));
    ui.cx.update(|_, cx| {
        let ex = ws.read(cx).explorer().clone();
        ex.update(cx, |e, cx| e.toggle(&format!("{root}/templates"), cx));
    });
    ui.cx.run_until_parked();
    ui.frame();
    assert!(ui.has("ex-icon:templates:folder-templates-open"));
    assert!(ui.has("ex-icon:t.md:markdown"));

    // Back to the built-in icons.
    open_settings(&mut ui, &ws);
    ui.click("settings-icon-theme-none");
    show_editor(&mut ui, &ws, &note);
    assert!(ui.has("ex-icon:note.md:lucide"));
    assert!(!ui.has("tab-icon-0:markdown"));
    ui.cx
        .read(|cx| assert_eq!(SettingsStore::get(cx).icon_theme, None));
}
