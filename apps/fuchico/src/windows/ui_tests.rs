//! Headless tests for the app-wide window actions: the Edit menu's
//! forwarding and the zoom shortcuts.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClipboardItem, Context, Entity, FocusHandle, KeyDownEvent, TestAppContext, Window, div,
};

use crate::actions::*;
use crate::settings::SettingsStore;
use crate::test_ui::{self, Ui};

struct EditHost {
    input: Entity<InputState>,
    raw: FocusHandle,
    keys: Vec<String>,
}

impl EditHost {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        EditHost {
            input: cx.new(|cx| InputState::new(window, cx)),
            raw: cx.focus_handle(),
            keys: Vec::new(),
        }
    }
}

impl Render for EditHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(Input::new(&self.input))
            // A view that reads raw keys (like the drawing canvas).
            .child(div().track_focus(&self.raw).size_20().on_key_down(
                cx.listener(|this, ev: &KeyDownEvent, _, _| this.keys.push(ev.keystroke.unparse())),
            ))
    }
}

fn setup(cx: &mut TestAppContext) -> (Ui, Entity<EditHost>) {
    cx.update(|cx| {
        test_ui::init(cx);
        crate::keymap::bind_global_keys(cx);
        crate::editor::init(cx);
        super::register_global_actions(cx);
    });
    test_ui::open(cx, (600., 400.), EditHost::new)
}

fn value(ui: &mut Ui, host: &Entity<EditHost>) -> String {
    let host = host.clone();
    ui.cx
        .cx
        .update(|cx| host.read(cx).input.read(cx).value().to_string())
}

fn menu(ui: &mut Ui, action: impl gpui_kit::Action) {
    ui.cx.dispatch_action(action);
    ui.cx.run_until_parked();
    ui.frame();
}

#[gpui_kit::test]
async fn edit_menu_forwards_to_the_focused_input_or_keys(cx: &mut TestAppContext) {
    let (mut ui, host) = setup(cx);
    let input = ui.cx.cx.update(|cx| host.read(cx).input.clone());
    // The menu targets the key window (test platform only; nothing real).
    ui.cx.update(|window, _| window.activate_window());
    ui.cx
        .update(|window, cx| input.update(cx, |i, cx| i.focus(window, cx)));
    ui.frame();
    ui.type_text("hello");
    assert_eq!(value(&mut ui, &host), "hello");

    menu(&mut ui, MenuSelectAll);
    menu(&mut ui, MenuCut);
    assert_eq!(value(&mut ui, &host), "");
    assert_eq!(
        ui.cx
            .cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .as_deref(),
        Some("hello")
    );

    ui.cx
        .cx
        .write_to_clipboard(ClipboardItem::new_string("abc".into()));
    menu(&mut ui, MenuPaste);
    assert_eq!(value(&mut ui, &host), "abc");
    menu(&mut ui, MenuSelectAll);
    menu(&mut ui, MenuCopy);
    assert_eq!(
        ui.cx
            .cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .as_deref(),
        Some("abc")
    );

    menu(&mut ui, MenuUndo);
    assert_ne!(value(&mut ui, &host), "abc", "undo reverts the paste");
    menu(&mut ui, MenuRedo);
    assert_eq!(value(&mut ui, &host), "abc", "redo re-applies it");

    // The menu-only shortcut bindings don't shadow the input's own keys.
    ui.press("cmd-a backspace");
    assert_eq!(value(&mut ui, &host), "");

    // No editor or input focused: the keystroke goes to the focused view.
    let raw = ui.cx.cx.update(|cx| host.read(cx).raw.clone());
    ui.cx.update(|window, cx| window.focus(&raw, cx));
    ui.frame();
    menu(&mut ui, MenuUndo);
    menu(&mut ui, MenuSelectAll);
    let keys = ui.cx.cx.update(|cx| host.read(cx).keys.clone());
    assert_eq!(keys, ["cmd-z", "cmd-a"]);
}

#[gpui_kit::test]
async fn zoom_keys_follow_the_original(cx: &mut TestAppContext) {
    let (mut ui, _host) = setup(cx);
    let font = |ui: &mut Ui| {
        ui.cx
            .cx
            .update(|cx| SettingsStore::get(cx).editor_font_size)
    };
    let scale = |ui: &mut Ui| ui.cx.cx.update(|cx| SettingsStore::get(cx).ui_scale);
    let f0 = font(&mut ui);
    let s0 = scale(&mut ui);
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;

    // Mod = / - / 0: editor font size. Numpad- / Numpad0 report `-` / `0`.
    ui.press("cmd-=");
    assert_eq!(font(&mut ui), f0 + 1.);
    ui.press("cmd--");
    ui.press("cmd--");
    assert_eq!(font(&mut ui), f0 - 1.);
    ui.press("cmd-0");
    assert_eq!(font(&mut ui), f0);

    // Mod-Shift: UI scale. macOS reports Cmd-Shift-= as `cmd-+`.
    ui.press("cmd-+");
    assert!(near(scale(&mut ui), s0 + 0.1));
    ui.press("cmd-shift-=");
    assert!(near(scale(&mut ui), s0 + 0.2));
    ui.press("cmd-_");
    assert!(near(scale(&mut ui), s0 + 0.1));
    ui.press("cmd-)");
    assert!(near(scale(&mut ui), s0));

    // Ctrl works as Mod too (the original matched `ctrlKey || metaKey`).
    ui.press("ctrl-=");
    assert_eq!(font(&mut ui), f0 + 1.);
    ui.press("ctrl-0");
    assert_eq!(font(&mut ui), f0);
    ui.press("ctrl-+");
    assert!(near(scale(&mut ui), s0 + 0.1));
    ui.press("ctrl-shift-0");
    assert!(near(scale(&mut ui), s0));
    assert_eq!(font(&mut ui), f0);
}
