//! The Settings main view (spec 5 §3): a 190px section nav with a settings
//! search on the left, the selected section (or, while searching, every
//! section filtered by visible text) on the right.
//!
//! Sections: Appearance (language, theme + custom theme editor, fonts),
//! Editor (keybindings, behavior, fold regions), Keyboard Shortcuts
//! (rebinding), Vault folders, Integrations (CalDAV) and About. Every
//! setting is written through [`SettingsStore::update`](crate::settings::SettingsStore::update).

mod appearance;
mod css_data;
mod editor;
mod integrations;
mod search;
mod shortcuts;
mod theme_editor;
mod vault;
mod widgets;

#[cfg(test)]
mod ui_tests;

use gpui_kit::component::Icon;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::SelectItem;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, EventEmitter, Focusable, FontWeight, SharedString, Subscription, Window, div, px};

use crate::assets::AppIcon;
use crate::i18n::{Msg, tr};
use crate::settings::ui_state::{SettingsSection, UiState};
use crate::theme::AppTheme;
use crate::ui::u;
use crate::workspace::view::{MainView, ViewEvent};

use widgets::{Item, Section};

/// A select option whose label differs from its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub label: SharedString,
    pub value: SharedString,
}

impl Opt {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Opt { label: label.into(), value: value.into() }
    }
}

impl SelectItem for Opt {
    type Value = SharedString;
    fn title(&self) -> SharedString {
        self.label.clone()
    }
    fn value(&self) -> &SharedString {
        &self.value
    }
}

pub struct SettingsView {
    root: Option<String>,
    query: Entity<InputState>,
    appearance: appearance::State,
    editor: editor::State,
    vault: vault::State,
    caldav: integrations::State,
    shortcuts: Entity<crate::palette::ShortcutsList>,
    _subs: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(root: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(crate::t!(SettingsSearchPlaceholder)));
        let mut subs = vec![cx.subscribe_in(&query, window, |_, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        })];
        let appearance = appearance::State::new(window, cx, &mut subs);
        let editor = editor::State::new(window, cx, &mut subs);
        let vault = vault::State::new(window, cx, &mut subs);
        let caldav = integrations::State::new(root.clone(), window, cx, &mut subs);
        let shortcuts = cx.new(|cx| crate::palette::ShortcutsList::new(window, cx));
        let mut view = SettingsView { root, query, appearance, editor, vault, caldav, shortcuts, _subs: subs };
        view.load_fonts(window, cx);
        view.refresh_caldav(cx);
        view
    }

    /// Selects a section and clears the search (nav click).
    pub fn select_section(&mut self, section: SettingsSection, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |q, cx| q.set_value("", window, cx));
        UiState::set_settings_section(cx, section);
        cx.notify();
    }

    fn sections(&mut self, s: SettingsSection, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        match s {
            SettingsSection::Appearance => self.appearance_sections(window, cx),
            SettingsSection::Editor => self.editor_sections(window, cx),
            SettingsSection::Shortcuts => vec![self.shortcuts_section(window, cx)],
            SettingsSection::Vault => vec![self.vault_section(window, cx)],
            SettingsSection::Integrations => self.integrations_sections(window, cx),
            SettingsSection::About => vec![vault::about_section(cx)],
        }
    }
}

impl EventEmitter<ViewEvent> for SettingsView {}

impl MainView for SettingsView {
    fn set_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        // The link form's folder defaults to the open vault.
        if self.caldav.folder.is_none() || self.caldav.folder == self.root {
            self.caldav.folder = root.clone();
        }
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

/// Copies `value` into an input unless the user is typing in it.
pub(crate) fn sync_input(input: &Entity<InputState>, value: &str, window: &mut Window, cx: &mut App) {
    let state = input.read(cx);
    if state.focus_handle(cx).is_focused(window) || state.value().as_ref() == value {
        return;
    }
    let value = value.to_string();
    input.update(cx, |s, cx| s.set_value(value, window, cx));
}

/// Renders one section, applying the search filter. `None` = hidden.
fn render_section(section: Section, terms: &[String], cx: &App) -> Option<gpui_kit::Div> {
    let th = AppTheme::get(cx);
    let head = format!("{} {}", section.title, section.desc);
    let texts: Vec<Option<&str>> = section
        .items
        .iter()
        .map(|i| match i {
            Item::Field(t, _) => Some(t.as_str()),
            Item::Other(_) => None,
        })
        .collect();
    let shown = search::visibility(terms, &head, &texts)?;
    let has_desc = !section.desc.is_empty();
    let desc = match section.desc_el {
        Some(el) => el,
        None => section.desc.clone().into_any_element(),
    };
    Some(
        div()
            .flex()
            .flex_col()
            .mb(u(28.))
            .child(div().text_size(u(13.)).font_weight(FontWeight::BOLD).mb(u(8.)).child(section.title))
            .when(has_desc, |d| {
                d.child(div().text_size(u(12.)).text_color(th.colors.muted_foreground).mb(u(12.)).child(desc))
            })
            .child(div().flex().flex_col().gap(u(10.)).children(section.items.into_iter().zip(shown).filter_map(
                |(item, show)| {
                    show.then_some(match item {
                        Item::Field(_, el) | Item::Other(el) => el,
                    })
                },
            ))),
    )
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let current = UiState::get(cx).settings_section;
        let query = self.query.read(cx).value().to_string();
        let terms = search::terms(&query);
        let searching = !terms.is_empty();
        let narrow = window.viewport_size().width < px(720.) * th.ui_scale;

        let nav = div()
            .id("settings-nav")
            .flex()
            .flex_none()
            .gap(u(2.))
            .px(u(10.))
            .py(u(16.))
            .when(narrow, |d| d.flex_row().flex_wrap().w_full().border_b_1())
            .when(!narrow, |d| d.flex_col().w(u(190.)).h_full().overflow_y_scroll().border_r_1())
            .border_color(th.colors.border)
            .child(
                div().mb(u(8.)).debug_selector(|| "settings-search".into()).when(narrow, |d| d.w_full()).child(
                    Input::new(&self.query)
                        .text_size(u(13.))
                        .cleanable(true)
                        .prefix(Icon::new(AppIcon::Search).size(u(13.)).text_color(th.colors.muted_foreground)),
                ),
            )
            .children(SettingsSection::ALL.into_iter().map(|s| {
                let active = s == current && !searching;
                div()
                    .id(SharedString::from(format!("settings-nav-{}", s.id())))
                    .debug_selector(move || format!("settings-nav-{}", s.id()))
                    .px(u(10.))
                    .py(u(6.))
                    .rounded(u(th.radius.sm))
                    .text_size(u(13.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(th.fg_alpha(0.09)).font_weight(FontWeight::MEDIUM))
                    .when(!active, |d| d.hover(|st| st.bg(th.fg_alpha(0.05))))
                    .on_click(cx.listener(move |this, _, window, cx| this.select_section(s, window, cx)))
                    .child(tr(section_label(s), &[]))
            }));

        let content: Vec<gpui_kit::Div> = if searching {
            let mut groups = Vec::new();
            for s in SettingsSection::ALL {
                let rendered: Vec<_> = self
                    .sections(s, window, cx)
                    .into_iter()
                    .filter_map(|sec| render_section(sec, &terms, cx))
                    .collect();
                if rendered.is_empty() {
                    continue;
                }
                groups.push(
                    div()
                        .flex()
                        .flex_col()
                        .debug_selector(move || format!("settings-group-{}", s.id()))
                        .child(
                            div()
                                .text_size(u(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(th.colors.muted_foreground)
                                .mb(u(12.))
                                .child(tr(section_label(s), &[]).to_uppercase()),
                        )
                        .children(rendered),
                );
            }
            if groups.is_empty() {
                vec![
                    div()
                        .debug_selector(|| "settings-no-results".into())
                        .text_size(u(13.))
                        .text_color(th.colors.muted_foreground)
                        .child(crate::t!(SettingsNoResults, query = query.trim())),
                ]
            } else {
                groups
            }
        } else {
            self.sections(current, window, cx).into_iter().filter_map(|sec| render_section(sec, &[], cx)).collect()
        };

        div()
            .flex()
            .when(narrow, |d| d.flex_col())
            .size_full()
            .bg(th.colors.background)
            .text_color(th.colors.foreground)
            .font_family(th.ui_font.clone())
            .child(nav)
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt(u(24.))
                    .px(u(28.))
                    .pb(u(40.))
                    .child(div().flex().flex_col().max_w(u(560.)).children(content)),
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
