//! Window geometry remembered per window label (`main`, `editor-*`,
//! `mermaid-*`), replacing `tauri_plugin_window_state`. Stored as one JSON
//! object under `helix.windowState`; writes are debounced.

use std::collections::BTreeMap;
use std::time::Duration;

use gpui_kit::{App, Bounds, Context, Global, Subscription, Task, Window, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

use crate::persist;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
/// Smallest size we restore (guards against a corrupt entry).
const MIN_RESTORE: f32 = 200.;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl Geometry {
    pub fn from_bounds(b: WindowBounds) -> Geometry {
        let (maximized, r) = match b {
            WindowBounds::Windowed(r) => (false, r),
            WindowBounds::Maximized(r) | WindowBounds::Fullscreen(r) => (true, r),
        };
        Geometry {
            x: f32::from(r.origin.x),
            y: f32::from(r.origin.y),
            width: f32::from(r.size.width),
            height: f32::from(r.size.height),
            maximized,
        }
    }

    pub fn to_bounds(self) -> WindowBounds {
        let r = Bounds { origin: point(px(self.x), px(self.y)), size: size(px(self.width), px(self.height)) };
        if self.maximized { WindowBounds::Maximized(r) } else { WindowBounds::Windowed(r) }
    }

    fn is_sane(&self) -> bool {
        self.width >= MIN_RESTORE && self.height >= MIN_RESTORE && self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Default)]
pub struct WindowStates {
    map: BTreeMap<String, Geometry>,
    save: Option<Task<()>>,
}

impl Global for WindowStates {}

impl WindowStates {
    /// Startup read.
    pub fn load() -> WindowStates {
        WindowStates { map: persist::load(persist::KEY_WINDOW_STATE).unwrap_or_default(), save: None }
    }
}

/// Bounds for a new window: the remembered ones, else `default` centered.
pub fn initial_bounds(label: &str, default: (f32, f32), cx: &App) -> WindowBounds {
    match cx.try_global::<WindowStates>().and_then(|s| s.map.get(label)) {
        Some(g) if g.is_sane() => g.to_bounds(),
        _ => WindowBounds::Windowed(Bounds::centered(None, size(px(default.0), px(default.1)), cx)),
    }
}

/// Records `window`'s bounds under `label` whenever they change.
pub fn track<V: 'static>(label: String, window: &mut Window, cx: &mut Context<V>) -> Subscription {
    cx.observe_window_bounds(window, move |_, window, cx| record(&label, window.window_bounds(), cx))
}

pub fn record(label: &str, bounds: WindowBounds, cx: &mut App) {
    if cx.try_global::<WindowStates>().is_none() {
        cx.set_global(WindowStates::default());
    }
    let g = Geometry::from_bounds(bounds);
    let states = cx.global_mut::<WindowStates>();
    if states.map.get(label) == Some(&g) {
        return;
    }
    states.map.insert(label.to_string(), g);
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DEBOUNCE).await;
        cx.update(|cx| persist::save(persist::KEY_WINDOW_STATE, &cx.global::<WindowStates>().map));
    });
    cx.global_mut::<WindowStates>().save = Some(task);
}

/// Writes pending geometry now (quit).
pub fn save_now(cx: &App) {
    if let Some(s) = cx.try_global::<WindowStates>() {
        persist::save(persist::KEY_WINDOW_STATE, &s.map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips_and_rejects_tiny() {
        let g = Geometry { x: 10., y: 20., width: 900., height: 650., maximized: false };
        assert_eq!(Geometry::from_bounds(g.to_bounds()), g);
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Geometry>(&json).unwrap(), g);
        assert!(!Geometry { width: 10., ..g }.is_sane());
    }
}
