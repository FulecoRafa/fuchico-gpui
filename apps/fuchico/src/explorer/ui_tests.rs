//! Headless interaction tests for the explorer inside a real [`Workspace`]
//! (gpui's test platform: real key dispatch, hit-testing, drag-and-drop and
//! focus in a test window; no OS window, input or focus).
//!
//! One test drives everything because `bus::init` binds a process-wide
//! sender to the first app that calls it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fuchico_core::events::CoreEvent;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, ExternalPaths, FileDropEvent, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, WindowBounds, WindowOptions,
    px, size,
};

use super::Explorer;
use crate::settings::SettingsStore;
use crate::settings::ui_state::UiState;
use crate::workspace::Workspace;

fn temp_vault() -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("fuchico-explorer-test-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    std::fs::write(dir.join("a/x.md"), "x\n").unwrap();
    for f in ["apple.md", "avocado.md", "beta.md", "zeta.md"] {
        std::fs::write(dir.join(f), "# note\n").unwrap();
    }
    dir.canonicalize().unwrap()
}

struct Ui {
    cx: VisualTestContext,
    ws: Entity<Workspace>,
    ex: Entity<Explorer>,
    root: String,
}

impl Ui {
    fn frame(&mut self) {
        self.cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    fn settle(&mut self) {
        self.cx.run_until_parked();
        self.frame();
    }

    fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.settle();
    }

    fn ex<R>(&mut self, f: impl FnOnce(&Explorer) -> R) -> R {
        let ex = self.ex.clone();
        self.cx.update(|_, cx| f(ex.read(cx)))
    }

    fn ws<R>(&mut self, f: impl FnOnce(&Workspace, &App) -> R) -> R {
        let ws = self.ws.clone();
        self.cx.update(|_, cx| f(ws.read(cx), cx))
    }

    fn p(&self, rel: &str) -> String {
        format!("{}/{rel}", self.root)
    }

    fn names(&mut self) -> Vec<String> {
        self.ex(|e| e.entries().iter().map(|p| super::tree::basename(p).to_string()).collect())
    }

    fn selected(&mut self) -> Vec<String> {
        self.ex(|e| e.sel.paths.iter().map(|p| super::tree::basename(p).to_string()).collect())
    }

    fn focused(&mut self) -> Option<String> {
        self.ex(|e| e.sel.focus.as_deref().map(|p| super::tree::basename(p).to_string()))
    }

    fn bounds(&mut self, sel: &str) -> Bounds<Pixels> {
        self.frame();
        self.cx.debug_bounds(sel.to_string().leak()).unwrap_or_else(|| panic!("no element {sel}"))
    }

    fn row(&mut self, name: &str) -> Point<Pixels> {
        self.bounds(&format!("ex-row:{name}")).center()
    }

    fn click(&mut self, at: Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_mouse_move(at, None, modifiers);
        self.frame();
        self.cx.simulate_click(at, modifiers);
        self.settle();
    }

    fn focus_explorer(&mut self) {
        let ex = self.ex.clone();
        self.cx.update(|window, cx| window.focus(&ex.read(cx).focus.clone(), cx));
        self.settle();
    }

    async fn wait(&mut self, what: &str, mut pred: impl FnMut(&mut Ui) -> bool) {
        for _ in 0..500 {
            self.settle();
            if pred(self) {
                return;
            }
            self.cx.executor().timer(Duration::from_millis(10)).await;
        }
        panic!("condition not reached: {what}");
    }
}

fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

#[gpui_kit::test]
async fn explorer_end_to_end(cx: &mut TestAppContext) {
    let vault = temp_vault();
    let config = vault.join(".config");
    // SAFETY: set before anything reads it (persist caches it once).
    unsafe { std::env::set_var("FUCHICO_CONFIG_DIR", &config) };
    let root = vault.to_string_lossy().to_string();
    cx.update(|cx| {
        gpui_kit::init(cx);
        SettingsStore::init(SettingsStore::load(), cx);
        crate::i18n::set_language(crate::i18n::Language::En);
        cx.set_global(UiState::load());
        cx.set_global(crate::windows::geometry::WindowStates::load());
        crate::bus::init(cx);
        crate::theme::init(cx);
        crate::keymap::bind_global_keys(cx);
        // The snapshot tests (same process, maybe in parallel) persist their
        // vault as the last root; don't let Workspace::new restore it.
        UiState::set_last_root(cx, None);
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
    let ex = cx.update(|cx| ws.read(cx).explorer().clone());
    let mut ui = Ui { cx: VisualTestContext::from_window(handle, cx), ws: ws.clone(), ex, root: root.clone() };
    ui.settle();
    ui.cx.update(|window, cx| ws.update(cx, |ws, cx| ws.set_root(Some(root.clone()), window, cx)));
    ui.wait("root listing", |ui| ui.names().len() == 6).await;
    assert_eq!(ui.names(), ["a", "b", "apple.md", "avocado.md", "beta.md", "zeta.md"], "folders first");

    // --- Click a folder: select + expand (lazy load), focus the tree ----------
    let at = ui.row("a");
    ui.click(at, Modifiers::none());
    ui.wait("a expanded", |ui| ui.names().contains(&"x.md".to_string())).await;
    assert_eq!(ui.selected(), ["a"]);
    let a = ui.p("a");
    assert!(ui.ex(|e| e.expanded.contains(&a)));

    // --- Keyboard: arrows, shift ranges, Mod-a ---------------------------------
    ui.press("down");
    assert_eq!(ui.selected(), ["x.md"]);
    ui.press("shift-down");
    ui.press("shift-down");
    assert_eq!(ui.selected(), ["x.md", "b", "apple.md"], "shift extends from the anchor");
    ui.press("up");
    assert_eq!(ui.selected(), ["b"], "plain arrow collapses to a single selection");
    ui.press("cmd-a");
    assert_eq!(ui.selected().len(), 7);
    // Left on a top-level entry does nothing; on x.md it moves the cursor only.
    ui.press("up");
    ui.press("up");
    assert_eq!(ui.focused().as_deref(), Some("a"));
    ui.press("left");
    assert!(!ui.names().contains(&"x.md".to_string()), "left collapses an expanded folder");
    ui.press("right");
    ui.wait("right expands", |ui| ui.names().contains(&"x.md".to_string())).await;
    ui.press("right");
    assert_eq!(ui.focused().as_deref(), Some("x.md"), "right on an expanded folder moves to the first child");
    ui.press("left");
    assert_eq!(ui.focused().as_deref(), Some("a"), "left on a file moves the cursor to the parent");
    assert_eq!(ui.selected(), ["x.md"], "quirk: the highlight stays");

    // --- Type-ahead ------------------------------------------------------------
    ui.press("a");
    assert_eq!(ui.selected(), ["apple.md"], "single letter searches after the cursor");
    ui.press("a");
    assert_eq!(ui.selected(), ["avocado.md"], "repeating the letter cycles");
    ui.press("z");
    // "aaz" is a mixed buffer with no match: selection unchanged.
    assert_eq!(ui.selected(), ["avocado.md"]);
    ui.cx.executor().advance_clock(Duration::from_millis(800));
    std::thread::sleep(Duration::from_millis(750));
    ui.press("z");
    assert_eq!(ui.selected(), ["zeta.md"], "buffer resets after 700ms");

    // --- Cut / paste ------------------------------------------------------------
    ui.press("cmd-x");
    assert_eq!(ui.ex(|e| e.cut.clone()), [ui.p("zeta.md")]);
    ui.press("escape");
    assert!(ui.ex(|e| e.cut.is_empty()), "escape clears the cut marking");
    ui.press("cmd-x");
    std::thread::sleep(Duration::from_millis(750));
    ui.press("b");
    assert_eq!(ui.selected(), ["b"]);
    ui.press("cmd-v");
    let moved = ui.p("b/zeta.md");
    ui.wait("paste moved zeta into b", |_| exists(&moved)).await;
    assert!(!exists(&ui.p("zeta.md")));
    ui.wait("zeta row gone", |ui| !ui.names().contains(&"zeta.md".to_string())).await;
    assert!(ui.ex(|e| e.cut.is_empty()));

    // --- Active file auto-select + rename (F2) with tab sync -------------------
    let beta = ui.p("beta.md");
    ui.cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_file(beta.clone(), None, window, cx)));
    ui.settle();
    assert_eq!(ui.selected(), ["beta.md"], "the active tab's file is auto-selected");
    ui.focus_explorer();
    ui.press("f2");
    assert!(ui.ex(|e| e.renaming.is_some() && e.input.is_some()));
    ui.bounds("ex-input");
    ui.type_text("gamma.md");
    ui.press("enter");
    let gamma = ui.p("gamma.md");
    ui.wait("renamed on disk", |_| exists(&gamma)).await;
    ui.wait("tab follows the rename", |ui| ui.ws(|ws, _| ws.tabs().active.clone()) == Some(gamma.clone())).await;
    assert!(ui.ex(|e| e.input.is_none() && e.renaming.is_none()));

    // --- Header: new file (no extension added), new drawing, escape cancels ---
    let at = ui.bounds("ex-new-file").center();
    ui.click(at, Modifiers::none());
    assert!(ui.ex(|e| e.pending.is_some()));
    ui.type_text("todo");
    ui.press("enter");
    let todo = ui.p("todo");
    ui.wait("new file created", |_| exists(&todo)).await;
    ui.wait("new file row", |ui| ui.names().contains(&"todo".to_string())).await;

    let at = ui.bounds("ex-new-drawing").center();
    ui.click(at, Modifiers::none());
    ui.type_text("sketch");
    ui.press("enter");
    let sketch = ui.p("sketch.excalidraw");
    ui.wait("drawing created", |_| exists(&sketch)).await;

    let at = ui.bounds("ex-new-folder").center();
    ui.click(at, Modifiers::none());
    ui.type_text("never");
    ui.press("escape");
    assert!(ui.ex(|e| e.pending.is_none() && e.input.is_none()), "escape cancels the create");
    ui.settle();
    assert!(!exists(&ui.p("never")));

    // Create inside a folder from the context-menu action: expands the parent.
    let b = ui.p("b");
    ui.cx.update(|window, cx| {
        ui.ex.update(cx, |e, cx| e.begin_create(b.clone(), super::PendingKind::Dir, None, None, window, cx))
    });
    ui.settle();
    ui.type_text("inner");
    ui.press("enter");
    let inner = ui.p("b/inner");
    ui.wait("folder created inside b", |_| Path::new(&inner).is_dir()).await;
    ui.wait("b expanded with inner", |ui| ui.names().contains(&"inner".to_string())).await;

    // --- Double-click starts the rename ----------------------------------------
    let at = ui.row("todo");
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    for count in [1, 2] {
        ui.cx.simulate_event(MouseDownEvent {
            position: at,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: count,
            first_mouse: false,
        });
        ui.cx.simulate_event(MouseUpEvent {
            position: at,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: count,
        });
        ui.settle();
    }
    assert_eq!(ui.ex(|e| e.renaming.clone()), Some(ui.p("todo")), "double-click renames");
    ui.press("escape");
    assert!(ui.ex(|e| e.renaming.is_none()));

    // --- Mouse multi-select + context menu labels ------------------------------
    ui.focus_explorer();
    let at = ui.row("apple.md");
    ui.click(at, Modifiers::secondary_key());
    let at = ui.row("avocado.md");
    ui.click(at, Modifiers::secondary_key());
    assert!(ui.selected().contains(&"apple.md".to_string()) && ui.selected().contains(&"avocado.md".to_string()));
    let at = ui.row("todo");
    ui.click(at, Modifiers::shift());
    let sel = ui.selected();
    assert!(sel.len() >= 2 && sel.contains(&"todo".to_string()), "shift-click range: {sel:?}");
    // Right-click inside the selection keeps it; menu targets the whole selection.
    let apple = ui.p("apple.md");
    let avocado = ui.p("avocado.md");
    let at = ui.row("avocado.md");
    ui.click(at, Modifiers::none());
    ui.cx.update(|_, cx| ui.ex.update(cx, |e, cx| {
        e.sel.select(&apple, false, true, &e.entries());
        cx.notify()
    }));
    ui.settle();
    assert_eq!(ui.selected(), ["avocado.md", "apple.md"]);
    let entry = ui.ex(|e| e.entry_at(&avocado).unwrap().1.clone());
    let labels: Vec<String> = ui.cx.update(|_, cx| {
        ui.ex.update(cx, |e, cx| e.row_menu(&entry, cx)).into_iter().map(menu_label).collect()
    });
    assert_eq!(
        labels,
        ["Open in new window", "Rename", "Cut 2 items", "-", "Reveal in file manager", "Open with default app", "Copy 2 paths", "-", "Delete 2 items"]
    );
    let folder = ui.ex(|e| e.entry_at(&b).unwrap().1.clone());
    ui.cx.update(|_, cx| ui.ex.update(cx, |e, _| e.cut = vec![apple.clone()]));
    let labels: Vec<String> =
        ui.cx.update(|_, cx| ui.ex.update(cx, |e, cx| e.row_menu(&folder, cx)).into_iter().map(menu_label).collect());
    assert_eq!(labels[..5], ["New file", "New folder", "New drawing", "-", "Rename"]);
    assert!(labels.contains(&"Paste apple.md here".to_string()));
    ui.cx.update(|_, cx| ui.ex.update(cx, |e, _| e.cut.clear()));

    // --- Drag-and-drop move, spring-loaded folder ------------------------------
    // Collapse b first, then drag apple.md onto it and hover past 600ms.
    let at = ui.row("b");
    ui.click(at, Modifiers::none());
    assert!(!ui.names().contains(&"inner".to_string()));
    let from = ui.row("apple.md");
    let to = ui.row("b");
    ui.cx.simulate_mouse_move(from, None, Modifiers::none());
    ui.cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    ui.cx.simulate_mouse_move(from + Point::new(px(0.), px(10.)), MouseButton::Left, Modifiers::none());
    ui.settle();
    ui.cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    ui.settle();
    assert_eq!(ui.ex(|e| e.drop_target.clone()), Some(b.clone()), "folder row is the drop target");
    ui.cx.executor().advance_clock(Duration::from_millis(super::tree::DROP_EXPAND_MS + 50));
    ui.settle();
    assert!(ui.ex(|e| e.expanded.contains(&b)), "spring-loaded folder opened");
    let to = ui.row("b");
    ui.cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    ui.cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    ui.settle();
    let moved = ui.p("b/apple.md");
    ui.wait("drag moved apple into b", |_| exists(&moved)).await;
    assert!(ui.ex(|e| e.drop_target.is_none()));

    // Dragging onto the header moves to the root.
    let from = ui.row("apple.md");
    let header = ui.bounds("ex-header").center();
    ui.cx.simulate_mouse_move(from, None, Modifiers::none());
    ui.cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    ui.cx.simulate_mouse_move(from + Point::new(px(0.), px(10.)), MouseButton::Left, Modifiers::none());
    ui.settle();
    ui.cx.simulate_mouse_move(header, MouseButton::Left, Modifiers::none());
    ui.settle();
    assert_eq!(ui.ex(|e| e.drop_target.clone()), Some(root.clone()), "header targets the root");
    ui.cx.simulate_mouse_up(header, MouseButton::Left, Modifiers::none());
    ui.settle();
    let back = ui.p("apple.md");
    ui.wait("drag to header moved apple to the root", |_| exists(&back)).await;

    // --- Finder import (ExternalPaths) onto a folder row ------------------------
    let outside = std::env::temp_dir().join(format!("fuchico-ex-import-{}.png", std::process::id()));
    std::fs::write(&outside, b"png").unwrap();
    let target = ui.row("a");
    ui.cx.simulate_event(FileDropEvent::Entered { position: target, paths: ExternalPaths(vec![outside.clone()].into()) });
    ui.cx.simulate_event(FileDropEvent::Pending { position: target });
    ui.settle();
    assert_eq!(ui.ex(|e| e.drop_target.clone()), Some(ui.p("a")));
    ui.cx.simulate_event(FileDropEvent::Submit { position: target });
    ui.settle();
    let name = outside.file_name().unwrap().to_string_lossy().to_string();
    let imported = ui.p(&format!("a/{name}"));
    ui.wait("imported into a", |_| exists(&imported)).await;
    ui.wait("notice", |ui| ui.ex(|e| e.notice.clone()).is_some()).await;
    assert_eq!(ui.ex(|e| e.notice.clone()), Some(("Imported 1 file into a".to_string(), false)));
    assert!(exists(outside.to_str().unwrap()), "import copies, the source stays");
    ui.cx.executor().advance_clock(Duration::from_millis(super::tree::NOTICE_MS + 10));
    ui.settle();
    assert!(ui.ex(|e| e.notice.is_none()), "notice clears after 4s");
    std::fs::remove_file(&outside).ok();

    // --- fs:changed from the bus refreshes a loaded folder --------------------
    std::fs::write(vault.join("external.md"), "x").unwrap();
    ui.cx.update(|_, cx| crate::bus::emit(cx, crate::bus::AppEvent::Core(CoreEvent::FsChanged { dir: root.clone(), from: None })));
    ui.wait("external file appears", |ui| ui.names().contains(&"external.md".to_string())).await;

    // --- Delete with confirmation (Backspace → dialog → OK) --------------------
    ui.focus_explorer();
    std::thread::sleep(Duration::from_millis(750));
    ui.press("e");
    assert_eq!(ui.selected(), ["external.md"]);
    ui.press("backspace");
    ui.cx.update(|window, cx| {
        use gpui_kit::component::WindowExt;
        assert!(window.has_active_dialog(cx), "delete asks first");
    });
    ui.press("enter");
    let ext = ui.p("external.md");
    ui.wait("deleted after confirming", |_| !exists(&ext)).await;

    std::fs::remove_dir_all(&vault).ok();
}

fn menu_label(e: crate::ui::MenuEntry) -> String {
    match e {
        crate::ui::MenuEntry::Item { label, .. } => label.to_string(),
        crate::ui::MenuEntry::Separator => "-".into(),
    }
}
