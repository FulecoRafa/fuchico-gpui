//! Headless shell tests through gpui's test window: tab bar clicks, keys,
//! context menu and close; activity bar switching; dock resize drag; vault
//! restore; Open Folder cancel. No OS input, no window focus.

use gpui_kit::{Entity, Modifiers, MouseButton, TestAppContext, point, px};

use super::{MainViewKind, Workspace};
use crate::persist;
use crate::settings::ui_state::UiState;
use crate::test_ui::{self, Ui};

fn vault(tag: &str, files: &[&str]) -> (String, Vec<String>) {
    let dir = test_ui::temp_dir(tag);
    let root = dir.to_string_lossy().to_string();
    let paths = files
        .iter()
        .map(|f| {
            let p = format!("{root}/{f}");
            std::fs::write(&p, format!("# {f}\n")).unwrap();
            p
        })
        .collect();
    (root, paths)
}

fn shell(cx: &mut TestAppContext, tag: &str) -> (Ui, Entity<Workspace>) {
    persist::set_test_dir(test_ui::temp_dir(&format!("{tag}-cfg")));
    cx.update(test_ui::init_shell);
    test_ui::open_workspace(cx)
}

fn active(ui: &mut Ui, ws: &Entity<Workspace>) -> Option<String> {
    ui.cx.read(|cx| ws.read(cx).tabs().active.clone())
}

fn tab_paths(ui: &mut Ui, ws: &Entity<Workspace>) -> Vec<String> {
    ui.cx.read(|cx| {
        ws.read(cx)
            .tabs()
            .tabs
            .iter()
            .map(|t| t.path.clone())
            .collect()
    })
}

#[gpui_kit::test]
async fn tab_bar_click_keys_menu_and_close(cx: &mut TestAppContext) {
    let (mut ui, ws) = shell(cx, "ws-tabs");
    let (root, p) = vault("ws-tabs", &["a.md", "b.md", "c.md"]);
    let files = p.clone();
    ui.cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.set_root(Some(root), window, cx);
            for f in files {
                ws.open_file(f, None, window, cx);
            }
        })
    });
    ui.frame();
    assert_eq!(tab_paths(&mut ui, &ws), p);
    assert_eq!(active(&mut ui, &ws), Some(p[2].clone()));

    // Click.
    ui.click("tab-0");
    assert_eq!(active(&mut ui, &ws), Some(p[0].clone()));

    // Tabs are tab stops: focus one, Enter / Space activate it.
    let focus = ui.cx.read(|cx| ws.read(cx).tab_focus[&p[1]].clone());
    ui.cx.update(|window, cx| window.focus(&focus, cx));
    ui.press("enter");
    assert_eq!(active(&mut ui, &ws), Some(p[1].clone()));
    let focus = ui.cx.read(|cx| ws.read(cx).tab_focus[&p[2]].clone());
    ui.cx.update(|window, cx| window.focus(&focus, cx));
    ui.press("space");
    assert_eq!(active(&mut ui, &ws), Some(p[2].clone()));
    // Tab moves between tab stops.
    // (Activating hands focus to the mounted editor; back to the tab.)
    ui.cx.update(|window, cx| window.focus(&focus, cx));
    ui.press("shift-tab");
    let on_b = ui
        .cx
        .update(|window, cx| ws.read(cx).tab_focus[&p[1]].is_focused(window));
    assert!(on_b, "shift-tab focuses the previous tab");

    // Close button: closes that tab only.
    ui.click("tab-close-0");
    assert_eq!(tab_paths(&mut ui, &ws), p[1..].to_vec());

    // Context menu: right-click, the first item is "Close".
    let at = ui.bounds("tab-0").unwrap().center();
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    ui.frame();
    ui.cx
        .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    ui.cx
        .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    ui.cx.run_until_parked();
    ui.frame();
    ui.press("down enter");
    assert_eq!(tab_paths(&mut ui, &ws), vec![p[2].clone()]);
    // Focus handles follow the open tabs.
    let kept: Vec<String> = ui
        .cx
        .read(|cx| ws.read(cx).tab_focus.keys().cloned().collect());
    assert_eq!(kept, vec![p[2].clone()]);
}

#[gpui_kit::test]
async fn activity_bar_switches_views(cx: &mut TestAppContext) {
    let (mut ui, ws) = shell(cx, "ws-activity");
    let view = |ui: &mut Ui| ui.cx.read(|cx| ws.read(cx).main_view());
    assert_eq!(view(&mut ui), MainViewKind::Editor);
    for (i, want) in [
        (1, MainViewKind::Agenda),
        (2, MainViewKind::Search),
        (3, MainViewKind::Tags),
        (4, MainViewKind::Settings),
    ] {
        ui.click(&format!("activity:{i}"));
        assert_eq!(view(&mut ui), want);
    }
    // The active view again is not a toggle.
    ui.click("activity:4");
    assert_eq!(view(&mut ui), MainViewKind::Settings);
    ui.click("activity:0");
    assert_eq!(view(&mut ui), MainViewKind::Editor);
}

#[gpui_kit::test]
async fn dock_resize_drag_clamps(cx: &mut TestAppContext) {
    let (mut ui, ws) = shell(cx, "ws-dock");
    ui.cx
        .update(|window, cx| ws.update(cx, |ws, cx| ws.toggle_backlinks(window, cx)));
    ui.frame();
    let width = |ui: &mut Ui| ui.cx.read(|cx| ws.read(cx).dock_width);
    let w0 = width(&mut ui);
    let h = ui.bounds("dock-resizer").expect("dock handle").center();
    // Dragging left widens the dock.
    ui.drag(h, point(h.x - px(100.), h.y));
    assert_eq!(width(&mut ui), w0 + 100.);
    assert!(
        ui.cx.read(|cx| ws.read(cx).dock_drag.is_none()),
        "mouse up ends the drag"
    );
    // Clamped to the original's 240..800.
    let h = ui.bounds("dock-resizer").unwrap().center();
    ui.drag(h, point(h.x + px(1000.), h.y));
    assert_eq!(width(&mut ui), super::dock::DOCK_MIN_WIDTH);
}

#[gpui_kit::test]
async fn vault_restore_adopts_the_last_root(cx: &mut TestAppContext) {
    let (root, _) = vault("ws-restore", &["a.md"]);
    persist::set_test_dir(test_ui::temp_dir("ws-restore-cfg"));
    persist::save(persist::KEY_LAST_ROOT, &root);
    persist::flush();
    cx.update(test_ui::init_shell);
    let (mut ui, ws) = test_ui::open_workspace(cx);
    let r = root.clone();
    ui.wait("vault restored", move |cx| {
        ws.read(cx).root() == Some(r.as_str())
    })
    .await;
}

#[gpui_kit::test]
async fn vault_restore_forgets_a_missing_root(cx: &mut TestAppContext) {
    let missing = test_ui::temp_dir("ws-gone")
        .join("gone")
        .to_string_lossy()
        .to_string();
    persist::set_test_dir(test_ui::temp_dir("ws-gone-cfg"));
    persist::save(persist::KEY_LAST_ROOT, &missing);
    persist::flush();
    cx.update(test_ui::init_shell);
    assert_eq!(
        cx.read(|cx| UiState::get(cx).last_root.clone()),
        Some(missing)
    );
    let (mut ui, ws) = test_ui::open_workspace(cx);
    let w = ws.clone();
    ui.wait("restore settled", move |cx| !w.read(cx).restoring)
        .await;
    assert_eq!(
        ui.cx.read(|cx| ws.read(cx).root().map(str::to_string)),
        None
    );
    assert_eq!(ui.cx.read(|cx| UiState::get(cx).last_root.clone()), None);
    persist::flush();
    assert_eq!(persist::load::<String>(persist::KEY_LAST_ROOT), None);
}

#[gpui_kit::test]
async fn open_folder_cancel_keeps_the_vault(cx: &mut TestAppContext) {
    let (mut ui, ws) = shell(cx, "ws-open");
    let (root, p) = vault("ws-open", &["a.md"]);
    let (r, f) = (root.clone(), p[0].clone());
    ui.cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.set_root(Some(r), window, cx);
            ws.open_file(f, None, window, cx);
        })
    });
    ui.cx.dispatch_action(crate::actions::OpenFolder);
    ui.cx.run_until_parked();
    assert!(ui.cx.did_prompt_for_paths());
    ui.cx.simulate_path_prompt_response(|o| {
        assert!(o.directories && !o.files && !o.multiple);
        None
    });
    ui.cx.run_until_parked();
    ui.frame();
    assert_eq!(
        ui.cx.read(|cx| ws.read(cx).root().map(str::to_string)),
        Some(root.clone())
    );
    assert_eq!(tab_paths(&mut ui, &ws), p, "cancel keeps the open tabs");

    // Picking a folder adopts it (and closes the tabs).
    let (other, _) = vault("ws-open-other", &["z.md"]);
    let pick = std::path::PathBuf::from(&other);
    ui.cx.dispatch_action(crate::actions::OpenFolder);
    ui.cx.run_until_parked();
    ui.cx
        .simulate_path_prompt_response(move |_| Some(vec![pick]));
    ui.cx.run_until_parked();
    ui.frame();
    assert_eq!(
        ui.cx.read(|cx| ws.read(cx).root().map(str::to_string)),
        Some(other)
    );
    assert!(tab_paths(&mut ui, &ws).is_empty());
}
