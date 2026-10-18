//! Installing an [`AppTheme`]: the `AppTheme` global for app views plus the
//! mapping onto gpui-component's `Theme` for library widgets (buttons,
//! inputs, menus, dialogs, notifications, scrollbars).
//!
//! UI scale: gpui-component's root sets the window rem size from
//! `Theme::font_size` every frame, so `font_size = 16px × uiScale` scales
//! every rem-based length. App layout is written in rems (see
//! [`crate::ui::style::u`]); the editor font size stays absolute px, which is
//! the original's `--editor-font-size = editorFontSize / uiScale` trick.

use gpui_kit::component::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, WindowAppearance, black, px};

use super::{AppTheme, ColorTokens, resolve};
use crate::settings::SettingsStore;

/// Whether the OS is in dark mode.
pub fn os_is_dark(cx: &App) -> bool {
    matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark)
}

/// Resolves from the current settings and OS appearance and installs it.
/// Call after settings change and on OS appearance changes. Cheap no-op when
/// nothing changed.
pub fn refresh(cx: &mut App) {
    let theme = resolve(&SettingsStore::get(cx), os_is_dark(cx));
    if cx.try_global::<AppTheme>() == Some(&theme) {
        return;
    }
    apply(theme, cx);
}

/// Registers the settings observer and applies the initial theme.
pub fn init(cx: &mut App) {
    refresh(cx);
    cx.observe_global::<SettingsStore>(refresh).detach();
}

pub fn apply(theme: AppTheme, cx: &mut App) {
    let mode = if theme.appearance.is_dark() { ThemeMode::Dark } else { ThemeMode::Light };
    if Theme::global(cx).mode != mode {
        // Switching mode reloads the registered config, which would clobber
        // edits made in the same closure; switch first, then edit.
        Theme::change(mode, None, cx);
    }
    let t = theme.clone();
    Theme::update(cx, move |gt| {
        map_colors(&t.colors, &mut gt.colors);
        gt.font_family = t.ui_font.clone();
        gt.mono_font_family = t.mono_font.clone();
        gt.font_size = px(16. * t.ui_scale);
        gt.mono_font_size = px(t.editor_font_size);
        gt.radius = px(t.radius.sm * t.ui_scale);
        gt.radius_lg = px(t.radius.md * t.ui_scale);
        gt.shadow = true;
    });
    cx.set_global(theme);
    cx.refresh_windows();
}

fn map_colors(c: &ColorTokens, t: &mut ThemeColor) {
    let panel = c.card.opacity(0.85);
    let hover = c.foreground.opacity(0.06);
    let primary_hover = c.primary.opacity(0.8);
    let on_danger: Hsla = c.background;

    t.background = c.background;
    t.foreground = c.foreground;
    t.border = c.border;
    t.input = c.input;
    t.ring = c.ring;
    t.caret = c.foreground;
    t.selection = c.ring.opacity(0.35);
    t.accent = c.accent;
    t.accent_foreground = c.accent_foreground;
    t.muted = c.muted;
    t.muted_foreground = c.muted_foreground;
    t.popover = c.card;
    t.popover_foreground = c.card_foreground;

    t.primary = c.primary;
    t.primary_hover = primary_hover;
    t.primary_active = c.primary;
    t.primary_foreground = c.primary_foreground;
    t.secondary = c.secondary;
    t.secondary_hover = c.secondary.opacity(0.8);
    t.secondary_active = c.secondary;
    t.secondary_foreground = c.secondary_foreground;
    t.danger = c.destructive;
    t.danger_hover = c.destructive.opacity(0.85);
    t.danger_active = c.destructive;
    t.danger_foreground = on_danger;

    t.button = c.secondary;
    t.button_hover = c.secondary.opacity(0.8);
    t.button_active = c.secondary;
    t.button_foreground = c.secondary_foreground;
    t.button_primary = c.primary;
    t.button_primary_hover = primary_hover;
    t.button_primary_active = c.primary;
    t.button_primary_foreground = c.primary_foreground;
    t.button_secondary = c.secondary;
    t.button_secondary_hover = c.secondary.opacity(0.8);
    t.button_secondary_active = c.secondary;
    t.button_secondary_foreground = c.secondary_foreground;
    t.button_danger = c.destructive;
    t.button_danger_hover = c.destructive.opacity(0.85);
    t.button_danger_active = c.destructive;
    t.button_danger_foreground = on_danger;

    t.sidebar = panel;
    t.sidebar_foreground = c.foreground;
    t.sidebar_border = c.border;
    t.sidebar_accent = c.accent;
    t.sidebar_accent_foreground = c.accent_foreground;
    t.sidebar_primary = c.primary;
    t.sidebar_primary_foreground = c.primary_foreground;

    t.tab_bar = panel;
    t.tab = gpui_kit::transparent_black();
    t.tab_active = c.background;
    t.tab_foreground = c.muted_foreground;
    t.tab_active_foreground = c.foreground;

    t.list = gpui_kit::transparent_black();
    t.list_hover = c.accent.opacity(0.7);
    t.list_active = c.accent;
    t.list_active_border = c.ring;
    t.list_even = gpui_kit::transparent_black();
    t.list_head = c.card;
    t.table = c.background;
    t.table_hover = hover;
    t.table_active = c.accent;
    t.table_head = c.card;
    t.table_head_foreground = c.muted_foreground;
    t.table_row_border = c.border;

    t.scrollbar = gpui_kit::transparent_black();
    t.scrollbar_thumb = c.foreground.opacity(0.18);
    t.scrollbar_thumb_hover = c.foreground.opacity(0.3);
    t.status_bar = panel;
    t.status_bar_border = c.border;
    t.title_bar = panel;
    t.title_bar_border = c.border;
    t.window_border = c.border;
    t.overlay = black().opacity(0.25);
    t.drag_border = c.ring;
    t.drop_target = c.primary.opacity(0.15);

    t.link = c.syntax_function;
    t.link_hover = c.syntax_function.opacity(0.8);
    t.link_active = c.syntax_function;

    t.success = c.syntax_string;
    t.success_hover = c.syntax_string.opacity(0.85);
    t.success_active = c.syntax_string;
    t.success_foreground = on_danger;
    t.warning = c.syntax_number;
    t.warning_hover = c.syntax_number.opacity(0.85);
    t.warning_active = c.syntax_number;
    t.warning_foreground = on_danger;
    t.info = c.syntax_function;
    t.info_hover = c.syntax_function.opacity(0.85);
    t.info_active = c.syntax_function;
    t.info_foreground = on_danger;

    t.red = c.destructive;
    t.red_light = c.destructive.opacity(0.2);
    t.green = c.syntax_string;
    t.green_light = c.syntax_string.opacity(0.2);
    t.blue = c.syntax_function;
    t.blue_light = c.syntax_function.opacity(0.2);
    t.yellow = c.syntax_number;
    t.yellow_light = c.syntax_number.opacity(0.2);
    t.magenta = c.syntax_keyword;
    t.magenta_light = c.syntax_keyword.opacity(0.2);
    t.cyan = c.syntax_type;

    t.chart_1 = c.syntax_function;
    t.chart_2 = c.syntax_string;
    t.chart_3 = c.syntax_number;
    t.chart_4 = c.syntax_keyword;
    t.chart_5 = c.syntax_type;

    t.switch = c.input;
    t.switch_thumb = c.background;
    t.slider_bar = c.primary;
    t.slider_thumb = c.primary_foreground;
    t.progress_bar = c.primary;
    t.skeleton = c.muted;
    t.accordion = c.background;
    t.group_box = c.card;
    t.group_box_foreground = c.card_foreground;
}
