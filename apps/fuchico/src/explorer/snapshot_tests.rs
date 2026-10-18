//! Offscreen snapshot of the main window with folders expanded, an active
//! file and a cut marking (visual check of row/tab fonts, icons and
//! highlights). Writes PNGs to `target/snapshots/`; no window is shown.

use gpui_kit::{AppContext, px, size};

use crate::snapshot::{SnapTheme, copy_vault, snapshot_with};
use crate::workspace::Workspace;

const DEMO_NOTES: &str = "/Users/fuleco/Documents/Dev/obsidian-helix/demo-notes";

fn explorer_snapshot(name: &str, theme: SnapTheme) {
    let src = std::path::Path::new(DEMO_NOTES);
    if !src.is_dir() {
        eprintln!("skipping {name}: {DEMO_NOTES} not found");
        return;
    }
    let vault = copy_vault(src, name).to_string_lossy().to_string();
    let png = snapshot_with(
        name,
        size(px(1400.), px(800.)),
        theme,
        |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        move |ws, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_root(Some(vault.clone()), window, cx);
                ws.open_file(format!("{vault}/project-launch.md"), None, window, cx);
                ws.explorer().clone().update(cx, |e, cx| {
                    for dir in ["desenhos", "templates"] {
                        e.toggle(&format!("{vault}/{dir}"), cx);
                    }
                    e.cut = vec![format!("{vault}/personal.md")];
                });
            })
        },
    );
    eprintln!("snapshot: {}", png.display());
}

#[test]
fn snapshot_explorer_ayu_light() {
    explorer_snapshot("explorer-ayu-light", SnapTheme::AYU_LIGHT);
}

#[test]
fn snapshot_explorer_ayu_dark() {
    explorer_snapshot("explorer-ayu-dark", SnapTheme::AYU_DARK);
}

/// The opt-in Zed icon theme (`crate::dev_marks`) plus the GPUI badge, when
/// the Material Icon Theme extension is installed locally.
#[test]
fn snapshot_explorer_zed_icons() {
    let Some(theme_dir) = dirs::data_dir()
        .map(|d| d.join("Zed/extensions/installed/material-icon-theme"))
        .filter(|d| d.is_dir())
    else {
        eprintln!("skipping: material-icon-theme not installed");
        return;
    };
    let src = std::path::Path::new(DEMO_NOTES);
    if !src.is_dir() {
        return;
    }
    let vault = copy_vault(src, "explorer-zed-icons")
        .to_string_lossy()
        .to_string();
    let png = snapshot_with(
        "explorer-zed-icons",
        size(px(1400.), px(800.)),
        SnapTheme::AYU_DARK,
        move |window, cx| {
            cx.set_global(crate::dev_marks::DevMarks {
                icon_theme: Some(crate::dev_marks::IconTheme::load(&theme_dir).unwrap()),
                badge: true,
            });
            cx.new(|cx| Workspace::new(window, cx))
        },
        move |ws, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_root(Some(vault.clone()), window, cx);
                ws.open_file(format!("{vault}/project-launch.md"), None, window, cx);
                ws.explorer().clone().update(cx, |e, cx| {
                    for dir in ["desenhos", "templates"] {
                        e.toggle(&format!("{vault}/{dir}"), cx);
                    }
                });
            })
        },
    );
    eprintln!("snapshot: {}", png.display());
}
