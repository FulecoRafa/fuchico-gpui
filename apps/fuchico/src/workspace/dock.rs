//! Right dock (spec 01-shell "Right dock"): `{Mermaid | Backlinks}`, 380px
//! default, resizable 240–800 with a 5px left-edge handle, width kept in
//! memory only.

use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, Entity, FontWeight, SharedString, Window, div};

use crate::backlinks::BacklinksPane;
use crate::mermaid::MermaidPane;
use crate::theme::AppTheme;
use crate::ui::{icon_button, u};

pub const DOCK_DEFAULT_WIDTH: f32 = 380.;
pub const DOCK_MIN_WIDTH: f32 = 240.;
pub const DOCK_MAX_WIDTH: f32 = 800.;
pub const DOCK_HANDLE_WIDTH: f32 = 5.;

/// What the dock shows.
pub enum Dock {
    Mermaid { block_key: String, pane: Entity<MermaidPane> },
    Backlinks(Entity<BacklinksPane>),
}

impl Dock {
    pub fn view(&self) -> gpui_kit::AnyView {
        match self {
            Dock::Mermaid { pane, .. } => pane.clone().into(),
            Dock::Backlinks(p) => p.clone().into(),
        }
    }
}

/// An in-progress resize: pointer x and width at mouse-down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockDrag {
    pub start_x: f32,
    pub start_width: f32,
}

impl DockDrag {
    /// `clamp(startWidth - (x - startX), 240, 800)` — dragging left widens.
    pub fn width_at(&self, x: f32) -> f32 {
        (self.start_width - (x - self.start_x)).clamp(DOCK_MIN_WIDTH, DOCK_MAX_WIDTH)
    }
}

/// The backlinks target for the active tab: none for drawings
/// (`.excalidraw`, case-sensitive as in the original).
pub fn backlinks_target(active: Option<&str>) -> Option<String> {
    active.filter(|p| !p.ends_with(".excalidraw")).map(str::to_string)
}

/// Shared panel chrome: a header with the title and a close button above
/// a padded `body`.
pub fn dock_frame(
    id: &'static str,
    title: impl Into<SharedString>,
    body: impl IntoElement,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .h(u(32.))
        .px(u(10.))
        .border_b_1()
        .border_color(th.colors.border)
        .child(div().text_size(u(12.)).font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(icon_button(
            SharedString::from(format!("{id}-close")),
            crate::assets::AppIcon::X,
            22.,
            14.,
            crate::t!(CommonClose),
            false,
            cx,
            on_close,
        ));
    div()
        .flex()
        .flex_col()
        .size_full()
        .text_size(u(13.))
        .text_color(th.colors.foreground)
        .child(header)
        .child(div().id(id).flex_1().overflow_y_scroll().p(u(12.)).text_color(th.colors.muted_foreground).child(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_clamps_and_inverts() {
        let d = DockDrag { start_x: 600., start_width: 380. };
        assert_eq!(d.width_at(500.), 480.);
        assert_eq!(d.width_at(700.), 280.);
        assert_eq!(d.width_at(0.), DOCK_MAX_WIDTH);
        assert_eq!(d.width_at(1000.), DOCK_MIN_WIDTH);
    }

    #[test]
    fn backlinks_skip_drawings_case_sensitively() {
        assert_eq!(backlinks_target(Some("/v/a.md")), Some("/v/a.md".into()));
        assert_eq!(backlinks_target(Some("/v/a.excalidraw")), None);
        assert_eq!(backlinks_target(Some("/v/a.Excalidraw")), Some("/v/a.Excalidraw".into()));
        assert_eq!(backlinks_target(None), None);
    }
}
