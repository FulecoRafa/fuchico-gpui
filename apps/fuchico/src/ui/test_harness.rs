//! Headless UI-test helpers (tests only): app globals, a test window, and
//! real pointer / keyboard dispatch against elements observed with
//! `.test_support()` (gpui-base `test-support`). Mirrors the small subset of
//! `gpui_kit::test` the views need.

use gpui_base::test_support::{ElementSnapshot, find};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, InputEvent, ElementId, Entity, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, TestAppContext, Window, WindowBounds, WindowOptions, point,
    px, size,
};

use crate::settings::SettingsStore;

/// The globals the views read: component theme, settings, English, bus,
/// app theme.
pub fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        SettingsStore::init(SettingsStore::load(), cx);
        crate::i18n::set_locale(crate::i18n::Locale::En);
        crate::bus::init(cx);
        crate::theme::init(cx);
    });
}

/// Opens a 900×640 window whose content is built by `build`.
pub fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        let bounds = Bounds { origin: Point::default(), size: size(px(900.), px(640.)) };
        let opts = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        gpui_kit::open_window(opts, cx, build).expect("open test window")
    })
}

pub fn render(window: &mut Window, cx: &mut App) {
    window.refresh();
    window.draw(cx).clear(cx);
}

pub fn try_find(window: &Window, id: impl Into<ElementId>) -> Option<ElementSnapshot> {
    find(window, &[], &id.into())
}

fn center(window: &mut Window, id: ElementId, cx: &mut App) -> Point<Pixels> {
    render(window, cx);
    let snap = find(window, &[], &id).unwrap_or_else(|| panic!("missing element {id:?}"));
    assert!(snap.visible(), "element {id:?} not visible");
    snap.bounds().center()
}

pub fn click_button(window: &mut Window, id: impl Into<ElementId>, button: MouseButton, cx: &mut App) {
    let position = center(window, id.into(), cx);
    window.dispatch_event(
        MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() }.to_platform_input(),
        cx,
    );
    render(window, cx);
    window.dispatch_event(
        MouseDownEvent { button, position, modifiers: Default::default(), click_count: 1, first_mouse: false }
            .to_platform_input(),
        cx,
    );
    render(window, cx);
    window.dispatch_event(
        MouseUpEvent { button, position, modifiers: Default::default(), click_count: 1 }.to_platform_input(),
        cx,
    );
    render(window, cx);
}

pub fn click(window: &mut Window, id: impl Into<ElementId>, cx: &mut App) {
    click_button(window, id, MouseButton::Left, cx);
}

pub fn right_click(window: &mut Window, id: impl Into<ElementId>, cx: &mut App) {
    click_button(window, id, MouseButton::Right, cx);
}

/// Types `text` into the focused element, one keystroke per char.
pub fn type_text(window: &mut Window, text: &str, cx: &mut App) {
    for c in text.chars() {
        let s = c.to_string();
        let mut key = Keystroke::parse(&s).expect("char keystroke");
        key.key_char = Some(s);
        window.dispatch_keystroke(key, cx);
        render(window, cx);
    }
}

/// Polls (advancing the test clock 10ms per step) until `pred` holds.
pub async fn wait_for(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    what: &str,
    mut pred: impl FnMut(&mut Window, &mut App) -> bool,
) {
    for _ in 0..500 {
        let ok = cx
            .update_window(handle, |_, window, cx| {
                render(window, cx);
                pred(window, cx)
            })
            .expect("window open");
        if ok {
            return;
        }
        cx.executor().timer(std::time::Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

/// A point helper for offsets.
pub fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}
