//! Test-only helpers for headless UI tests (`#[gpui_kit::test]`): app
//! setup, element lookup by `debug_selector`, real pointer / key dispatch
//! through hit-testing and a bus recorder. Offscreen PNG snapshots live in
//! [`crate::snapshot`].

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, IntoElement, Modifiers, MouseButton,
    Pixels, Point, Render, Subscription, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowOptions, div, px, size,
};

use crate::bus::{self, AppEvent};

/// Globals every view expects: gpui-kit, fonts, the default theme, the bus.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::assets::register_fonts(cx);
    crate::settings::SettingsStore::init(
        crate::settings::SettingsStore::with(Default::default()),
        cx,
    );
    // The default `System` language follows the OS; tests assert English.
    crate::i18n::set_locale(crate::i18n::Locale::En);
    crate::theme::apply::apply(
        crate::theme::resolve(&crate::settings::Settings::default(), true),
        cx,
    );
    bus::init(cx);
}

/// The main window's globals, like `main` (persisted stores load from this
/// test thread's scratch config dir), English, and the global key bindings.
pub fn init_shell(cx: &mut App) {
    gpui_kit::init(cx);
    crate::assets::register_fonts(cx);
    crate::settings::SettingsStore::init(crate::settings::SettingsStore::load(), cx);
    crate::i18n::set_locale(crate::i18n::Locale::En);
    cx.set_global(crate::settings::ui_state::UiState::load());
    cx.set_global(crate::windows::geometry::WindowStates::load());
    bus::init(cx);
    crate::theme::init(cx);
    crate::keymap::bind_global_keys(cx);
    crate::editor::init(cx);
}

/// Opens a 1200×800 test window holding a fresh [`crate::workspace::Workspace`].
pub fn open_workspace(cx: &mut TestAppContext) -> (Ui, Entity<crate::workspace::Workspace>) {
    open(cx, (1200., 800.), crate::workspace::Workspace::new)
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("fuchico-{tag}-test-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Opens a test window rooted at `build`'s view.
pub fn open<V: Render + 'static>(
    cx: &mut TestAppContext,
    (w, h): (f32, f32),
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> (Ui, Entity<V>) {
    let (handle, view): (AnyWindowHandle, Entity<V>) = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(w), px(h)),
        };
        let (h, v) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| build(window, cx)),
        )
        .expect("open test window");
        (h, v)
    });
    (Ui::new(handle, cx), view)
}

pub struct Ui {
    pub cx: VisualTestContext,
    pub win: AnyWindowHandle,
}

impl Ui {
    pub fn new(win: AnyWindowHandle, cx: &TestAppContext) -> Ui {
        Ui {
            cx: VisualTestContext::from_window(win, cx),
            win,
        }
    }

    /// Draws a frame (a no-op once the window is gone).
    pub fn frame(&mut self) {
        let _ = self.cx.cx.update_window(self.win, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    pub fn bounds(&mut self, sel: &str) -> Option<Bounds<Pixels>> {
        self.frame();
        // `debug_bounds` wants `&'static str`; leaking a few test strings is fine.
        self.cx.debug_bounds(sel.to_string().leak())
    }

    pub fn has(&mut self, sel: &str) -> bool {
        self.bounds(sel).is_some()
    }

    pub fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.frame();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
        self.frame();
    }

    pub fn click(&mut self, sel: &str) {
        let at = self
            .bounds(sel)
            .unwrap_or_else(|| panic!("no element {sel}"))
            .center();
        self.click_at(at);
    }

    /// Left-button drag from `a` to `b` in a few steps.
    pub fn drag(&mut self, a: Point<Pixels>, b: Point<Pixels>) {
        self.cx.simulate_mouse_move(a, None, Modifiers::none());
        self.frame();
        self.cx
            .simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
        for i in 1..=4 {
            let t = i as f32 / 4.;
            let p = Point {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
            };
            self.cx
                .simulate_mouse_move(p, MouseButton::Left, Modifiers::none());
            self.frame();
        }
        self.cx
            .simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
        self.frame();
    }

    /// Types `text` into the focused element.
    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
        self.frame();
    }

    pub fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
        self.frame();
    }

    /// Advances test time by `ms`, running whatever becomes due.
    pub async fn sleep(&mut self, ms: u64) {
        self.cx.executor().timer(Duration::from_millis(ms)).await;
        self.cx.run_until_parked();
        self.frame();
    }

    /// Polls (10 ms of test time per step) until `pred` holds.
    pub async fn wait(&mut self, what: &str, mut pred: impl FnMut(&mut App) -> bool) {
        for _ in 0..500 {
            self.frame();
            if self.cx.cx.update(|cx| pred(cx)) {
                return;
            }
            self.cx.executor().timer(Duration::from_millis(10)).await;
        }
        panic!("condition not reached: {what}");
    }
}

/// Records bus events matching a filter.
pub struct BusRecorder {
    pub seen: Vec<AppEvent>,
    _sub: Subscription,
}

impl Render for BusRecorder {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub fn record(
    cx: &mut TestAppContext,
    keep: impl Fn(&AppEvent) -> bool + 'static,
) -> Entity<BusRecorder> {
    cx.update(|cx| {
        cx.new(|cx| BusRecorder {
            seen: Vec::new(),
            _sub: bus::subscribe(cx, move |this: &mut BusRecorder, ev, _| {
                if keep(ev) {
                    this.seen.push(ev.clone());
                }
            }),
        })
    })
}
