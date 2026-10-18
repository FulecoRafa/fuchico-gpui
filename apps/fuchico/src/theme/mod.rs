//! Fuchico themes (spec 5 §4): palettes × light/dark, custom CSS themes, and
//! the mapping onto gpui-component's [`Theme`](gpui_kit::component::Theme).
//!
//! * [`ColorTokens`] — the 22 CSS color variables as GPUI colors.
//! * [`AppTheme`] — the resolved theme (a `Global`): colors, radii, fonts,
//!   editor font size and UI scale. App views read it with
//!   [`AppTheme::get`]; gpui-component widgets read the mapped `Theme`.
//! * [`resolve`] — pure `Settings` + OS appearance → `AppTheme`.
//! * [`apply`] — installs an `AppTheme` globally (see `apply.rs`).

pub mod apply;
pub mod color;
pub mod css;
pub mod files;
pub mod palettes;

use std::time::Duration;

use gpui_kit::{App, Global, Hsla, Rgba, SharedString};

pub use apply::init;
use color::{Color, parse_color};

use crate::settings::{ModePref, PaletteId, Settings};

macro_rules! color_tokens {
    ($($field:ident => $css:literal),+ $(,)?) => {
        /// The 22 theme color variables (`--background` ... `--syntax-comment`).
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct ColorTokens {
            $(pub $field: Hsla,)+
        }

        impl ColorTokens {
            /// CSS variable names in table order.
            pub const NAMES: [&'static str; 22] = [$($css),+];

            pub fn get(&self, css_name: &str) -> Option<Hsla> {
                match css_name {
                    $($css => Some(self.$field),)+
                    _ => None,
                }
            }

            /// Returns false for an unknown variable.
            pub fn set(&mut self, css_name: &str, value: Hsla) -> bool {
                match css_name {
                    $($css => { self.$field = value; true })+
                    _ => false,
                }
            }

            fn transparent() -> Self {
                ColorTokens { $($field: gpui_kit::transparent_black(),)+ }
            }
        }
    };
}

color_tokens! {
    background => "--background",
    foreground => "--foreground",
    card => "--card",
    card_foreground => "--card-foreground",
    primary => "--primary",
    primary_foreground => "--primary-foreground",
    secondary => "--secondary",
    secondary_foreground => "--secondary-foreground",
    muted => "--muted",
    muted_foreground => "--muted-foreground",
    accent => "--accent",
    accent_foreground => "--accent-foreground",
    destructive => "--destructive",
    border => "--border",
    input => "--input",
    ring => "--ring",
    syntax_keyword => "--syntax-keyword",
    syntax_string => "--syntax-string",
    syntax_number => "--syntax-number",
    syntax_function => "--syntax-function",
    syntax_type => "--syntax-type",
    syntax_comment => "--syntax-comment",
}

impl ColorTokens {
    /// Parses a built-in variant table.
    pub fn from_variant(variant: &palettes::Variant) -> ColorTokens {
        let mut t = ColorTokens::transparent();
        for (name, value) in variant {
            let c = parse_color(value, &|_| None)
                .unwrap_or_else(|| panic!("bad built-in color {name}: {value}"));
            t.set(name, c.to_hsla());
        }
        t
    }

    /// The colors of `palette` in `appearance`. `Custom` → the Ayu light base.
    pub fn built_in(palette: PaletteId, appearance: Appearance) -> ColorTokens {
        use palettes::*;
        let v = match (palette, appearance) {
            (PaletteId::Ayu, Appearance::Light) | (PaletteId::Custom, _) => &AYU_LIGHT,
            (PaletteId::Ayu, Appearance::Dark) => &AYU_DARK,
            (PaletteId::Dracula, _) => &DRACULA,
            (PaletteId::Catppuccin, Appearance::Light) => &CATPPUCCIN_LATTE,
            (PaletteId::Catppuccin, Appearance::Dark) => &CATPPUCCIN_MOCHA,
        };
        ColorTokens::from_variant(v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub fn is_dark(self) -> bool {
        self == Appearance::Dark
    }
}

/// Corner radii in px (`--radius` and the derived `--radius-*`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub base: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

impl Radii {
    pub fn from_base(base: f32) -> Radii {
        Radii {
            base,
            sm: base * 0.6,
            md: base * 0.8,
            lg: base,
            xl: base * 1.4,
        }
    }
}

impl Default for Radii {
    fn default() -> Self {
        Radii::from_base(10.)
    }
}

pub const DUR_FAST: Duration = Duration::from_millis(160);
pub const DUR_BASE: Duration = Duration::from_millis(240);
/// `--ease-premium`: cubic-bezier(0.16, 1, 0.3, 1).
pub const EASE_PREMIUM: [f32; 4] = [0.16, 1., 0.3, 1.];

pub const DEFAULT_UI_FONT: &str = "Inter";
pub const DEFAULT_MONO_FONT: &str = "JetBrains Mono";

/// The fully resolved theme every view paints with.
#[derive(Clone, Debug, PartialEq)]
pub struct AppTheme {
    pub palette: PaletteId,
    pub appearance: Appearance,
    pub colors: ColorTokens,
    pub radius: Radii,
    pub ui_font: SharedString,
    pub mono_font: SharedString,
    /// Editor text size in px (absolute; unaffected by UI scale).
    pub editor_font_size: f32,
    /// Whole-UI zoom factor (applied through the rem size).
    pub ui_scale: f32,
    /// Custom-theme CSS fragments that were not understood.
    pub ignored_css: Vec<String>,
}

impl Global for AppTheme {}

impl AppTheme {
    pub fn get(cx: &App) -> &AppTheme {
        cx.global::<AppTheme>()
    }

    /// The "card 85%" panel background idiom.
    pub fn panel_bg(&self) -> Hsla {
        self.colors.card.opacity_scaled(0.85)
    }

    /// Foreground at `alpha` (hover idioms: 5–6%, 9%, 10%).
    pub fn fg_alpha(&self, alpha: f32) -> Hsla {
        self.colors.foreground.opacity_scaled(alpha)
    }
}

trait OpacityScaled {
    fn opacity_scaled(self, factor: f32) -> Hsla;
}

impl OpacityScaled for Hsla {
    fn opacity_scaled(self, factor: f32) -> Hsla {
        Hsla {
            a: self.a * factor,
            ..self
        }
    }
}

/// `color-mix(in oklch, c p%, transparent)` for app code.
pub fn alpha(c: Hsla, p: f32) -> Hsla {
    c.opacity_scaled(p)
}

/// Resolves `mode` against the OS (Dracula is always dark).
pub fn resolve_appearance(palette: PaletteId, mode: ModePref, os_dark: bool) -> Appearance {
    if palette == PaletteId::Dracula {
        return Appearance::Dark;
    }
    match mode {
        ModePref::Light => Appearance::Light,
        ModePref::Dark => Appearance::Dark,
        ModePref::System if os_dark => Appearance::Dark,
        ModePref::System => Appearance::Light,
    }
}

/// Pure resolution of the visual theme from settings.
pub fn resolve(settings: &Settings, os_dark: bool) -> AppTheme {
    let appearance = resolve_appearance(settings.palette, settings.mode, os_dark);
    let mut colors = ColorTokens::built_in(settings.palette, appearance);
    let mut radius = Radii::default();
    let mut ui_font: Option<String> = None;
    let mut mono_font: Option<String> = None;
    let mut ignored = Vec::new();

    if settings.palette == PaletteId::Custom
        && let Some(theme) = settings.active_custom_theme()
    {
        let parsed = css::parse_declarations(&theme.css);
        ignored = parsed.ignored;
        let overrides = apply_custom_css(&mut colors, &parsed.decls, &mut ignored);
        if let Some(r) = overrides.radius {
            radius = Radii::from_base(r);
        }
        for (name, px) in overrides.radius_parts {
            match name.as_str() {
                "--radius-sm" => radius.sm = px,
                "--radius-md" => radius.md = px,
                "--radius-lg" => radius.lg = px,
                "--radius-xl" => radius.xl = px,
                _ => {}
            }
        }
        ui_font = overrides.font_sans;
        mono_font = overrides.font_mono;
    }

    // Settings fonts win over theme `--font-*` (inline style precedence).
    let pick = |setting: &str, theme: Option<String>, fallback: &str| -> SharedString {
        if !setting.trim().is_empty() {
            setting.trim().to_string().into()
        } else {
            theme.unwrap_or_else(|| fallback.to_string()).into()
        }
    };

    AppTheme {
        palette: settings.palette,
        appearance,
        colors,
        radius,
        ui_font: pick(&settings.ui_font, ui_font, DEFAULT_UI_FONT),
        mono_font: pick(&settings.editor_font, mono_font, DEFAULT_MONO_FONT),
        editor_font_size: crate::settings::clamp_editor_font_size(settings.editor_font_size),
        ui_scale: crate::settings::clamp_ui_scale(settings.ui_scale),
        ignored_css: ignored,
    }
}

#[derive(Default)]
struct CustomOverrides {
    radius: Option<f32>,
    radius_parts: Vec<(String, f32)>,
    font_sans: Option<String>,
    font_mono: Option<String>,
}

/// Applies `--var: value` overrides in order; `var()` sees earlier results
/// (and custom, non-theme variables declared in the same block).
fn apply_custom_css(
    colors: &mut ColorTokens,
    decls: &[(String, String)],
    ignored: &mut Vec<String>,
) -> CustomOverrides {
    let mut out = CustomOverrides::default();
    // Last declaration wins, like CSS; resolve lazily with a depth guard.
    let base = *colors;

    fn resolve_var(
        name: &str,
        decls: &[(String, String)],
        base: &ColorTokens,
        depth: u8,
    ) -> Option<Color> {
        if depth > 16 {
            return None;
        }
        match decls.iter().rev().find(|(n, _)| n == name) {
            Some((_, v)) => parse_color(v, &|n| resolve_var(n, decls, base, depth + 1)),
            None => base.get(name).map(hsla_to_color),
        }
    }

    for (name, value) in decls {
        match name.as_str() {
            "--radius" => match css::parse_length_px(value) {
                Some(px) => out.radius = Some(px),
                None => ignored.push(format!("{name}: {value}")),
            },
            "--radius-sm" | "--radius-md" | "--radius-lg" | "--radius-xl" => {
                if let Some(px) = css::parse_length_px(value) {
                    out.radius_parts.push((name.clone(), px));
                }
            }
            "--font-sans" => out.font_sans = css::first_font_family(value),
            "--font-mono" => out.font_mono = css::first_font_family(value),
            n if ColorTokens::NAMES.contains(&n) => {
                match parse_color(value, &|v| resolve_var(v, decls, &base, 0)) {
                    Some(c) => {
                        colors.set(n, c.to_hsla());
                    }
                    None => ignored.push(format!("{name}: {value}")),
                }
            }
            // Helper variables (`--my-blue`) are only used through var().
            _ => {}
        }
    }
    out
}

fn hsla_to_color(h: Hsla) -> Color {
    let r = Rgba::from(h);
    Color::rgba(r.r, r.g, r.b, r.a)
}

/// Hex of a token for tests/debug.
pub fn hex(h: Hsla) -> String {
    hsla_to_color(h).to_hex()
}

/// CSS snapshot for "New theme": every color variable of `colors`.
pub fn snapshot_css(colors: &ColorTokens) -> String {
    ColorTokens::NAMES
        .iter()
        .map(|n| {
            format!(
                "{n}: {};",
                colors.get(n).map(hex).unwrap_or_else(|| "inherit".into())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::CustomTheme;

    fn custom(css: &str) -> Settings {
        Settings {
            palette: PaletteId::Custom,
            custom_themes: vec![CustomTheme {
                id: "t".into(),
                name: "T".into(),
                css: css.into(),
            }],
            custom_theme_id: Some("t".into()),
            ..Settings::default()
        }
    }

    #[test]
    fn all_built_ins_parse() {
        for p in PaletteId::BUILT_IN {
            for a in [Appearance::Light, Appearance::Dark] {
                let t = ColorTokens::built_in(p, a);
                assert!(t.background.a > 0.99, "{p:?} {a:?}");
            }
        }
        assert_eq!(
            hex(ColorTokens::built_in(PaletteId::Dracula, Appearance::Light).background),
            "#282a36"
        );
        assert_eq!(
            hex(ColorTokens::built_in(PaletteId::Catppuccin, Appearance::Light).background),
            "#eff1f5"
        );
        assert_eq!(
            hex(ColorTokens::built_in(PaletteId::Catppuccin, Appearance::Dark).border),
            "#ffffff1a"
        );
        assert_eq!(
            hex(ColorTokens::built_in(PaletteId::Ayu, Appearance::Light).background),
            "#ffffff"
        );
    }

    #[test]
    fn mode_resolution() {
        assert_eq!(
            resolve_appearance(PaletteId::Dracula, ModePref::Light, false),
            Appearance::Dark
        );
        assert_eq!(
            resolve_appearance(PaletteId::Ayu, ModePref::System, true),
            Appearance::Dark
        );
        assert_eq!(
            resolve_appearance(PaletteId::Ayu, ModePref::System, false),
            Appearance::Light
        );
        assert_eq!(
            resolve_appearance(PaletteId::Catppuccin, ModePref::Dark, false),
            Appearance::Dark
        );
    }

    #[test]
    fn custom_theme_overrides_ayu_light_base() {
        let s = custom(
            "--background: #101010;\n--my: #ff0000;\n--primary: var(--my);\n--ring: color-mix(in oklch, var(--primary) 50%, transparent);\n--radius: 0.5rem;\n--font-mono: \"Fira Code\", monospace;\ncolor: red;",
        );
        // Not mode-aware: same result in dark.
        let t = resolve(&s, true);
        assert_eq!(t.appearance, Appearance::Dark);
        assert_eq!(hex(t.colors.background), "#101010");
        assert_eq!(hex(t.colors.primary), "#ff0000");
        assert_eq!(hex(t.colors.ring), "#ff000080");
        // Untouched vars come from Ayu light.
        assert_eq!(hex(t.colors.card), "#ffffff");
        assert_eq!(t.radius, Radii::from_base(8.));
        assert_eq!(t.mono_font.as_ref(), "Fira Code");
        assert_eq!(t.ui_font.as_ref(), DEFAULT_UI_FONT);
        assert_eq!(t.ignored_css, vec!["color: red".to_string()]);
    }

    #[test]
    fn var_cycles_do_not_hang() {
        let t = resolve(
            &custom("--primary: var(--ring);\n--ring: var(--primary);"),
            false,
        );
        assert_eq!(t.ignored_css.len(), 2);
    }

    #[test]
    fn settings_fonts_win_and_values_clamp() {
        let mut s = custom("--font-sans: Comic;");
        s.ui_font = "Avenir".into();
        s.editor_font_size = 99.;
        s.ui_scale = 0.1;
        let t = resolve(&s, false);
        assert_eq!(t.ui_font.as_ref(), "Avenir");
        assert_eq!(t.editor_font_size, 32.);
        assert_eq!(t.ui_scale, 0.7);
    }

    #[test]
    fn custom_without_theme_is_ayu_light() {
        let s = Settings {
            palette: PaletteId::Custom,
            ..Settings::default()
        };
        let t = resolve(&s, true);
        assert_eq!(
            t.colors,
            ColorTokens::built_in(PaletteId::Ayu, Appearance::Light)
        );
    }

    #[test]
    fn snapshot_round_trips() {
        let base = ColorTokens::built_in(PaletteId::Dracula, Appearance::Dark);
        let t = resolve(&custom(&snapshot_css(&base)), false);
        for n in ColorTokens::NAMES {
            assert_eq!(
                hex(t.colors.get(n).unwrap()),
                hex(base.get(n).unwrap()),
                "{n}"
            );
        }
    }
}
