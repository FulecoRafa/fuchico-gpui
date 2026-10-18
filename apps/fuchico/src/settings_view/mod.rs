//! Settings view — PARTIAL main view.
//!
//! Implemented: the six-section nav (persisted `helix.settingsSection`) and
//! the core of Appearance: language, palette and mode segmented controls
//! (written through [`SettingsStore::update`], which re-themes every window),
//! plus editor font size / UI zoom readouts with -/+ steppers.
//!
//! Placeholders: custom themes (editor, import/export), font pickers, the
//! Editor / Shortcuts / Vault / Integrations sections and search.

use gpui_kit::prelude::*;
use gpui_kit::{App, Context, EventEmitter, FontWeight, SharedString, Window, div};

use crate::i18n::{Language, Msg, tr};
use crate::settings::ui_state::{SettingsSection, UiState};
use crate::settings::{ModePref, PaletteId, SettingsStore, clamp_editor_font_size, clamp_ui_scale};
use crate::theme::AppTheme;
use crate::ui::{Segment, segmented, u};
use crate::workspace::view::{MainView, ViewEvent};

pub struct SettingsView {
    root: Option<String>,
}

impl SettingsView {
    pub fn new(root: Option<String>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        SettingsView { root }
    }
}

impl EventEmitter<ViewEvent> for SettingsView {}

impl MainView for SettingsView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.root = root;
        cx.notify();
    }
}

pub fn section_label(s: SettingsSection) -> Msg {
    match s {
        SettingsSection::Appearance => Msg::SettingsNavAppearance,
        SettingsSection::Editor => Msg::SettingsNavEditor,
        SettingsSection::Shortcuts => Msg::SettingsNavShortcuts,
        SettingsSection::Vault => Msg::SettingsNavVault,
        SettingsSection::Integrations => Msg::SettingsNavIntegrations,
        SettingsSection::About => Msg::SettingsNavAbout,
    }
}

fn row(label: impl Into<SharedString>, control: impl IntoElement, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(u(16.))
        .py(u(8.))
        .border_b_1()
        .border_color(th.colors.border)
        .child(div().text_size(u(13.)).child(label.into()))
        .child(control)
}

fn card(title: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    let th = AppTheme::get(cx);
    div()
        .flex()
        .flex_col()
        .p(u(16.))
        .gap(u(4.))
        .rounded(u(th.radius.lg))
        .border_1()
        .border_color(th.colors.border)
        .bg(th.colors.card)
        .child(div().text_size(u(14.)).font_weight(FontWeight::SEMIBOLD).pb(u(4.)).child(title.into()))
}

fn stepper(id: &'static str, value: String, on_step: impl Fn(f32, &mut App) + 'static, cx: &App) -> impl IntoElement {
    let on_step = std::rc::Rc::new(on_step);
    let (minus, plus) = (on_step.clone(), on_step);
    segmented(
        id,
        vec![
            Segment::new("-", false, move |_, cx| minus(-1., cx)),
            Segment::new(value, false, |_, _| {}).disabled(true),
            Segment::new("+", false, move |_, cx| plus(1., cx)),
        ],
        cx,
    )
}

impl SettingsView {
    fn appearance(&self, cx: &App) -> gpui_kit::Div {
        let s = SettingsStore::get(cx);
        let langs = [
            (Language::System, crate::t!(SettingsLanguageSystem)),
            (Language::En, "English".to_string()),
            (Language::PtBr, "Português (Brasil)".to_string()),
        ];
        let language = segmented(
            "settings-language",
            langs
                .into_iter()
                .map(|(l, label)| Segment::new(label, s.language == l, move |_, cx| {
                    SettingsStore::update(cx, |s| s.language = l)
                }))
                .collect(),
            cx,
        );
        let mut palettes: Vec<Segment> = PaletteId::BUILT_IN
            .into_iter()
            .map(|p| Segment::new(p.display_name(), s.palette == p, move |_, cx| SettingsStore::update(cx, |s| s.palette = p)))
            .collect();
        let has_custom = !s.custom_themes.is_empty();
        palettes.push(
            Segment::new(crate::t!(SettingsThemePaletteCustom), s.palette == PaletteId::Custom, |_, cx| {
                SettingsStore::update(cx, |s| {
                    if s.custom_theme_id.is_none() {
                        s.custom_theme_id = s.custom_themes.first().map(|t| t.id.clone());
                    }
                    s.palette = PaletteId::Custom;
                })
            })
            .disabled(!has_custom),
        );
        let dracula = s.palette == PaletteId::Dracula;
        let modes = [
            (ModePref::System, crate::t!(SettingsThemeModeSystem)),
            (ModePref::Light, crate::t!(SettingsThemeModeLight)),
            (ModePref::Dark, crate::t!(SettingsThemeModeDark)),
        ];
        let mode = segmented(
            "settings-mode",
            modes
                .into_iter()
                .map(|(m, label)| {
                    Segment::new(label, s.mode == m, move |_, cx| SettingsStore::update(cx, |s| s.mode = m)).disabled(dracula)
                })
                .collect(),
            cx,
        );
        let font = stepper(
            "settings-font-size",
            format!("{}px", s.editor_font_size),
            |d, cx| SettingsStore::update(cx, |s| s.editor_font_size = clamp_editor_font_size(s.editor_font_size + d)),
            cx,
        );
        let zoom = stepper(
            "settings-ui-zoom",
            format!("{}%", (s.ui_scale * 100.).round()),
            |d, cx| SettingsStore::update(cx, |s| s.ui_scale = clamp_ui_scale(s.ui_scale + d * 0.1)),
            cx,
        );
        let muted = AppTheme::get(cx).colors.muted_foreground;
        div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .child(card(crate::t!(SettingsLanguageTitle), cx).child(row(crate::t!(SettingsLanguageLabel), language, cx)))
            .child(
                card(crate::t!(SettingsThemeTitle), cx)
                    .child(row(crate::t!(SettingsThemePalette), segmented("settings-palette", palettes, cx), cx))
                    .child(row(crate::t!(SettingsThemeMode), mode, cx))
                    .when(dracula, |d| {
                        d.child(div().text_size(u(12.)).text_color(muted).child(crate::t!(SettingsThemeDraculaHint)))
                    }),
            )
            .child(
                card(crate::t!(SettingsFontTitle), cx)
                    .child(row(crate::t!(SettingsFontEditorFontSize), font, cx))
                    .child(row(crate::t!(SettingsFontUiZoom), zoom, cx)),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let current = UiState::get(cx).settings_section;
        let nav = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(u(200.))
            .p(u(12.))
            .gap(u(2.))
            .border_r_1()
            .border_color(th.colors.border)
            .children(SettingsSection::ALL.into_iter().map(|s| {
                let active = s == current;
                div()
                    .id(SharedString::from(format!("settings-nav-{}", s.id())))
                    .px(u(10.))
                    .py(u(6.))
                    .rounded(u(th.radius.md))
                    .text_size(u(13.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(th.colors.accent).text_color(th.colors.accent_foreground))
                    .when(!active, |d| d.hover(|st| st.bg(th.fg_alpha(0.06))))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        UiState::set_settings_section(cx, s);
                        cx.notify();
                    }))
                    .child(tr(section_label(s), &[]))
            }));
        let body = match current {
            SettingsSection::Appearance => self.appearance(cx),
            other => crate::ui::placeholder_view(
                tr(section_label(other), &[]),
                "This section is not ported yet.",
                cx,
            ),
        };
        div()
            .flex()
            .size_full()
            .child(nav)
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(u(24.))
                    .child(div().max_w(u(720.)).child(body)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_a_label() {
        let labels: std::collections::HashSet<_> =
            SettingsSection::ALL.into_iter().map(|s| format!("{:?}", section_label(s))).collect();
        assert_eq!(labels.len(), 6);
    }
}
