//! Drag payload for vault paths (`PATH_DRAG_TYPE`,
//! `application/x-fuchico-paths`): explorer rows start the drag; the
//! explorer itself (move) and the editor area (insert links) accept it.

use gpui_kit::prelude::*;
use gpui_kit::{Context, Pixels, Point, SharedString, Window, div, px};

use crate::theme::AppTheme;
use crate::ui::u;

/// Dragged absolute vault paths (the row's targets, in selection order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraggedPaths(pub Vec<String>);

/// The drag image: a small accent pill with the basename or "{count} items"
/// (`.tree-drag-ghost`).
pub struct DragGhost {
    pub label: SharedString,
    /// Cursor position inside the dragged row (GPUI paints the drag view
    /// at `cursor - offset`).
    pub offset: Point<Pixels>,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        // Cursor at (8, 12) inside the pill, like `setDragImage(ghost, 8, 12)`.
        let left = (f32::from(self.offset.x) - 8.).max(0.);
        let top = (f32::from(self.offset.y) - 12.).max(0.);
        div().pl(px(left)).pt(px(top)).child(
            div()
                .px(u(8.))
                .py(u(2.))
                .rounded(u(4.))
                .bg(th.colors.accent)
                .text_color(th.colors.accent_foreground)
                .font_family(th.ui_font.clone())
                .text_size(u(12.))
                .whitespace_nowrap()
                .child(self.label.clone()),
        )
    }
}
