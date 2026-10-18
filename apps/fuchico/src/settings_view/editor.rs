//! Editor: Keybindings, Editor Behavior and Fold Regions (spec 5 §3.8–3.10).

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, Subscription, Window, div};

use super::widgets::{Section, Toggle, code, field, hint, hint_text, text_input, toggles};
use super::{SettingsView, sync_input};
use crate::settings::{KeybindingMode, Settings, SettingsStore, format_rulers, parse_rulers};
use crate::theme::AppTheme;
use crate::ui::u;

pub(super) struct State {
    rulers: Entity<InputState>,
    fold_start: Entity<InputState>,
    fold_end: Entity<InputState>,
}

/// The rulers text as committed: parsed, or `None` when unchanged.
pub(super) fn commit_rulers(text: &str, current: &[u32]) -> Option<Vec<u32>> {
    let next = parse_rulers(text);
    (next != current).then_some(next)
}

impl State {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<SettingsView>,
        subs: &mut Vec<Subscription>,
    ) -> State {
        let s = SettingsStore::get(cx);
        let rulers = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("80, 120")
                .default_value(format_rulers(&s.rulers))
        });
        // Rulers commit on blur / Enter (the text is reformatted then).
        subs.push(
            cx.subscribe_in(&rulers, window, |_, input, ev: &InputEvent, window, cx| {
                if !matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    return;
                }
                let text = input.read(cx).value().to_string();
                let current = SettingsStore::get(cx).rulers.clone();
                let next = commit_rulers(&text, &current).unwrap_or(current);
                let formatted = format_rulers(&next);
                SettingsStore::update(cx, |s| s.rulers = next);
                input.update(cx, |i, cx| i.set_value(formatted, window, cx));
            }),
        );
        let marker = |v: &str, window: &mut Window, cx: &mut Context<SettingsView>| {
            let v = v.to_string();
            cx.new(|cx| InputState::new(window, cx).default_value(v))
        };
        let fold_start = marker(&s.fold_start_marker, window, cx);
        let fold_end = marker(&s.fold_end_marker, window, cx);
        for (input, start) in [(&fold_start, true), (&fold_end, false)] {
            subs.push(
                cx.subscribe_in(input, window, move |_, input, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::Change) {
                        let v = input.read(cx).value().to_string();
                        SettingsStore::update(cx, |s| {
                            if start {
                                s.fold_start_marker = v
                            } else {
                                s.fold_end_marker = v
                            }
                        });
                    }
                }),
            );
        }
        State {
            rulers,
            fold_start,
            fold_end,
        }
    }
}

impl SettingsView {
    pub(super) fn editor_sections(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Section> {
        let s = SettingsStore::get(cx);
        let st = &self.editor;
        sync_input(&st.rulers, &format_rulers(&s.rulers), window, cx);
        sync_input(&st.fold_start, &s.fold_start_marker, window, cx);
        sync_input(&st.fold_end, &s.fold_end_marker, window, cx);
        vec![
            keybindings_section(&s, cx),
            self.behavior_section(&s, cx),
            self.folding_section(&s, cx),
        ]
    }

    fn behavior_section(&self, s: &Settings, cx: &mut Context<Self>) -> Section {
        let rel = s.relative_line_numbers;
        let rel_label = crate::t!(SettingsBehaviorRelativeLineNumbers);
        let numbers = toggles(
            vec![
                Toggle::new(
                    "settings-lines-absolute",
                    crate::t!(SettingsBehaviorAbsolute),
                    !rel,
                    |_, _, cx| SettingsStore::update(cx, |s| s.relative_line_numbers = false),
                )
                .tooltip(crate::t!(SettingsBehaviorAbsoluteTitle)),
                Toggle::new(
                    "settings-lines-relative",
                    crate::t!(SettingsBehaviorRelative),
                    rel,
                    |_, _, cx| SettingsStore::update(cx, |s| s.relative_line_numbers = true),
                )
                .tooltip(crate::t!(SettingsBehaviorRelativeTitle)),
            ],
            cx,
        );
        let tab_label = crate::t!(SettingsBehaviorTabSize);
        let tabs = toggles(
            [2u32, 4, 8]
                .into_iter()
                .map(|n| {
                    Toggle::new(
                        format!("settings-tab-{n}"),
                        n.to_string(),
                        s.tab_size == n,
                        move |_, _, cx| SettingsStore::update(cx, |s| s.tab_size = n),
                    )
                })
                .collect(),
            cx,
        );
        let rulers_label = crate::t!(SettingsBehaviorRulers);
        Section::new(
            crate::t!(SettingsBehaviorTitle),
            crate::t!(SettingsBehaviorDesc),
        )
        .field(
            format!("{rel_label} {}", crate::t!(SettingsBehaviorRelativeHint)),
            field(&rel_label, numbers, cx)
                .child(hint_text(crate::t!(SettingsBehaviorRelativeHint), cx)),
        )
        .field(
            format!("{tab_label} {}", crate::t!(SettingsBehaviorTabSizeHint)),
            field(&tab_label, tabs, cx)
                .child(hint_text(crate::t!(SettingsBehaviorTabSizeHint), cx)),
        )
        .field(
            format!("{rulers_label} {}", crate::t!(SettingsBehaviorRulersHint)),
            field(
                &rulers_label,
                div()
                    .debug_selector(|| "settings-rulers".into())
                    .child(text_input(&self.editor.rulers)),
                cx,
            )
            .child(hint_text(crate::t!(SettingsBehaviorRulersHint), cx)),
        )
    }

    fn folding_section(&self, s: &Settings, cx: &mut Context<Self>) -> Section {
        let th = AppTheme::get(cx).clone();
        let st = &self.editor;
        let (start_label, end_label) = (
            crate::t!(SettingsFoldingStartMarker),
            crate::t!(SettingsFoldingEndMarker),
        );
        let region = crate::t!(SettingsFoldingRegionName);
        let row = |label: String, el: gpui_kit::Div| {
            div()
                .flex()
                .items_center()
                .gap(u(10.))
                .child(
                    div()
                        .w(u(80.))
                        .text_size(u(11.))
                        .text_color(th.colors.muted_foreground)
                        .child(label),
                )
                .child(el)
        };
        let bar = div()
            .flex()
            .flex_1()
            .items_center()
            .gap(u(6.))
            .px(u(8.))
            .py(u(3.))
            .rounded(u(th.radius.sm))
            .bg(th.colors.primary.opacity(0.1))
            .border_l_2()
            .border_color(th.colors.primary)
            .text_size(u(12.))
            .child(div().text_color(th.colors.muted_foreground).child("▾"))
            .child("📁")
            .child(div().font_weight(FontWeight::MEDIUM).child(region.clone()));
        let pill = div()
            .flex()
            .items_center()
            .gap(u(6.))
            .px(u(8.))
            .py(u(2.))
            .rounded_full()
            .border_1()
            .border_color(th.colors.border)
            .bg(th.colors.muted)
            .text_size(u(12.))
            .child("📁")
            .child(div().font_weight(FontWeight::MEDIUM).child(region.clone()))
            .child(
                div()
                    .text_color(th.colors.muted_foreground)
                    .child(crate::t!(SettingsFoldingLinesExample)),
            );
        let preview = div()
            .flex()
            .flex_col()
            .gap(u(8.))
            .p(u(10.))
            .border_1()
            .border_color(th.colors.border)
            .rounded(u(th.radius.md))
            .child(row(crate::t!(SettingsFoldingExpanded), bar))
            .child(row(crate::t!(SettingsFoldingCollapsed), pill));
        Section::new(
            crate::t!(SettingsFoldingTitle),
            crate::t!(SettingsFoldingDesc),
        )
        .field(
            start_label.clone(),
            field(&start_label, text_input(&st.fold_start), cx).child(
                hint(cx)
                    .child(crate::t!(SettingsFoldingEg))
                    .child(code(format!("{} {region}", s.fold_start_marker), cx)),
            ),
        )
        .field(
            end_label.clone(),
            field(&end_label, text_input(&st.fold_end), cx),
        )
        .other(field(&crate::t!(CommonPreview), preview, cx))
    }
}

fn keybindings_section(s: &Settings, cx: &mut Context<SettingsView>) -> Section {
    let modes = [
        (
            KeybindingMode::Helix,
            "helix",
            "Helix".to_string(),
            crate::t!(SettingsKeybindingsHelixDesc),
        ),
        (
            KeybindingMode::Vim,
            "vim",
            "Vim".to_string(),
            crate::t!(SettingsKeybindingsVimDesc),
        ),
        (
            KeybindingMode::Normal,
            "normal",
            crate::t!(SettingsKeybindingsNormalLabel),
            crate::t!(SettingsKeybindingsNormalDesc),
        ),
    ];
    let current_desc = modes
        .iter()
        .find(|m| m.0 == s.keybinding_mode)
        .map(|m| m.3.clone())
        .unwrap_or_default();
    let search_text = modes
        .iter()
        .map(|m| format!("{} {}", m.2, m.3))
        .collect::<Vec<_>>()
        .join(" ");
    let group = toggles(
        modes
            .into_iter()
            .map(|(m, id, label, desc)| {
                Toggle::new(
                    format!("settings-keymap-{id}"),
                    label,
                    s.keybinding_mode == m,
                    move |_, _, cx| SettingsStore::update(cx, |s| s.keybinding_mode = m),
                )
                .tooltip(desc)
            })
            .collect(),
        cx,
    );
    Section::new(
        crate::t!(SettingsKeybindingsTitle),
        crate::t!(SettingsKeybindingsDesc),
    )
    .field(
        search_text,
        div()
            .flex()
            .flex_col()
            .gap(u(4.))
            .child(group)
            .child(hint_text(current_desc, cx)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rulers_commit_only_on_change() {
        assert_eq!(commit_rulers("80, 120", &[80, 120]), None);
        assert_eq!(commit_rulers("120,80", &[]), Some(parse_rulers("120,80")));
        assert_eq!(commit_rulers("", &[80]), Some(vec![]));
    }
}
