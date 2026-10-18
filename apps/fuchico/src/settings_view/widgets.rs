//! Settings-local widgets drawn with the app tokens, sized after the
//! original's shared settings styling (spec 5 §3.15).

use std::rc::Rc;

use gpui_kit::component::Icon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, Entity, FontWeight, Hsla, SharedString, Window, div,
};

use crate::assets::AppIcon;
use crate::theme::AppTheme;
use crate::ui::u;

pub type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One block of the settings page: title, description and its items.
pub struct Section {
    pub title: String,
    pub desc: String,
    /// Rich description (inline code); `desc` stays the searchable text.
    pub desc_el: Option<AnyElement>,
    pub items: Vec<Item>,
}

impl Section {
    pub fn new(title: String, desc: String) -> Self {
        Section {
            title,
            desc,
            desc_el: None,
            items: Vec::new(),
        }
    }

    pub fn desc_el(mut self, el: impl IntoElement) -> Self {
        self.desc_el = Some(el.into_any_element());
        self
    }

    /// A filterable field (its `text` is what the search matches).
    pub fn field(mut self, text: impl Into<String>, el: impl IntoElement) -> Self {
        self.items
            .push(Item::Field(text.into(), el.into_any_element()));
        self
    }

    /// Content shown whenever the section is (previews, headers).
    pub fn other(mut self, el: impl IntoElement) -> Self {
        self.items.push(Item::Other(el.into_any_element()));
        self
    }
}

pub enum Item {
    Field(String, AnyElement),
    Other(AnyElement),
}

/// `.settings-label`: 11px semibold muted.
pub fn label(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .text_size(u(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(th.colors.muted_foreground)
        .child(text.into())
}

/// `.settings-hint`: 12px muted.
pub fn hint(cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .flex()
        .flex_wrap()
        .gap_x(u(4.))
        .text_size(u(12.))
        .text_color(th.colors.muted_foreground)
}

pub fn hint_text(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    hint(cx).child(text.into())
}

/// Inline `<code>`.
pub fn code(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .px(u(4.))
        .rounded(u(th.radius.sm))
        .bg(th.fg_alpha(0.08))
        .font_family(th.mono_font.clone())
        .text_size(u(11.))
        .child(text.into())
}

/// Status line: ok = `--syntax-function`, error = `--destructive`.
pub fn status(text: impl Into<SharedString>, ok: bool, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .mt(u(6.))
        .text_size(u(12.))
        .text_color(if ok {
            th.colors.syntax_function
        } else {
            th.colors.destructive
        })
        .child(text.into())
}

/// `.settings-field`: label above control, optional hint below.
pub fn field(label_text: &str, control: impl IntoElement, cx: &App) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap(u(4.))
        .child(label(label_text.to_string(), cx))
        .child(control)
}

/// A framed single-line text input.
pub fn text_input(state: &Entity<InputState>) -> Input {
    Input::new(state).text_size(u(13.))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Primary,
    Secondary,
    Danger,
}

/// `.btn` / `.btn-secondary` / `.btn-danger`.
#[allow(clippy::too_many_arguments)]
pub fn btn(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    kind: BtnKind,
    icon: Option<AppIcon>,
    disabled: bool,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let th = AppTheme::get(cx);
    let (bg, fg) = match kind {
        BtnKind::Primary => (th.colors.primary, th.colors.primary_foreground),
        BtnKind::Secondary => (th.colors.secondary, th.colors.secondary_foreground),
        BtnKind::Danger => (th.colors.secondary, th.colors.destructive),
    };
    let hover = bg.opacity(0.8);
    let id: SharedString = id.into();
    let sel = id.to_string();
    div()
        .id(ElementId::Name(id))
        .debug_selector(move || sel.clone())
        .flex()
        .flex_none()
        .items_center()
        .gap(u(6.))
        .px(u(16.))
        .py(u(7.))
        .rounded(u(th.radius.xl))
        .bg(bg)
        .text_color(fg)
        .text_size(u(13.))
        .font_weight(FontWeight::MEDIUM)
        .when_some(icon, |d, i| d.child(Icon::new(i).size(u(13.))))
        .child(label.into())
        .when(disabled, |d| d.opacity(0.5))
        .when(!disabled, |d| {
            d.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(on_click)
        })
}

/// One toggle button of a [`toggles`] group (`theme-palette-btn`).
pub struct Toggle {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<AppIcon>,
    pub active: bool,
    pub disabled: bool,
    pub tooltip: Option<SharedString>,
    pub on_click: Handler,
}

impl Toggle {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        active: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Toggle {
            id: id.into(),
            label: label.into(),
            icon: None,
            active,
            disabled: false,
            tooltip: None,
            on_click: Rc::new(on_click),
        }
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn tooltip(mut self, t: impl Into<SharedString>) -> Self {
        self.tooltip = Some(t.into());
        self
    }
    pub fn icon(mut self, i: AppIcon) -> Self {
        self.icon = Some(i);
        self
    }
}

/// A wrapping group of toggle buttons (gap 6px).
pub fn toggles(items: Vec<Toggle>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    let (border, ring, primary, primary_fg, bg, fg) = (
        th.colors.border,
        th.colors.ring,
        th.colors.primary,
        th.colors.primary_foreground,
        th.colors.background,
        th.colors.foreground,
    );
    let radius = u(th.radius.sm);
    div()
        .flex()
        .flex_wrap()
        .gap(u(6.))
        .children(items.into_iter().map(move |t| {
            let on_click = t.on_click.clone();
            let tooltip = t.tooltip.clone();
            div()
                .id(ElementId::Name(t.id.clone()))
                .debug_selector(|| t.id.to_string())
                .flex()
                .items_center()
                .gap(u(4.))
                .px(u(10.))
                .py(u(5.))
                .border_1()
                .rounded(radius)
                .text_size(u(12.))
                .when(t.active, |d| {
                    d.bg(primary).border_color(primary).text_color(primary_fg)
                })
                .when(!t.active, |d| d.bg(bg).border_color(border).text_color(fg))
                .when(t.disabled, |d| d.opacity(0.5))
                .when(!t.disabled, |d| {
                    d.cursor_pointer()
                        .hover(move |s| s.border_color(ring))
                        .on_click(move |e, w, cx| on_click(e, w, cx))
                })
                .when_some(tooltip, |d, tip| {
                    d.tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                })
                .when_some(t.icon, |d, i| d.child(Icon::new(i).size(u(13.))))
                .child(t.label)
        }))
}

/// Bordered row (account / link / shortcut / sync log).
pub fn row_box(cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .flex()
        .items_center()
        .gap(u(8.))
        .px(u(8.))
        .py(u(6.))
        .border_1()
        .border_color(th.colors.border)
        .rounded(u(th.radius.sm))
}

/// The preview card: toolbar with three dots, Primary / Secondary buttons,
/// the pangram and a syntax-colored code sample, drawn with `th`.
pub fn preview_card(
    th: &AppTheme,
    ui_font: Option<SharedString>,
    mono_font: Option<SharedString>,
) -> gpui_kit::Div {
    let c = &th.colors;
    let dot = || div().size(u(8.)).rounded_full().bg(c.muted);
    let pill = |text: String, bg: Hsla, fg: Hsla| {
        div()
            .px(u(16.))
            .py(u(7.))
            .rounded(u(th.radius.xl))
            .bg(bg)
            .text_color(fg)
            .text_size(u(13.))
            .font_weight(FontWeight::MEDIUM)
            .child(text)
    };
    let span = |t: &'static str, col: Hsla| div().text_color(col).child(t);
    let line = |parts: Vec<gpui_kit::Div>| div().flex().whitespace_nowrap().children(parts);
    let fg = c.foreground;
    let code = div()
        .flex()
        .flex_col()
        .p(u(10.))
        .rounded(u(th.radius.md))
        .border_1()
        .border_color(c.border)
        .bg(c.card)
        .text_size(u(11.))
        .line_height(u(11. * 1.6))
        .font_family(mono_font.unwrap_or_else(|| th.mono_font.clone()))
        .child(line(vec![
            span("function", c.syntax_keyword),
            span(" ", fg),
            span("greet", c.syntax_function),
            span("(", fg),
            span("name", c.syntax_type),
            span(") {", fg),
        ]))
        .child(line(vec![
            span("  ", fg),
            span("return", c.syntax_keyword),
            span(" ", fg),
            span("`Hello, ${name}`", c.syntax_string),
            span("; ", fg),
            span("// greets someone", c.syntax_comment),
        ]))
        .child(line(vec![span("}", fg)]));
    div()
        .flex()
        .flex_col()
        .border_1()
        .border_color(c.border)
        .rounded(u(th.radius.md))
        .overflow_hidden()
        .bg(c.background)
        .text_color(c.foreground)
        .child(
            div()
                .flex()
                .gap(u(4.))
                .px(u(10.))
                .py(u(8.))
                .bg(c.card)
                .border_b_1()
                .border_color(c.border)
                .children([dot(), dot(), dot()]),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .p(u(14.))
                .gap(u(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(u(10.))
                        .font_family(ui_font.unwrap_or_else(|| th.ui_font.clone()))
                        .child(
                            div()
                                .flex()
                                .gap(u(8.))
                                .child(pill(
                                    crate::t!(SettingsPreviewPrimary),
                                    c.primary,
                                    c.primary_foreground,
                                ))
                                .child(pill(
                                    crate::t!(SettingsPreviewSecondary),
                                    c.secondary,
                                    c.secondary_foreground,
                                )),
                        )
                        .child(
                            div()
                                .text_size(u(13.))
                                .child(crate::t!(SettingsPreviewPangram)),
                        ),
                )
                .child(code),
        )
}
