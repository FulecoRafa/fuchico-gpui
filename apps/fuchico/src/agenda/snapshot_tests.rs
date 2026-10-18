//! Offscreen snapshots of the main window with Agenda active (visual check
//! against the original's `docs/screenshots/agenda.png`). Writes PNGs to
//! `target/snapshots/`; no window is shown and nothing takes focus.

use chrono::NaiveDate;
use gpui_kit::{AppContext, px, size};

use super::model::TODAY_OVERRIDE;
use crate::snapshot::{SnapTheme, copy_vault, snapshot_with};
use crate::workspace::{MainViewKind, Workspace};

const DEMO_NOTES: &str = "/Users/fuleco/Documents/Dev/obsidian-helix/demo-notes";

fn agenda_snapshot(name: &str, theme: SnapTheme) {
    let src = std::path::Path::new(DEMO_NOTES);
    if !src.is_dir() {
        eprintln!("skipping {name}: {DEMO_NOTES} not found");
        return;
    }
    // The original screenshot was taken on 2026-07-20.
    TODAY_OVERRIDE.with(|t| t.set(NaiveDate::from_ymd_opt(2026, 7, 20)));
    let vault = copy_vault(src, name).to_string_lossy().to_string();
    let png = snapshot_with(
        name,
        size(px(1800.), px(1130.)),
        theme,
        |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        move |ws, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_root(Some(vault), window, cx);
                ws.set_view(MainViewKind::Agenda, window, cx);
            })
        },
    );
    eprintln!("snapshot: {}", png.display());
}

#[test]
fn snapshot_agenda_ayu_light() {
    agenda_snapshot("agenda-ayu-light", SnapTheme::AYU_LIGHT);
}

#[test]
fn snapshot_agenda_ayu_dark() {
    agenda_snapshot("agenda-ayu-dark", SnapTheme::AYU_DARK);
}
