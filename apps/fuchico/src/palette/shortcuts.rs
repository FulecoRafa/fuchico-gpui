//! The searchable Keyboard Shortcuts list (`ShortcutsSection.tsx`): the
//! rebindable editor actions (click a chip, press keys) followed by the
//! fixed shortcuts grouped by area.
//!
//! [`ShortcutsList`] renders the search field and the rows only; the
//! settings view embeds it under its own section title/description
//! (`ShortcutsTitle` / `ShortcutsDesc`).

use gpui_kit::component::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, KeystrokeEvent, SharedString,
    Subscription, Window, div,
};

use crate::i18n::tr;
use crate::keymap::{
    FIXED_SHORTCUTS, FixedShortcut, SHORTCUT_ACTIONS, ShortcutAction, ShortcutGroup,
    binding_from_keystroke, display_binding,
};
use crate::settings::{Settings, SettingsStore};
use crate::theme::{AppTheme, alpha};
use crate::ui::u;

/// `matches(q, ...fields)`: every whitespace-separated term of `q` is a
/// case-insensitive substring of the joined fields.
pub fn matches(query: &str, fields: &[&str]) -> bool {
    let hay = fields.join(" ").to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|term| hay.contains(term))
}

/// The rows left after filtering: rebindable actions and fixed shortcuts
/// grouped in first-appearance order.
pub struct Filtered {
    pub rebindable: Vec<&'static ShortcutAction>,
    pub groups: Vec<(ShortcutGroup, Vec<&'static FixedShortcut>)>,
}

pub fn filter(query: &str, settings: &Settings) -> Filtered {
    let editor = "editor";
    let rebindable = SHORTCUT_ACTIONS
        .iter()
        .filter(|a| {
            let b = settings.shortcut(a.id);
            matches(
                query,
                &[
                    editor,
                    &tr(a.label, &[]),
                    &tr(a.desc, &[]),
                    b,
                    &display_binding(b),
                ],
            )
        })
        .collect();
    let mut groups: Vec<(ShortcutGroup, Vec<&'static FixedShortcut>)> = Vec::new();
    for f in FIXED_SHORTCUTS.iter() {
        let fields = [
            &tr(f.group.label(), &[]),
            &tr(f.label, &[]),
            &tr(f.desc, &[]),
            &f.keys_text(),
            &display_binding(f.keys),
        ];
        if !matches(query, &fields.map(String::as_str)) {
            continue;
        }
        match groups.iter_mut().find(|(g, _)| *g == f.group) {
            Some((_, list)) => list.push(f),
            None => groups.push((f.group, vec![f])),
        }
    }
    Filtered { rebindable, groups }
}

pub struct ShortcutsList {
    input: Entity<InputState>,
    /// The action id whose chip is waiting for keys.
    recording: Option<&'static str>,
    chip_focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl ShortcutsList {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::t!(ShortcutsSearchPlaceholder))
        });
        let chip_focus = cx.focus_handle();
        let weak = cx.entity().downgrade();
        let focus = chip_focus.clone();
        let subs = vec![
            cx.subscribe(&input, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.on_blur(&chip_focus, window, |this, _, cx| {
                this.recording = None;
                cx.notify();
            }),
            cx.observe_global_in::<SettingsStore>(window, |this, window, cx| {
                this.input.update(cx, |s, cx| {
                    s.set_placeholder(crate::t!(ShortcutsSearchPlaceholder), window, cx)
                });
                cx.notify();
            }),
            // The recorder sees keys before any binding (Escape, Mod-p, ...).
            cx.intercept_keystrokes(move |ev: &KeystrokeEvent, window, cx| {
                if !focus.is_focused(window) {
                    return;
                }
                let Some(this) = weak.upgrade() else { return };
                let Some(action) = this.read(cx).recording else {
                    return;
                };
                if ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.update(cx, |this, cx| this.stop(cx));
                    return;
                }
                let Some(binding) = binding_from_keystroke(&ev.keystroke) else {
                    return;
                };
                cx.stop_propagation();
                SettingsStore::set_shortcut(cx, action, &binding);
                this.update(cx, |this, cx| this.stop(cx));
            }),
        ];
        ShortcutsList {
            input,
            recording: None,
            chip_focus,
            _subs: subs,
        }
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        cx.notify();
    }

    pub fn recording(&self) -> Option<&'static str> {
        self.recording
    }

    fn render_info(label: String, desc: String, cx: &App) -> impl IntoElement {
        let th = AppTheme::get(cx);
        div()
            .flex()
            .flex_col()
            .gap(u(2.))
            .min_w_0()
            .child(
                div()
                    .text_size(u(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label),
            )
            .child(
                div()
                    .text_size(u(12.))
                    .text_color(th.colors.muted_foreground)
                    .child(desc),
            )
    }

    fn row(info: impl IntoElement, chip: impl IntoElement, cx: &App) -> gpui_kit::Div {
        let th = AppTheme::get(cx);
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(u(12.))
            .p(u(8.))
            .border_1()
            .border_color(th.colors.border)
            .rounded(u(th.radius.sm))
            .child(info)
            .child(chip)
    }

    /// `.settings-shortcut-key`.
    fn chip(text: String, cx: &App) -> gpui_kit::Div {
        let th = AppTheme::get(cx);
        div()
            .flex_none()
            .flex()
            .justify_center()
            .min_w(u(110.))
            .font_family(th.mono_font.clone())
            .text_size(u(11.))
            .border_1()
            .border_color(th.colors.border)
            .rounded(u(th.radius.sm))
            .bg(th.colors.secondary)
            .text_color(th.colors.secondary_foreground)
            .px(u(8.))
            .py(u(4.))
            .child(text)
    }

    fn group_header(text: String, cx: &App) -> AnyElement {
        let th = AppTheme::get(cx);
        div()
            .mt(u(12.))
            .mb(u(4.))
            .text_size(u(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(th.colors.muted_foreground)
            .child(text.to_uppercase())
            .into_any_element()
    }
}

impl Render for ShortcutsList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = AppTheme::get(cx).clone();
        let query = self.input.read(cx).value().to_string();
        let settings = SettingsStore::get(cx);
        let f = filter(&query, &settings);
        let mut rows: Vec<AnyElement> = Vec::new();
        if !f.rebindable.is_empty() {
            rows.push(Self::group_header(crate::t!(ShortcutsEditorRebindable), cx));
        }
        for a in &f.rebindable {
            let id = a.id;
            let recording = self.recording == Some(id);
            let text = if recording {
                crate::t!(ShortcutsPressKeys)
            } else {
                display_binding(settings.shortcut(id))
            };
            let chip = Self::chip(text, cx)
                .id(SharedString::from(format!("shortcut-key-{id}")))
                .debug_selector(move || format!("shortcut-key-{id}"))
                .cursor_pointer()
                .hover(|s| s.border_color(th.colors.ring))
                .when(recording, |d| {
                    d.track_focus(&self.chip_focus)
                        .border_color(th.colors.primary)
                        .text_color(th.colors.primary)
                        .bg(alpha(th.colors.primary, 0.12))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.recording = Some(id);
                    window.focus(&this.chip_focus, cx);
                    cx.notify();
                }));
            rows.push(
                Self::row(
                    Self::render_info(tr(a.label, &[]), tr(a.desc, &[]), cx),
                    chip,
                    cx,
                )
                .into_any_element(),
            );
        }
        for (group, items) in &f.groups {
            rows.push(Self::group_header(tr(group.label(), &[]), cx));
            for s in items {
                let chip = Self::chip(s.keys_text(), cx).opacity(0.85);
                rows.push(
                    Self::row(
                        Self::render_info(tr(s.label, &[]), tr(s.desc, &[]), cx),
                        chip,
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        if f.rebindable.is_empty() && f.groups.is_empty() {
            rows.push(
                div()
                    .text_size(u(12.))
                    .text_color(th.colors.muted_foreground)
                    .child(crate::t!(ShortcutsNoMatch, query = query))
                    .into_any_element(),
            );
        }
        div()
            .id("shortcuts-list")
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .mb(u(12.))
                    .text_color(th.colors.muted_foreground)
                    .child(gpui_kit::component::Icon::new(IconName::Search).size(u(13.)))
                    .child(
                        div()
                            .flex_1()
                            .max_w(u(320.))
                            .border_1()
                            .border_color(th.colors.border)
                            .rounded(u(th.radius.sm))
                            .bg(th.colors.background)
                            .text_color(th.colors.foreground)
                            .child(
                                Input::new(&self.input)
                                    .appearance(false)
                                    .px(u(8.))
                                    .py(u(6.))
                                    .text_size(u(13.)),
                            ),
                    ),
            )
            .child(div().flex().flex_col().gap(u(10.)).children(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_are_anded_substrings() {
        assert!(matches("", &["a"]));
        assert!(matches("  ", &["a"]));
        assert!(matches("ins da", &["Insert date", "x"]));
        assert!(matches("DATE", &["insert date"]));
        assert!(!matches("insert zzz", &["insert date"]));
        // Terms may span fields.
        assert!(matches("editor date", &["editor", "Insert date"]));
    }

    #[test]
    fn filter_groups_in_first_appearance_order() {
        crate::i18n::set_language(crate::i18n::Language::En);
        let s = Settings::default();
        let all = filter("", &s);
        assert_eq!(all.rebindable.len(), SHORTCUT_ACTIONS.len());
        assert_eq!(
            all.groups.iter().map(|(g, _)| *g).collect::<Vec<_>>(),
            [
                ShortcutGroup::Global,
                ShortcutGroup::Editor,
                ShortcutGroup::Explorer,
                ShortcutGroup::Tabs
            ]
        );
        // "editor" matches every rebindable row plus the Editor group.
        let ed = filter("editor", &s);
        assert_eq!(ed.rebindable.len(), SHORTCUT_ACTIONS.len());
        assert!(ed.groups.iter().any(|(g, _)| *g == ShortcutGroup::Editor));
        // Raw bindings are searchable.
        let tbl = filter("Mod-Alt-t", &s);
        assert_eq!(
            tbl.rebindable.iter().map(|a| a.id).collect::<Vec<_>>(),
            ["insertTable"]
        );
        let none = filter("qqqzzz", &s);
        assert!(none.rebindable.is_empty() && none.groups.is_empty());
    }
}
