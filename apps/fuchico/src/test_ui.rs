//! Test-only helpers for headless UI tests (`#[gpui_kit::test]`): app
//! setup, element lookup by `debug_selector`, real pointer / key dispatch
//! through hit-testing, a bus recorder, and offscreen PNG snapshots
//! (`HeadlessAppContext` + the Metal headless renderer — no NSWindow ever
//! appears, nothing takes focus).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, IntoElement, Modifiers, MouseButton, Pixels, Point,
    Render, Subscription, TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions, div, px, size,
};

use crate::bus::{self, AppEvent};

/// Globals every view expects: gpui-kit, fonts, the default theme, the bus.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::assets::register_fonts(cx);
    crate::settings::SettingsStore::init(crate::settings::SettingsStore::with(Default::default()), cx);
    crate::theme::apply::apply(crate::theme::resolve(&crate::settings::Settings::default(), true), cx);
    bus::init(cx);
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("fuchico-{tag}-test-{}-{nanos}", std::process::id()));
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
        let bounds = Bounds { origin: Point::default(), size: size(px(w), px(h)) };
        let (h, v) = gpui_kit::open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
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
        Ui { cx: VisualTestContext::from_window(win, cx), win }
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
        let at = self.bounds(sel).unwrap_or_else(|| panic!("no element {sel}")).center();
        self.click_at(at);
    }

    /// Left-button drag from `a` to `b` in a few steps.
    pub fn drag(&mut self, a: Point<Pixels>, b: Point<Pixels>) {
        self.cx.simulate_mouse_move(a, None, Modifiers::none());
        self.frame();
        self.cx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
        for i in 1..=4 {
            let t = i as f32 / 4.;
            let p = Point { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
            self.cx.simulate_mouse_move(p, MouseButton::Left, Modifiers::none());
            self.frame();
        }
        self.cx.simulate_mouse_up(b, MouseButton::Left, Modifiers::none());
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

pub fn record(cx: &mut TestAppContext, keep: impl Fn(&AppEvent) -> bool + 'static) -> Entity<BusRecorder> {
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

/// Renders `build`'s view offscreen and writes `<FUCHICO_SNAPSHOT_DIR>/<name>.png`.
/// A no-op unless `FUCHICO_SNAPSHOT_DIR` is set (snapshots are for humans).
#[cfg(target_os = "macos")]
pub fn snapshot<V: Render + 'static>(
    name: &str,
    (w, h): (f32, f32),
    dark: bool,
    settle: impl Fn(&mut gpui_kit::HeadlessAppContext),
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    let Some(dir) = std::env::var_os("FUCHICO_SNAPSHOT_DIR").map(PathBuf::from) else { return };
    let mut cx = gpui_kit::HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(crate::assets::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::assets::register_fonts(cx);
        let mut s = crate::settings::Settings::default();
        s.mode = if dark { crate::settings::ModePref::Dark } else { crate::settings::ModePref::Light };
        crate::settings::SettingsStore::init(crate::settings::SettingsStore::with(s.clone()), cx);
        crate::theme::apply::apply(crate::theme::resolve(&s, dark), cx);
        bus::init(cx);
    });
    let handle = cx
        .open_window(size(px(w), px(h)), |window, cx| {
            let th = crate::theme::AppTheme::get(cx).clone();
            let inner = cx.new(|cx| build(window, cx));
            cx.new(|_| Backdrop { inner: inner.into(), bg: th.colors.background, fg: th.colors.foreground })
        })
        .expect("headless window");
    cx.run_until_parked();
    settle(&mut cx);
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, _| window.refresh()).ok();
    }
    let img = cx.capture_screenshot(handle.into()).expect("capture");
    std::fs::create_dir_all(&dir).unwrap();
    img.save(dir.join(format!("{name}.png"))).expect("save png");
}

#[cfg(not(target_os = "macos"))]
pub fn snapshot<V: Render + 'static>(
    _: &str,
    _: (f32, f32),
    _: bool,
    _: impl Fn(&mut gpui_kit::HeadlessAppContext),
    _: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
}

struct Backdrop {
    inner: gpui_kit::AnyView,
    bg: gpui_kit::Hsla,
    fg: gpui_kit::Hsla,
}

impl Render for Backdrop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::prelude::*;
        div().size_full().flex().bg(self.bg).text_color(self.fg).child(self.inner.clone())
    }
}
