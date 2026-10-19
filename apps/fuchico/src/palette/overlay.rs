//! Overlay chrome shared by the palette and the switcher (spec 4 §2,
//! `globals.css` `.palette-overlay*`), plus the match highlighter
//! (`HighlightedText`).

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, Div, Entity, HighlightStyle, MouseButton, MouseDownEvent,
    ScrollHandle, SharedString, Stateful, StyledText, Window, div, hsla, point, px, relative,
};

use crate::theme::{AppTheme, alpha};
use crate::ui::u;

/// `.palette-overlay-backdrop` > `.palette-overlay` with the input and the
/// scrolling list. `on_backdrop` runs on a mousedown outside the panel.
pub fn frame(
    id: &'static str,
    input: &Entity<InputState>,
    list: Vec<AnyElement>,
    scroll: &ScrollHandle,
    on_backdrop: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let th = AppTheme::get(cx);
    div()
        .id(id)
        .absolute()
        .inset_0()
        .flex()
        .justify_center()
        .pt(relative(0.1))
        .bg(hsla(0., 0., 0., 0.25))
        .occlude()
        .on_mouse_down(MouseButton::Left, on_backdrop)
        .child(
            div()
                .id("palette-overlay")
                .debug_selector(|| "palette-overlay".into())
                .flex()
                .flex_col()
                .w(relative(0.9))
                .max_w(u(560.))
                .max_h(relative(0.6))
                .overflow_hidden()
                .bg(th.colors.card)
                .text_color(th.colors.card_foreground)
                .border_1()
                .border_color(th.colors.border)
                .rounded(u(th.radius.md))
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.3),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(32.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex_none()
                        .border_b_1()
                        .border_color(th.colors.border)
                        .text_color(th.colors.foreground)
                        .child(
                            Input::new(input)
                                .appearance(false)
                                .px(u(12.))
                                .py(u(10.))
                                .h(u(36.))
                                .text_size(u(13.)),
                        ),
                )
                .child(
                    div()
                        .id("palette-overlay-list")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .p(u(4.))
                        .children(list),
                ),
        )
}

/// `.palette-overlay-empty`.
pub fn empty(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let th = AppTheme::get(cx);
    div()
        .p(u(16.))
        .flex()
        .justify_center()
        .text_size(u(12.))
        .text_color(th.colors.muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// `.palette-overlay-item` (with `-active`): icon + highlighted text. The
/// caller appends chips and wires hover/click.
pub fn item(
    id: impl Into<gpui_kit::ElementId>,
    icon: impl Into<gpui_kit::component::Icon>,
    text: &str,
    indices: &[usize],
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    let icon = gpui_kit::component::Icon::new(icon)
        .size(u(13.))
        .into_any_element();
    item_with(id, icon, text, indices, active, cx)
}

/// [`item`] with any leading element (e.g. a themed file icon).
pub fn item_with(
    id: impl Into<gpui_kit::ElementId>,
    icon: gpui_kit::AnyElement,
    text: &str,
    indices: &[usize],
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    let th = AppTheme::get(cx);
    let (bg, fg) = if active {
        (Some(th.colors.accent), th.colors.accent_foreground)
    } else {
        (None, th.colors.foreground)
    };
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .gap(u(8.))
        .w_full()
        .px(u(10.))
        .py(u(6.))
        .rounded(u(th.radius.sm))
        .text_size(u(12.))
        .text_color(fg)
        .when_some(bg, |d, bg| d.bg(bg))
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .text_color(th.colors.muted_foreground)
                .child(icon),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(highlighted(
                    text,
                    indices,
                    alpha(th.colors.syntax_number, 0.35),
                )),
        )
}

/// `.palette-overlay-alias`.
pub fn alias_chip(alias: &'static str, cx: &App) -> Div {
    let th = AppTheme::get(cx);
    div()
        .flex_none()
        .ml_auto()
        .font_family(th.mono_font.clone())
        .text_size(u(10.5))
        .text_color(th.colors.muted_foreground)
        .whitespace_nowrap()
        .child(alias)
}

/// `.palette-overlay-keys` (`<kbd>`); `after_alias` drops the auto margin.
pub fn keys_chip(text: String, active: bool, after_alias: bool, cx: &App) -> Div {
    let th = AppTheme::get(cx);
    let (fg, border) = if active {
        (
            th.colors.accent_foreground,
            alpha(th.colors.accent_foreground, 0.3),
        )
    } else {
        (th.colors.muted_foreground, th.colors.border)
    };
    div()
        .flex_none()
        .when(!after_alias, |d| d.ml_auto())
        .text_size(u(10.5))
        .text_color(fg)
        .bg(alpha(th.colors.muted_foreground, 0.12))
        .border_1()
        .border_color(border)
        .rounded(u(th.radius.sm))
        .px(u(5.))
        .py(u(1.))
        .whitespace_nowrap()
        .child(text)
}

/// Byte ranges of the given char indices, merged when adjacent.
pub fn char_ranges(text: &str, indices: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut want = indices.iter().copied().peekable();
    for (ci, (bi, ch)) in text.char_indices().enumerate() {
        while want.peek().is_some_and(|&i| i < ci) {
            want.next();
        }
        if want.peek() == Some(&ci) {
            let r = bi..bi + ch.len_utf8();
            match out.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => out.push(r),
            }
        }
    }
    out
}

/// `HighlightedText`: matched chars get the `syntax-number` 35% background.
pub fn highlighted(text: &str, indices: &[usize], bg: gpui_kit::Hsla) -> StyledText {
    let style = HighlightStyle {
        background_color: Some(bg),
        ..Default::default()
    };
    StyledText::new(SharedString::from(text.to_string()))
        .with_highlights(char_ranges(text, indices).into_iter().map(|r| (r, style)))
}

#[cfg(test)]
mod tests {
    use super::char_ranges;

    #[test]
    fn ranges_are_bytes_and_merged() {
        assert_eq!(char_ranges("Save File", &[0, 1, 2, 3]), vec![0..4]);
        assert_eq!(char_ranges("abc", &[0, 2]), vec![0..1, 2..3]);
        assert_eq!(
            char_ranges("calendário x", &[7, 8, 11]),
            vec![8..10, 12..13]
        );
        assert!(char_ranges("ab", &[5]).is_empty());
    }
}
