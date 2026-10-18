//! Headless interaction tests for the overlays mounted in a real
//! [`Workspace`] through gpui's test harness: real key bindings, hit-testing
//! and focus inside a test window, no OS input or window focus.
//!
//! One test drives everything because `bus::init` binds a process-wide
//! sender to the first app that calls it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Modifiers, Point, TestAppContext, VisualTestContext,
    WindowBounds, WindowOptions, point, px, size,
};

use crate::actions::{EditorInsertDate, GoToLine};
use crate::settings::SettingsStore;
use crate::settings::ui_state::UiState;
use crate::workspace::Workspace;

fn temp_vault() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("fuchico-palette-test-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("project-launch.md"), "# Launch\n\nline 3\n").unwrap();
    std::fs::write(dir.join("notes/zeta.md"), "zeta\n").unwrap();
    std::fs::write(dir.join("templates/meeting.md"), "# {{title}}\n").unwrap();
    dir
}

struct Ui {
    cx: VisualTestContext,
    ws: Entity<Workspace>,
}

impl Ui {
    fn frame(&mut self) {
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
        self.frame();
    }

    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
        self.frame();
    }

    fn read<R>(&mut self, f: impl FnOnce(&Workspace, &App) -> R) -> R {
        let ws = self.ws.clone();
        self.cx.update(|_, cx| f(ws.read(cx), cx))
    }

    fn palette_titles(&mut self) -> Vec<String> {
        self.read(|ws, cx| match ws.palette.command_palette() {
            Some(p) => p.read(cx).results().iter().map(|r| r.command.title.clone()).collect(),
            None => vec![],
        })
    }

    fn center(&mut self, sel: &str) -> Point<gpui_kit::Pixels> {
        self.frame();
        self.cx.debug_bounds(sel.to_string().leak()).unwrap_or_else(|| panic!("no element {sel}")).center()
    }

    fn click_at(&mut self, at: Point<gpui_kit::Pixels>) {
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.frame();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
        self.frame();
    }

    async fn wait(&mut self, mut pred: impl FnMut(&Workspace, &App) -> bool) {
        for _ in 0..500 {
            self.frame();
            let ws = self.ws.clone();
            if self.cx.update(|_, cx| pred(ws.read(cx), cx)) {
                return;
            }
            self.cx.executor().timer(Duration::from_millis(10)).await;
        }
        panic!("condition not reached");
    }
}

#[gpui_kit::test]
async fn palette_and_switcher_end_to_end(cx: &mut TestAppContext) {
    let vault = temp_vault();
    // Outside the vault, so settings writes don't wake the vault watcher.
    let config = vault.with_extension("config");
    crate::persist::set_test_dir(config.clone());
    let root = vault.to_string_lossy().to_string();
    let launch = format!("{root}/project-launch.md");

    let dispatched: Rc<RefCell<Vec<String>>> = Rc::default();
    let rec = dispatched.clone();
    let rec2 = dispatched.clone();
    cx.update(|cx| {
        gpui_kit::init(cx);
        SettingsStore::init(SettingsStore::load(), cx);
        crate::i18n::set_language(crate::i18n::Language::En);
        cx.set_global(UiState::load());
        cx.set_global(crate::windows::geometry::WindowStates::load());
        crate::bus::init(cx);
        crate::theme::init(cx);
        crate::keymap::bind_global_keys(cx);
        // Editor-targeted actions bubble here when no pane handles them.
        cx.on_action(move |a: &GoToLine, _| rec.borrow_mut().push(format!("goto {}", a.line)));
        cx.on_action(move |_: &EditorInsertDate, _| rec2.borrow_mut().push("insert-date".into()));
    });
    let (handle, ws): (AnyWindowHandle, Entity<Workspace>) = cx.update(|cx| {
        let bounds = Bounds { origin: Point::default(), size: size(px(1200.), px(800.)) };
        gpui_kit::open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .expect("open test window")
    });
    let mut ui = Ui { cx: VisualTestContext::from_window(handle, cx), ws: ws.clone() };
    ui.cx.update(|window, cx| ws.update(cx, |ws, cx| ws.set_root(Some(root.clone()), window, cx)));
    ui.frame();

    // --- Quick Switcher: Mod-P, fuzzy, Enter opens --------------------------
    ui.press("cmd-p");
    assert!(ui.read(|ws, _| ws.palette.quick_switcher().is_some()));
    ui.wait(|ws, cx| ws.palette.quick_switcher().is_some_and(|s| s.read(cx).results().len() == 3)).await;
    // The panel sits 10% (of the width) below the top, 560 px wide, centred.
    ui.frame();
    let panel = ui.cx.debug_bounds("palette-overlay").expect("panel");
    assert_eq!(panel.size.width, px(560.));
    assert_eq!(panel.origin.x, px(320.));
    assert_eq!(panel.origin.y, px(120.));
    // Stretched by the flex row, capped at 60% of the backdrop's content box
    // ((800 - 120) * 0.6), as in CSS.
    assert_eq!(panel.size.height, px(408.));
    ui.type_text("plaun");
    let rel = ui.read(|ws, cx| ws.palette.quick_switcher().unwrap().read(cx).results()[0].rel.clone());
    assert_eq!(rel, "project-launch.md");
    ui.press("enter");
    assert!(ui.read(|ws, _| ws.palette.quick_switcher().is_none()));
    assert_eq!(ui.read(|ws, _| ws.tabs().active.clone()), Some(launch.clone()));

    // --- Escape and backdrop close ------------------------------------------
    ui.press("cmd-p");
    ui.press("escape");
    assert!(ui.read(|ws, _| !ws.palette.is_open()));
    ui.press("cmd-shift-p");
    assert!(ui.read(|ws, _| ws.palette.command_palette().is_some()));
    ui.click_at(point(px(20.), px(20.)));
    assert!(ui.read(|ws, _| !ws.palette.is_open()), "mousedown on the backdrop closes");
    // Ctrl works too on macOS.
    if crate::keymap::IS_MAC {
        ui.press("ctrl-shift-p");
        assert!(ui.read(|ws, _| ws.palette.command_palette().is_some()));
        ui.press("escape");
    }

    // --- Command Palette: registry, ranking, keys ---------------------------
    ui.press("cmd-shift-p");
    let titles = ui.palette_titles();
    assert_eq!(titles[0], "Go to Editor");
    assert!(titles.iter().any(|t| t == "Save File"), "active tab → save commands");
    // Template commands arrive with the background Markdown listing.
    ui.wait(|ws, cx| {
        ws.palette.command_palette().is_some_and(|p| {
            p.read(cx).results().iter().any(|r| r.command.title == "New Note from Template: meeting")
        })
    })
    .await;
    // Arrow keys move the active row; Enter runs it.
    ui.press("down");
    ui.press("down");
    assert_eq!(ui.read(|ws, cx| ws.palette.command_palette().unwrap().read(cx).active()), 2);
    ui.press("up");
    assert_eq!(ui.read(|ws, cx| ws.palette.command_palette().unwrap().read(cx).active()), 1);
    ui.type_text("agenda");
    assert_eq!(ui.read(|ws, cx| ws.palette.command_palette().unwrap().read(cx).active()), 0, "reset on change");
    assert_eq!(ui.palette_titles()[0], "Go to Tasks & Calendar");
    ui.press("enter");
    assert_eq!(ui.read(|ws, _| ws.main_view()), crate::workspace::MainViewKind::Agenda);

    // `:` go-to-line and an editor action reach the GPUI action handlers.
    ui.press("cmd-shift-p");
    assert!(ui.read(|ws, _| ws.palette.command_palette().is_some()), "palette opens from the agenda view");
    ui.type_text(":12");
    assert_eq!(ui.palette_titles()[0], "Go to Line 12");
    ui.press("enter");
    assert_eq!(ui.read(|ws, _| ws.main_view()), crate::workspace::MainViewKind::Agenda);
    ui.cx.update(|window, cx| ws.update(cx, |ws, cx| ws.set_view(crate::workspace::MainViewKind::Editor, window, cx)));
    ui.press("cmd-shift-p");
    ui.type_text("insert date");
    assert_eq!(ui.palette_titles()[0], "Editor: Insert date");
    // Click the row (hover makes it active first).
    let at = ui.center("palette-row-0");
    ui.click_at(at);
    assert!(ui.read(|ws, _| !ws.palette.is_open()));
    assert_eq!(*dispatched.borrow(), ["goto 12", "insert-date"]);

    // `:` aliases: `:q` closes the active tab.
    ui.press("cmd-shift-p");
    ui.type_text(":q");
    assert_eq!(ui.palette_titles()[0], "Close Active Tab");
    ui.press("enter");
    assert_eq!(ui.read(|ws, _| ws.tabs().len()), 0);

    // (Running a template command creates a file, which the vault watcher
    // reports from its own thread; the test scheduler forbids that, so the
    // creation itself is covered by fuchico-core's template tests.)

    // No match.
    ui.press("cmd-shift-p");
    ui.type_text("zzzzqqq");
    assert!(ui.palette_titles().is_empty());
    ui.press("escape");

    shortcuts_recorder(cx);
    let _ = std::fs::remove_dir_all(&vault);
    let _ = std::fs::remove_dir_all(&config);
}

/// The Keyboard Shortcuts list: search, record a binding, Escape cancels.
/// (Same app as above: it shares the `FUCHICO_CONFIG_DIR` override.)
fn shortcuts_recorder(cx: &mut TestAppContext) {
    let (handle, list): (AnyWindowHandle, Entity<super::ShortcutsList>) = cx.update(|cx| {
        let bounds = Bounds { origin: Point::default(), size: size(px(900.), px(1400.)) };
        gpui_kit::open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            |window, cx| cx.new(|cx| super::ShortcutsList::new(window, cx)),
        )
        .expect("open shortcuts window")
    });
    let mut cx = VisualTestContext::from_window(handle, cx);
    let frame = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
    };
    let click = |cx: &mut VisualTestContext, sel: &'static str| {
        frame(cx);
        let at = cx.debug_bounds(sel).unwrap_or_else(|| panic!("no {sel}")).center();
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        frame(cx);
    };
    let shortcut = |cx: &mut VisualTestContext| cx.update(|_, cx| SettingsStore::get(cx).shortcut("insertDate").to_string());
    assert_eq!(shortcut(&mut cx), "Mod-Shift-d");
    click(&mut cx, "shortcut-key-insertDate");
    assert_eq!(cx.update(|_, cx| list.read(cx).recording()), Some("insertDate"));
    // A bound key (Mod-P) is captured, not run.
    cx.simulate_keystrokes("cmd-shift-k");
    cx.run_until_parked();
    assert_eq!(shortcut(&mut cx), "Mod-Shift-k");
    assert_eq!(cx.update(|_, cx| list.read(cx).recording()), None);
    click(&mut cx, "shortcut-key-insertDate");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(cx.update(|_, cx| list.read(cx).recording()), None);
    assert_eq!(shortcut(&mut cx), "Mod-Shift-k");
    // Restore the default.
    cx.update(|_, cx| SettingsStore::set_shortcut(cx, "insertDate", "Mod-Shift-d"));
}
