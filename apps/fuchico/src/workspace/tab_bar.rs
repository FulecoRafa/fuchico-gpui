//! Tab bar (`.tab-bar`, only when at least one tab is open): a horizontally
//! scrolling strip of 32px tabs (max 200px) with dirty dot, ellipsized name,
//! close button, full-path tooltip, a 2px `--ring` bottom line on the active
//! tab and the Close / Close others / Close all / Open in new window menu.

use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{Context, SharedString, WeakEntity, div};

use super::Workspace;
use super::tabs::basename;
use crate::assets::AppIcon;
use crate::theme::AppTheme;
use crate::ui::{icon_button, u};

pub const TAB_HEIGHT: f32 = 32.;
pub const TAB_MAX_WIDTH: f32 = 200.;

impl Workspace {
    pub(super) fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let entity = cx.entity().downgrade();
        let count = self.tabs.len();
        let active = self.tabs.active.clone();
        let tabs: Vec<_> = self
            .tabs
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let path = tab.path.clone();
                let is_active = active.as_deref() == Some(path.as_str());
                let name: SharedString = basename(&path).to_string().into();
                let tip: SharedString = path.clone().into();
                let (e_click, e_close, e_menu) = (entity.clone(), entity.clone(), entity.clone());
                let (p_click, p_close, p_menu) = (path.clone(), path.clone(), path.clone());
                div()
                    .id(("tab", i))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(6.))
                    .h(u(TAB_HEIGHT))
                    .max_w(u(TAB_MAX_WIDTH))
                    .pl(u(10.))
                    .pr(u(8.))
                    .text_size(u(12.))
                    .font_family(th.ui_font.clone())
                    .border_r_1()
                    .border_color(th.colors.border)
                    .cursor_pointer()
                    .when(is_active, |d| {
                        d.bg(th.colors.background)
                            .text_color(th.colors.foreground)
                            .border_b_2()
                            .border_color(th.colors.ring)
                    })
                    .when(!is_active, |d| d.text_color(th.colors.muted_foreground).hover(|s| s.bg(th.fg_alpha(0.05))))
                    .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                    .on_click(move |_, w, cx| {
                        e_click.update(cx, |this, cx| this.activate_tab(&p_click, w, cx)).ok();
                    })
                    .when(tab.dirty, |d| {
                        d.child(div().flex_none().size(u(6.)).rounded_full().bg(th.colors.syntax_function))
                    })
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                    .child(icon_button(("tab-close", i), AppIcon::X, 18., 12., crate::t!(TabsClose), false, cx, {
                        move |_, w, cx| {
                            cx.stop_propagation();
                            e_close.update(cx, |this, cx| this.close_tab(&p_close, w, cx)).ok();
                        }
                    }))
                    .context_menu(move |menu, _, _| tab_menu(menu, e_menu.clone(), p_menu.clone(), count))
            })
            .collect();
        div()
            .id("tab-bar")
            .flex()
            .flex_none()
            .h(u(TAB_HEIGHT))
            .w_full()
            .overflow_x_scroll()
            .bg(th.panel_bg())
            .border_b_1()
            .border_color(th.colors.border)
            .children(tabs)
    }
}

fn tab_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    ws: WeakEntity<Workspace>,
    path: String,
    count: usize,
) -> gpui_kit::component::menu::PopupMenu {
    let item = |label: String, f: fn(&mut Workspace, &str, &mut gpui_kit::Window, &mut Context<Workspace>)| {
        let (ws, path) = (ws.clone(), path.clone());
        PopupMenuItem::new(label).on_click(move |_, w, cx| {
            ws.update(cx, |this, cx| f(this, &path, w, cx)).ok();
        })
    };
    menu.item(item(crate::t!(TabsClose), |t, p, w, cx| t.close_tab(p, w, cx)))
        .item(item(crate::t!(TabsCloseOthers), |t, p, w, cx| t.close_other_tabs(p, w, cx)).disabled(count < 2))
        .item(item(crate::t!(TabsCloseAll), |t, _, w, cx| t.close_all_tabs(w, cx)))
        .separator()
        .item(item(crate::t!(TabsOpenInNewWindow), |t, p, _, cx| {
            crate::windows::open_editor_window(p.to_string(), t.root().map(str::to_string), cx)
        }))
}
