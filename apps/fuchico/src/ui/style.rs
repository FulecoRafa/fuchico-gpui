//! Sizing helpers. App layout is specified in CSS px from the original and
//! expressed in rems (`px / 16`), so the UI-scale setting (window rem size =
//! `16px × uiScale`, see `theme::apply`) zooms everything uniformly.

use gpui_kit::{Rems, rems};

/// CSS px → rems at a 16px root: `u(13.)` is the original's `13px`.
pub fn u(css_px: f32) -> Rems {
    rems(css_px / 16.)
}
