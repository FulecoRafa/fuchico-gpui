//! Appearance: Language, Theme (palettes, custom themes with the CSS
//! editor, import/export, preview) and Font (spec 5 §3.3–3.7).

use std::path::PathBuf;

use gpui_kit::component::input::{Editor, EditorState, InputEvent, InputState, NumberInput};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, PathPromptOptions, SharedString, Subscription, Window, div};

use super::widgets::{BtnKind, Section, Toggle, btn, code, field, hint, hint_text, preview_card, status, text_input, toggles};
use super::{Opt, SettingsView, sync_input, theme_editor};
use crate::assets::AppIcon;
use crate::i18n::Language;
use crate::settings::{
    CustomTheme, ModePref, PaletteId, Settings, SettingsStore, clamp_editor_font_size, clamp_ui_scale,
    new_custom_theme_id,
};
use crate::theme::{self, AppTheme, files};
use crate::ui::u;

type FontSelect = Entity<SelectState<SearchableVec<Opt>>>;

pub(super) enum Fonts {
    Loading,
    Loaded,
    Failed(String),
}

pub(super) struct State {
    fonts: Fonts,
    ui_font: FontSelect,
    editor_font: FontSelect,
    font_size: Entity<InputState>,
    zoom: Entity<InputState>,
    theme_name: Entity<InputState>,
    css: Entity<EditorState>,
    /// Id of the custom theme the draft belongs to.
    editing_id: Option<String>,
    draft: String,
    /// Transient import/export result: (text, ok).
    theme_status: Option<(String, bool)>,
}

fn font_options(families: &[String]) -> SearchableVec<Opt> {
    let mut items = vec![Opt::new(crate::t!(CommonDefault), "")];
    items.extend(families.iter().map(|f| Opt::new(f.clone(), f.clone())));
    SearchableVec::new(items)
}

impl State {
    pub fn new(window: &mut Window, cx: &mut Context<SettingsView>, subs: &mut Vec<Subscription>) -> State {
        let s = SettingsStore::get(cx);
        let select = |window: &mut Window, cx: &mut Context<SettingsView>| {
            cx.new(|cx| SelectState::new(font_options(&[]), None, window, cx).searchable(true))
        };
        let ui_font = select(window, cx);
        let editor_font = select(window, cx);
        for (sel, is_ui) in [(&ui_font, true), (&editor_font, false)] {
            subs.push(cx.subscribe_in(sel, window, move |_, _, ev: &SelectEvent<SearchableVec<Opt>>, _, cx| {
                let SelectEvent::Confirm(Some(v)) = ev else { return };
                let v = v.to_string();
                SettingsStore::update(cx, |s| {
                    if is_ui {
                        s.ui_font = v
                    } else {
                        s.editor_font = v
                    }
                });
            }));
        }

        let font_size = cx.new(|cx| {
            InputState::new(window, cx)
                .step(1.)
                .min(8.)
                .max(32.)
                .default_value(format!("{}", s.editor_font_size))
        });
        subs.push(cx.subscribe_in(&font_size, window, |_, input, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                let v = input.read(cx).value().trim().parse::<f32>().unwrap_or(f32::NAN);
                SettingsStore::update(cx, |s| s.editor_font_size = clamp_editor_font_size(v));
            }
        }));
        let zoom = cx.new(|cx| {
            InputState::new(window, cx)
                .step(10.)
                .min(70.)
                .max(160.)
                .default_value(format!("{}", (s.ui_scale * 100.).round()))
        });
        subs.push(cx.subscribe_in(&zoom, window, |_, input, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change)
                && let Ok(v) = input.read(cx).value().trim().parse::<f32>()
            {
                SettingsStore::update(cx, |s| s.ui_scale = clamp_ui_scale(v / 100.));
            }
        }));

        let theme_name = cx.new(|cx| InputState::new(window, cx));
        subs.push(cx.subscribe_in(&theme_name, window, |this, input, ev: &InputEvent, _, cx| {
            if !matches!(ev, InputEvent::Change) {
                return;
            }
            let Some(id) = this.appearance.editing_id.clone() else { return };
            let name = input.read(cx).value().to_string();
            SettingsStore::update(cx, |s| {
                if let Some(t) = s.custom_themes.iter_mut().find(|t| t.id == id) {
                    t.name = name;
                }
            });
        }));
        let css = cx.new(|cx| theme_editor::new_css_editor(window, cx));
        subs.push(cx.subscribe_in(&css, window, |this, editor, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                this.appearance.draft = editor.read(cx).value().to_string();
                cx.notify();
            }
        }));

        State {
            fonts: Fonts::Loading,
            ui_font,
            editor_font,
            font_size,
            zoom,
            theme_name,
            css,
            editing_id: None,
            draft: String::new(),
            theme_status: None,
        }
    }
}

/// The custom theme being edited (palette is custom and one is active).
fn editing(s: &Settings) -> Option<&CustomTheme> {
    if s.palette == PaletteId::Custom { s.active_custom_theme() } else { None }
}

fn theme_names(s: &Settings) -> Vec<String> {
    s.custom_themes.iter().map(|t| t.name.clone()).collect()
}

/// Adds a theme, selects it and switches to the custom palette.
fn add_theme(cx: &mut gpui_kit::App, name: String, css: String) {
    SettingsStore::update(cx, move |s| {
        let id = new_custom_theme_id();
        s.custom_themes.push(CustomTheme { id: id.clone(), name, css });
        s.custom_theme_id = Some(id);
        s.palette = PaletteId::Custom;
    });
}

impl SettingsView {
    /// Lists system fonts off the UI thread (once, on mount).
    pub(super) fn load_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async move { fuchico_core::fonts::fonts_list_system() });
        cx.spawn_in(window, async move |this, cx| {
            let res = task.await;
            this.update_in(cx, |this, window, cx| {
                match res {
                    Ok(families) => {
                        let st = &this.appearance;
                        for sel in [st.ui_font.clone(), st.editor_font.clone()] {
                            sel.update(cx, |sel, cx| sel.set_items(font_options(&families), window, cx));
                        }
                        this.appearance.fonts = Fonts::Loaded;
                    }
                    Err(e) => this.appearance.fonts = Fonts::Failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Keeps inputs, selects and the CSS draft in step with the settings.
    fn sync_appearance(&mut self, s: &Settings, window: &mut Window, cx: &mut Context<Self>) {
        let st = &mut self.appearance;
        sync_input(&st.font_size, &format!("{}", s.editor_font_size), window, cx);
        sync_input(&st.zoom, &format!("{}", (s.ui_scale * 100.).round()), window, cx);
        if matches!(st.fonts, Fonts::Loaded) {
            for (sel, v) in [(&st.ui_font, &s.ui_font), (&st.editor_font, &s.editor_font)] {
                let want = SharedString::from(v.clone());
                if sel.read(cx).selected_value() != Some(&want) {
                    sel.update(cx, |sel, cx| sel.set_selected_value(&want, window, cx));
                }
            }
        }
        let editing = editing(s);
        let id = editing.map(|t| t.id.clone());
        if id != st.editing_id {
            // Switching themes replaces the draft; edits never leak across.
            st.editing_id = id;
            st.draft = editing.map(|t| t.css.clone()).unwrap_or_default();
            st.theme_status = None;
            let draft = st.draft.clone();
            st.css.update(cx, |e, cx| e.set_value(draft, window, cx));
        }
        if let Some(t) = editing {
            sync_input(&st.theme_name, &t.name, window, cx);
        }
    }

    fn import_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::t!(SettingsThemeImportTheme).into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let p = path.to_string_lossy().to_string();
            if !files::is_css_path(&p) {
                this.update(cx, |this, cx| {
                    this.appearance.theme_status = Some((crate::t!(SettingsThemeImportFailed), false));
                    cx.notify();
                })
                .ok();
                return;
            }
            let read = cx.background_spawn({
                let p = p.clone();
                async move { fuchico_core::fs::file::fs_read_file(p) }
            });
            let res = read.await;
            this.update(cx, |this, cx| {
                use fuchico_core::fs::file::ReadResult;
                this.appearance.theme_status = Some(match res {
                    Ok(ReadResult::Text { content, .. }) => {
                        let (name, css) = files::parse_theme_file(&content, &files::theme_stem(&p));
                        let names = theme_names(&SettingsStore::get(cx));
                        let name = files::unique_name(&name, names.iter().map(String::as_str));
                        add_theme(cx, name, css);
                        (crate::t!(SettingsThemeImported), true)
                    }
                    Ok(_) => (crate::t!(SettingsThemeImportFailed), false),
                    Err(e) => (e, false),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn export_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = SettingsStore::get(cx);
        let Some(theme) = editing(&s) else { return };
        let content = files::serialize_theme(&theme.name, &self.appearance.draft);
        let file_name = files::theme_file_name(&theme.name);
        let dir = self.root.clone().map(PathBuf::from).or_else(std::env::home_dir).unwrap_or_default();
        let target = cx.prompt_for_new_path(&dir, Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = target.await else { return };
            let p = files::with_css_extension(path).to_string_lossy().to_string();
            let write = cx.background_spawn(async move {
                fuchico_core::fs::file::fs_write_file(
                    p,
                    content,
                    Some("theme-export".into()),
                    crate::bus::sink().as_ref(),
                )
            });
            let res = write.await;
            this.update(cx, |this, cx| {
                this.appearance.theme_status = Some(match res {
                    Ok(()) => (crate::t!(SettingsThemeExported), true),
                    Err(e) => (e, false),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = SettingsStore::get(cx);
        let Some(theme) = editing(&s) else { return };
        let id = theme.id.clone();
        let opts = crate::ui::ConfirmOptions::new(crate::t!(SettingsThemeDeleteConfirm, name = theme.name)).danger();
        crate::ui::confirm(window, cx, opts, move |_, cx| {
            let id = id.clone();
            SettingsStore::update(cx, move |s| {
                s.custom_themes.retain(|t| t.id != id);
                s.custom_theme_id = s.custom_themes.first().map(|t| t.id.clone());
                s.palette = if s.custom_themes.is_empty() { PaletteId::Ayu } else { PaletteId::Custom };
            });
        });
    }

    pub(super) fn appearance_sections(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let s = SettingsStore::get(cx);
        self.sync_appearance(&s, window, cx);
        vec![self.language_section(&s, cx), self.theme_section(&s, cx), self.font_section(&s, cx)]
    }

    fn language_section(&self, s: &Settings, cx: &mut Context<Self>) -> Section {
        let langs = [
            (Language::System, "system", crate::t!(SettingsLanguageSystem)),
            (Language::En, "en", "English".to_string()),
            (Language::PtBr, "pt-BR", "Português (Brasil)".to_string()),
        ];
        let label = crate::t!(SettingsLanguageLabel);
        let group = toggles(
            langs
                .into_iter()
                .map(|(l, id, text)| {
                    Toggle::new(format!("settings-language-{id}"), text, s.language == l, move |_, _, cx| {
                        SettingsStore::update(cx, |s| s.language = l)
                    })
                })
                .collect(),
            cx,
        );
        Section::new(crate::t!(SettingsLanguageTitle), crate::t!(SettingsLanguageDesc))
            .field(format!("{label} System English Português"), field(&label, group, cx))
    }

    fn theme_section(&mut self, s: &Settings, cx: &mut Context<Self>) -> Section {
        let th = AppTheme::get(cx).clone();
        let mut palettes: Vec<Toggle> = PaletteId::BUILT_IN
            .into_iter()
            .map(|p| {
                Toggle::new(format!("settings-palette-{}", p.display_name()), p.display_name(), s.palette == p, move |_, _, cx| {
                    SettingsStore::update(cx, |s| s.palette = p)
                })
            })
            .collect();
        for t in &s.custom_themes {
            let id = t.id.clone();
            let active = s.palette == PaletteId::Custom && s.custom_theme_id.as_deref() == Some(&t.id);
            palettes.push(Toggle::new(format!("settings-palette-custom-{}", t.id), t.name.clone(), active, move |_, _, cx| {
                let id = id.clone();
                SettingsStore::update(cx, |s| {
                    s.palette = PaletteId::Custom;
                    s.custom_theme_id = Some(id);
                })
            }));
        }
        palettes.push(
            Toggle::new("settings-theme-new", crate::t!(SettingsThemeNewTheme), false, |_, _, cx| {
                let s = SettingsStore::get(cx);
                let names = theme_names(&s);
                let name = files::unique_name(&crate::t!(SettingsThemeNewThemeName), names.iter().map(String::as_str));
                let css = theme::snapshot_css(&AppTheme::get(cx).colors);
                add_theme(cx, name, css);
            })
            .icon(AppIcon::Plus)
            .tooltip(crate::t!(SettingsThemeNewTheme)),
        );
        palettes.push(
            Toggle::new(
                "settings-theme-import",
                crate::t!(SettingsThemeImportTheme),
                false,
                cx.listener(|this, _, window, cx| this.import_theme(window, cx)),
            )
            .icon(AppIcon::Upload)
            .tooltip(crate::t!(SettingsThemeImportTheme)),
        );
        let palette_label = crate::t!(SettingsThemePalette);
        let custom_names = theme_names(s).join(" ");

        let dracula = s.palette == PaletteId::Dracula;
        let modes = [
            (ModePref::System, "system", crate::t!(SettingsThemeModeSystem)),
            (ModePref::Light, "light", crate::t!(SettingsThemeModeLight)),
            (ModePref::Dark, "dark", crate::t!(SettingsThemeModeDark)),
        ];
        let mode_label = crate::t!(SettingsThemeMode);
        let mode_group = toggles(
            modes
                .into_iter()
                .map(|(m, id, text)| {
                    Toggle::new(format!("settings-mode-{id}"), text, s.mode == m, move |_, _, cx| {
                        SettingsStore::update(cx, |s| s.mode = m)
                    })
                    .disabled(dracula)
                })
                .collect(),
            cx,
        );

        let mut section = Section::new(crate::t!(SettingsThemeTitle), crate::t!(SettingsThemeDesc))
            .field(
                format!("{palette_label} Ayu Dracula Catppuccin {custom_names} {}", crate::t!(SettingsThemeNewTheme)),
                field(&palette_label, toggles(palettes, cx), cx),
            )
            .field(
                format!("{mode_label} {}", if dracula { crate::t!(SettingsThemeDraculaHint) } else { String::new() }),
                field(&mode_label, mode_group, cx)
                    .when(dracula, |d| d.child(hint_text(crate::t!(SettingsThemeDraculaHint), cx))),
            );

        // Preview theme: the draft applied on top of the current settings.
        let mut preview_settings = s.clone();
        if let Some(t) = editing(s) {
            let name_label = crate::t!(SettingsThemeThemeName);
            section = section.field(
                name_label.clone(),
                field(&name_label, div().debug_selector(|| "settings-theme-name".into()).child(text_input(&self.appearance.theme_name)), cx),
            );
            let st = &self.appearance;
            let applied = st.draft == t.css;
            if let Some(pt) = preview_settings.custom_themes.iter_mut().find(|x| x.id == t.id) {
                pt.css = st.draft.clone();
            }
            let ignored = theme::resolve(&preview_settings, theme::apply::os_is_dark(cx)).ignored_css;
            let css_label = crate::t!(SettingsThemeCustomCss);
            let editor = div()
                .debug_selector(|| "settings-theme-css".into())
                .rounded(u(th.radius.md))
                .border_1()
                .border_color(th.colors.input)
                .bg(th.colors.background)
                .text_size(u(12.))
                .font_family(th.mono_font.clone())
                .child(
                    Editor::new(&st.css)
                        .text_size(u(12.))
                        .h(u(theme_editor::editor_height(st.css.read(cx).value().matches('\n').count() + 1)))
                        .bordered(false),
                );
            let hint_el = hint(cx)
                .child(crate::t!(SettingsThemeCustomCssHintPrefix))
                .child(code("[data-palette=\"custom\"]", cx))
                .child(crate::t!(SettingsThemeCustomCssHintSuffix))
                .child(crate::t!(SettingsThemeAutocompleteHint));
            let actions = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(u(8.))
                .child(btn("settings-theme-apply", crate::t!(CommonApply), BtnKind::Primary, None, applied, cx, {
                    let id = t.id.clone();
                    cx.listener(move |this, _, _, cx| {
                        let (id, css) = (id.clone(), this.appearance.draft.clone());
                        SettingsStore::update(cx, move |s| {
                            if let Some(t) = s.custom_themes.iter_mut().find(|t| t.id == id) {
                                t.css = css;
                            }
                        });
                    })
                }))
                .child(btn(
                    "settings-theme-duplicate",
                    crate::t!(SettingsThemeDuplicate),
                    BtnKind::Secondary,
                    Some(AppIcon::Copy),
                    false,
                    cx,
                    {
                        let name = t.name.clone();
                        cx.listener(move |this, _, _, cx| {
                            let names = theme_names(&SettingsStore::get(cx));
                            let n = files::unique_name(&name, names.iter().map(String::as_str));
                            add_theme(cx, n, this.appearance.draft.clone());
                        })
                    },
                ))
                .child(btn(
                    "settings-theme-export",
                    crate::t!(SettingsThemeExportTheme),
                    BtnKind::Secondary,
                    Some(AppIcon::Download),
                    false,
                    cx,
                    cx.listener(|this, _, window, cx| this.export_theme(window, cx)),
                ))
                .child(btn(
                    "settings-theme-delete",
                    crate::t!(CommonDelete),
                    BtnKind::Danger,
                    Some(AppIcon::Trash),
                    false,
                    cx,
                    cx.listener(|this, _, window, cx| this.delete_theme(window, cx)),
                ))
                .when(applied, |d| d.child(status(crate::t!(CommonApplied), true, cx).mt(u(0.))));
            section = section.field(
                format!("{css_label} {}", crate::t!(SettingsThemeAutocompleteHint)),
                field(&css_label, editor, cx)
                    .child(hint_el)
                    .when(!ignored.is_empty(), |d| {
                        d.child(status(crate::t!(SettingsThemeIgnoredCss, items = ignored.join("; ")), false, cx))
                    })
                    .child(actions)
                    .when_some(st.theme_status.clone(), |d, (text, ok)| d.child(status(text, ok, cx))),
            );
        } else if let Some((text, ok)) = self.appearance.theme_status.clone() {
            section = section.other(status(text, ok, cx));
        }
        let preview_theme = theme::resolve(&preview_settings, theme::apply::os_is_dark(cx));
        section.other(field(&crate::t!(CommonPreview), preview_card(&preview_theme, None, None), cx))
    }

    fn font_section(&self, s: &Settings, cx: &mut Context<Self>) -> Section {
        let st = &self.appearance;
        let th = AppTheme::get(cx).clone();
        let select = |sel: &FontSelect, id: &'static str| -> gpui_kit::AnyElement {
            match &st.fonts {
                Fonts::Loading => {
                    Select::new(sel).id(id).placeholder(crate::t!(SettingsFontLoading)).disabled(true).into_any_element()
                }
                Fonts::Failed(e) => status(crate::t!(SettingsFontError, message = e), false, cx).into_any_element(),
                Fonts::Loaded => Select::new(sel).id(id).placeholder(crate::t!(CommonDefault)).into_any_element(),
            }
        };
        let (ui_label, ed_label) = (crate::t!(SettingsFontUiFont), crate::t!(SettingsFontEditorFont));
        let (size_label, zoom_label) = (crate::t!(SettingsFontEditorFontSize), crate::t!(SettingsFontUiZoom));
        let font = |v: &str| (!v.trim().is_empty()).then(|| SharedString::from(v.trim().to_string()));
        Section::new(crate::t!(SettingsFontTitle), crate::t!(SettingsFontDesc))
            .field(ui_label.clone(), field(&ui_label, select(&st.ui_font, "settings-ui-font"), cx))
            .field(ed_label.clone(), field(&ed_label, select(&st.editor_font, "settings-editor-font"), cx))
            .field(
                format!("{size_label} {}", crate::t!(SettingsFontEditorFontSizeHint)),
                field(&size_label, div().w(u(110.)).child(NumberInput::new(&st.font_size)), cx)
                    .child(hint_text(crate::t!(SettingsFontEditorFontSizeHint), cx)),
            )
            .field(
                format!("{zoom_label} {}", crate::t!(SettingsFontUiZoomHint)),
                field(&zoom_label, div().w(u(110.)).child(NumberInput::new(&st.zoom)), cx)
                    .child(hint_text(crate::t!(SettingsFontUiZoomHint), cx)),
            )
            .other(field(&crate::t!(CommonPreview), preview_card(&th, font(&s.ui_font), font(&s.editor_font)), cx))
    }
}
