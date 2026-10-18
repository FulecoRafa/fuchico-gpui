//! Vault Folders and About (spec 5 §3.12, §3.13).

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, Subscription, Window};

use super::widgets::{BtnKind, Section, btn, code, field, hint, hint_text, text_input};
use super::{SettingsView, sync_input};
use crate::assets::AppIcon;
use crate::settings::{Settings, SettingsStore};

pub const GITHUB_URL: &str = "https://github.com/FulecoRafa/fuchico";

pub(super) struct State {
    daily: Entity<InputState>,
    templates: Entity<InputState>,
    external: Entity<InputState>,
}

type Setter = fn(&mut Settings, String);

impl State {
    pub fn new(window: &mut Window, cx: &mut Context<SettingsView>, subs: &mut Vec<Subscription>) -> State {
        let s = SettingsStore::get(cx);
        let mut make = |value: &str, placeholder: Option<String>, set: Setter| {
            let value = value.to_string();
            let input = cx.new(|cx| {
                let i = InputState::new(window, cx).default_value(value);
                match placeholder {
                    Some(p) => i.placeholder(p),
                    None => i,
                }
            });
            subs.push(cx.subscribe_in(&input, window, move |_, input, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    let v = input.read(cx).value().to_string();
                    SettingsStore::update(cx, |s| set(s, v));
                }
            }));
            input
        };
        let daily = make(&s.daily_notes_folder, None, |s, v| s.daily_notes_folder = v);
        let templates = make(&s.templates_folder, None, |s, v| s.templates_folder = v);
        let external =
            make(&s.external_tool, Some(crate::t!(SettingsVaultExternalToolPlaceholder)), |s, v| s.external_tool = v);
        State { daily, templates, external }
    }
}

impl SettingsView {
    pub(super) fn vault_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Section {
        let s = SettingsStore::get(cx);
        let st = &self.vault;
        sync_input(&st.daily, &s.daily_notes_folder, window, cx);
        sync_input(&st.templates, &s.templates_folder, window, cx);
        sync_input(&st.external, &s.external_tool, window, cx);
        let (prefix, and, images) =
            (crate::t!(SettingsVaultDescPrefix), crate::t!(SettingsVaultAnd), crate::t!(SettingsVaultDescImages));
        let desc = format!("{prefix} {{{{date}}}}, {{{{time}}}}, {{{{title}}}} {and} {{{{date:YYYY-MM-DD}}}}{images} attachments/.");
        let desc_el = hint(cx)
            .child(prefix)
            .child(code("{{date}}", cx))
            .child(code("{{time}}", cx))
            .child(code("{{title}}", cx))
            .child(and)
            .child(code("{{date:YYYY-MM-DD}}", cx))
            .child(images)
            .child(code("attachments/", cx));
        let (daily_l, tpl_l, ext_l) = (
            crate::t!(SettingsVaultDailyNotesFolder),
            crate::t!(SettingsVaultTemplatesFolder),
            crate::t!(SettingsVaultExternalTool),
        );
        let (tpl_pre, tpl_suf) = (crate::t!(SettingsVaultTemplatesHintPrefix), crate::t!(SettingsVaultTemplatesHintSuffix));
        Section::new(crate::t!(SettingsVaultTitle), desc)
            .desc_el(desc_el)
            .field(daily_l.clone(), field(&daily_l, text_input(&st.daily), cx))
            .field(
                format!("{tpl_l} {tpl_pre} daily.md {tpl_suf}"),
                field(&tpl_l, text_input(&st.templates), cx)
                    .child(hint(cx).child(tpl_pre).child(code("daily.md", cx)).child(tpl_suf)),
            )
            .field(
                format!("{ext_l} {}", crate::t!(SettingsVaultExternalToolHint)),
                field(&ext_l, text_input(&st.external), cx).child(hint_text(crate::t!(SettingsVaultExternalToolHint), cx)),
            )
    }
}

pub fn about_section(cx: &App) -> Section {
    let desc = crate::t!(SettingsAboutDesc, version = env!("CARGO_PKG_VERSION"));
    Section::new(crate::t!(SettingsAboutTitle), desc).other(
        gpui_kit::div().flex().child(btn(
            "settings-about-github",
            crate::t!(SettingsAboutGithub),
            BtnKind::Secondary,
            Some(AppIcon::Github),
            false,
            cx,
            |_, _, cx| cx.open_url(GITHUB_URL),
        )),
    )
}
