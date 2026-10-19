//! Opt-in visual marker for telling the GPUI build apart from the Tauri one
//! (not part of the original app; off unless its env var is set):
//!
//! - `FUCHICO_BADGE=1`: a small "GPUI" chip at the bottom of the activity bar.
//!
//! The former `FUCHICO_ICON_THEME=<dir>` override is gone: Zed icon themes
//! are a real setting now (`crate::zed_themes`, Settings → Appearance).

use gpui_kit::{App, Global};

#[derive(Default)]
pub struct DevMarks {
    pub badge: bool,
}

impl Global for DevMarks {}

pub fn init(cx: &mut App) {
    let badge = std::env::var_os("FUCHICO_BADGE").is_some_and(|v| v != "0");
    cx.set_global(DevMarks { badge });
}

pub fn badge(cx: &App) -> bool {
    cx.try_global::<DevMarks>().is_some_and(|m| m.badge)
}
