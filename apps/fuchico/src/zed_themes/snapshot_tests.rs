//! Offscreen snapshots with Zed themes: the main window (explorer with
//! folders expanded + an open note) painted by the One fixture and using
//! the Material Icon Theme (imported read-only from the local Zed install,
//! else the `tiny-icons` fixture), plus the Appearance settings section.
//! PNGs go to `target/snapshots/`; no window is shown, and the import
//! lands in the snapshot's scratch config dir.

use gpui_kit::{AppContext, px, size};

use super::test_support::fixtures;
use crate::settings::ui_state::SettingsSection;
use crate::settings::{ModePref, PaletteId, SettingsStore};
use crate::snapshot::{SnapTheme, copy_vault, snapshot_with};
use crate::workspace::Workspace;

const DEMO_NOTES: &str = "/Users/fuleco/Documents/Dev/obsidian-helix/demo-notes";

/// The icon theme source: Zed's installed Material Icon Theme if present.
fn icon_source() -> (std::path::PathBuf, &'static str) {
    dirs::data_dir()
        .map(|d| d.join("Zed/extensions/installed/material-icon-theme"))
        .filter(|d| d.is_dir())
        .map(|d| (d, "Material Icon Theme"))
        .unwrap_or_else(|| (fixtures().join("tiny-icons"), "Tiny Icons"))
}

fn zed_shot(
    name: &str,
    color_theme: &'static str,
    mode: ModePref,
    settings: bool,
) -> Option<std::path::PathBuf> {
    let src = std::path::Path::new(DEMO_NOTES);
    if !src.is_dir() {
        eprintln!("skipping {name}: {DEMO_NOTES} not found");
        return None;
    }
    let vault = copy_vault(src, &format!("{name}/demo-notes"))
        .to_string_lossy()
        .to_string();
    let png = snapshot_with(
        name,
        size(px(1400.), px(860.)),
        SnapTheme::AYU_DARK,
        move |window, cx| {
            // `snapshot_with` already pointed the config dir at a scratch dir.
            let dest = super::imported_dir().expect("scratch config dir");
            let (icons, icon_name) = icon_source();
            super::import::import_path(&fixtures().join("one"), &dest).unwrap();
            super::import::import_path(&icons, &dest).unwrap();
            super::reload(cx);
            SettingsStore::update(cx, |s| {
                s.palette = PaletteId::Zed;
                s.zed_theme = Some(color_theme.into());
                s.mode = mode;
                s.icon_theme = Some(icon_name.into());
            });
            cx.set_global(crate::dev_marks::DevMarks { badge: true });
            cx.new(|cx| Workspace::new(window, cx))
        },
        move |ws, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_root(Some(vault.clone()), window, cx);
                ws.open_file(format!("{vault}/project-launch.md"), None, window, cx);
                ws.open_file(format!("{vault}/personal.md"), None, window, cx);
                ws.explorer().clone().update(cx, |e, cx| {
                    for dir in ["desenhos", "templates"] {
                        e.toggle(&format!("{vault}/{dir}"), cx);
                    }
                });
                if settings {
                    ws.open_settings(SettingsSection::Appearance, window, cx);
                }
            })
        },
    );
    eprintln!("snapshot: {}", png.display());
    Some(png)
}

#[test]
fn snapshot_zed_one_dark_material_icons() {
    zed_shot("zed-one-dark-icons", "One Dark", ModePref::Dark, false);
}

#[test]
fn snapshot_zed_one_light_material_icons() {
    // Selecting One Dark in light mode paints its counterpart, One Light.
    zed_shot("zed-one-light-icons", "One Dark", ModePref::Light, false);
}

#[test]
fn snapshot_zed_settings_appearance() {
    zed_shot("zed-settings-appearance", "One Dark", ModePref::Dark, true);
}
