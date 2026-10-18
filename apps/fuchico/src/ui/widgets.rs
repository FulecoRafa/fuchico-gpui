//! Small shared widgets drawn with app tokens (exact CSS sizes, see `u`).

use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    App, ClickEvent, ElementId, Hsla, prelude::FluentBuilder, InteractiveElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};

use crate::theme::AppTheme;
use crate::ui::u;

/// A square icon button: `size`×`size` box, `icon_size` glyph, accent hover,
/// optional active state and tooltip.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    size: f32,
    icon_size: f32,
    tooltip: impl Into<SharedString>,
    active: bool,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let th = AppTheme::get(cx);
    let (bg, fg): (Option<Hsla>, Hsla) =
        if active { (Some(th.colors.accent), th.colors.accent_foreground) } else { (None, th.colors.muted_foreground) };
    let hover_bg = th.colors.accent;
    let hover_fg = th.colors.accent_foreground;
    let tooltip: SharedString = tooltip.into();
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(size))
        .rounded(u(th.radius.sm))
        .text_color(fg)
        .when_some(bg, |d, bg| d.bg(bg))
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .cursor_pointer()
        .child(Icon::new(icon).size(u(icon_size)))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(on_click)
}

/// Centered muted message (empty states, "No file open").
pub fn empty_state(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .flex()
        .flex_1()
        .size_full()
        .items_center()
        .justify_center()
        .text_size(u(13.))
        .text_color(th.colors.muted_foreground)
        .child(text.into())
}

/// A placeholder main view: title plus a note about what will live here.
pub fn placeholder_view(title: impl Into<SharedString>, note: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .flex()
        .flex_col()
        .size_full()
        .p(u(24.))
        .gap(u(8.))
        .child(div().text_size(u(18.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(title.into()))
        .child(div().text_size(u(13.)).text_color(th.colors.muted_foreground).child(note.into()))
}

pub type SegmentHandler = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

/// One option of a [`segmented`] control.
pub struct Segment {
    pub label: SharedString,
    pub active: bool,
    pub disabled: bool,
    pub on_click: SegmentHandler,
}

impl Segment {
    pub fn new(label: impl Into<SharedString>, active: bool, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Segment { label: label.into(), active, disabled: false, on_click: std::rc::Rc::new(on_click) }
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
}

/// A row of toggle buttons (the original's `.seg` groups).
pub fn segmented(id: &'static str, segments: Vec<Segment>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    let (border, accent, accent_fg, fg, card) =
        (th.colors.border, th.colors.primary, th.colors.primary_foreground, th.colors.foreground, th.colors.card);
    let hover = th.colors.accent;
    div()
        .flex()
        .flex_none()
        .border_1()
        .border_color(border)
        .rounded(u(th.radius.md))
        .overflow_hidden()
        .bg(card)
        .children(segments.into_iter().enumerate().map(move |(i, s)| {
            let on_click = s.on_click.clone();
            div()
                .id((id, i))
                .px(u(10.))
                .py(u(4.))
                .text_size(u(12.))
                .when(i > 0, |d| d.border_l_1().border_color(border))
                .when(s.active, |d| d.bg(accent).text_color(accent_fg))
                .when(!s.active, |d| d.text_color(fg))
                .when(s.disabled, |d| d.opacity(0.5))
                .when(!s.disabled && !s.active, |d| d.cursor_pointer().hover(move |st| st.bg(hover)))
                .when(!s.disabled, |d| d.on_click(move |_, w, cx| on_click(w, cx)))
                .child(s.label)
        }))
}
