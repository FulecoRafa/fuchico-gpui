//! Offscreen snapshots of the main window (Ayu dark, a copy of the
//! original's `demo-notes`) for side-by-side checks against
//! `obsidian-helix/docs/screenshots/`. Writes PNGs to `target/snapshots/`;
//! no window is shown and nothing takes focus.

use gpui_kit::{AppContext, px, size};

use super::{MainViewKind, Workspace};
use crate::palette::PaletteKind;
use crate::settings::ui_state::SettingsSection;
use crate::snapshot::{SnapTheme, copy_vault, snapshot_with};

const DEMO_NOTES: &str = "/Users/fuleco/Documents/Dev/obsidian-helix/demo-notes";

/// Same logical size as the original screenshots (3600x2260 @2x).
fn shot(
    name: &str,
    prepare: impl FnOnce(
        &mut Workspace,
        String,
        &mut gpui_kit::Window,
        &mut gpui_kit::Context<Workspace>,
    ) + 'static,
) {
    let src = std::path::Path::new(DEMO_NOTES);
    if !src.is_dir() {
        eprintln!("skipping {name}: {DEMO_NOTES} not found");
        return;
    }
    // Nested so the explorer header reads "demo-notes" like the original.
    let vault = copy_vault(src, &format!("{name}/demo-notes"));
    // The demo notes carry no tags; give the copy a couple so Tags has content.
    for (file, tags) in [
        ("project-launch.md", "#launch #planning"),
        ("personal.md", "#personal #planning"),
    ] {
        let p = vault.join(file);
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        std::fs::write(&p, format!("{text}\n{tags}\n")).unwrap();
    }
    let vault = vault.to_string_lossy().to_string();
    let png = snapshot_with(
        name,
        size(px(1800.), px(1130.)),
        SnapTheme::AYU_DARK,
        |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        move |ws, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_root(Some(vault.clone()), window, cx);
                prepare(ws, vault, window, cx);
            })
        },
    );
    eprintln!("snapshot: {}", png.display());
}

#[test]
fn snapshot_search_query() {
    shot("search-ayu-dark", |ws, _, window, cx| {
        ws.set_view(MainViewKind::Search, window, cx);
        if let Some(s) = ws.search.clone() {
            s.update(cx, |s, cx| s.set_query("launch", window, cx));
        }
    });
}

#[test]
fn snapshot_tags_selected() {
    shot("tags-ayu-dark", |ws, _, window, cx| {
        ws.set_view(MainViewKind::Tags, window, cx);
        if let Some(t) = ws.tags.clone() {
            t.update(cx, |t, cx| t.select_tag("planning".into(), 1, cx));
        }
    });
}

#[test]
fn snapshot_settings_appearance() {
    shot("settings-appearance-ayu-dark", |ws, _, window, cx| {
        ws.open_settings(SettingsSection::Appearance, window, cx)
    });
}

#[test]
fn snapshot_settings_editor() {
    shot("settings-editor-ayu-dark", |ws, _, window, cx| {
        ws.open_settings(SettingsSection::Editor, window, cx)
    });
}

#[test]
fn snapshot_palette_open() {
    shot("palette-ayu-dark", |ws, vault, window, cx| {
        ws.open_file(format!("{vault}/project-launch.md"), None, window, cx);
        crate::palette::open(
            ws,
            PaletteKind::Commands {
                query: String::new(),
            },
            window,
            cx,
        );
    });
}

#[test]
fn snapshot_editor() {
    shot("editor-ayu-dark", |ws, vault, window, cx| {
        ws.open_file(format!("{vault}/editor-features.md"), None, window, cx);
    });
}

#[test]
fn snapshot_excalidraw_pane() {
    shot("excalidraw-pane-ayu-dark", |ws, vault, window, cx| {
        ws.open_file(
            format!("{vault}/desenhos/Desenho.excalidraw"),
            None,
            window,
            cx,
        );
    });
}
