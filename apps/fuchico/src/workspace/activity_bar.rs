//! Activity bar (`.app-activitybar`): 40px column, card background, five
//! 30×30 buttons with 17px icons; the current view gets the accent style.
//! Clicking the active view again does nothing (not a toggle).

use gpui_kit::prelude::*;
use gpui_kit::{Context, div};

use super::{MainViewKind, Workspace};
use crate::assets::AppIcon;
use crate::i18n::{Msg, tr};
use crate::theme::AppTheme;
use crate::ui::{icon_button, u};

pub const ACTIVITY_BAR_WIDTH: f32 = 40.;

/// Buttons in order: (view, icon, tooltip).
pub const ITEMS: [(MainViewKind, AppIcon, Msg); 5] = [
    (MainViewKind::Editor, AppIcon::Files, Msg::AppFiles),
    (
        MainViewKind::Agenda,
        AppIcon::CalendarClock,
        Msg::AppTasksCalendar,
    ),
    (MainViewKind::Search, AppIcon::Search, Msg::AppSearch),
    (MainViewKind::Tags, AppIcon::Hash, Msg::AppTags),
    (MainViewKind::Settings, AppIcon::Settings, Msg::AppSettings),
];

impl Workspace {
    pub(super) fn render_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx);
        let (bg, border) = (th.colors.card, th.colors.border);
        let current = self.view;
        let entity = cx.entity().downgrade();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .w(u(ACTIVITY_BAR_WIDTH))
            .h_full()
            .pt(u(8.))
            .gap(u(4.))
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .children(
                ITEMS
                    .into_iter()
                    .enumerate()
                    .map(|(i, (view, icon, label))| {
                        let entity = entity.clone();
                        icon_button(
                            ("activity", i),
                            icon,
                            30.,
                            17.,
                            tr(label, &[]),
                            view == current,
                            cx,
                            move |_, w, cx| {
                                entity
                                    .update(cx, |this, cx| this.set_view(view, w, cx))
                                    .ok();
                            },
                        )
                        .debug_selector(move || format!("activity:{i}"))
                    }),
            )
            .when(crate::dev_marks::badge(cx), |d| {
                d.child(div().flex_1()).child(
                    div()
                        .debug_selector(|| "gpui-badge".into())
                        .mb(u(8.))
                        .px(u(4.))
                        .py(u(2.))
                        .rounded(u(4.))
                        .bg(th.colors.primary)
                        .text_color(th.colors.primary_foreground)
                        .text_size(u(9.))
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .child("GPUI"),
                )
            })
    }
}
