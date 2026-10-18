//! [`EditorStyle`]: every colour and font the editor paints with.
//!
//! The defaults are the original app's Ayu light/dark tokens
//! (`src/styles/globals.css`), converted from oklch. The host normally
//! builds an `EditorStyle` from its own theme (all fields are public) and
//! passes it with [`crate::EditorView::set_style`].

use fuchico_markdown::{CalloutKind, ColorToken};
use gpui_kit::{Hsla, Pixels, Rgba, SharedString, px};

/// Colours (CSS variable names in the docs) and fonts of the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorStyle {
    pub dark: bool,
    /// `--font-mono` (editor text, gutter, prompt).
    pub mono_family: SharedString,
    /// `--font-sans` (tables, pills, toolbars, outline).
    pub sans_family: SharedString,
    /// `--editor-font-size` (default 13).
    pub font_size: Pixels,
    /// Body line height as a multiple of the font size (1.55).
    pub line_height: f32,
    /// `--radius-sm` / `--radius-md`.
    pub radius_sm: Pixels,
    pub radius_md: Pixels,

    pub background: Hsla,
    pub foreground: Hsla,
    pub card: Hsla,
    pub card_foreground: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub secondary: Hsla,
    pub secondary_foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    pub destructive: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    pub ring: Hsla,
    pub syntax_keyword: Hsla,
    pub syntax_string: Hsla,
    pub syntax_number: Hsla,
    pub syntax_function: Hsla,
    pub syntax_type: Hsla,
    pub syntax_comment: Hsla,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self::ayu_dark()
    }
}

/// `oklch(l c h / a)` → [`Hsla`] (CSS Color 4 formulas, gamut clipped).
pub fn oklch(l: f32, c: f32, h_deg: f32, alpha: f32) -> Hsla {
    let h = h_deg.to_radians();
    let (a_, b_) = (c * h.cos(), c * h.sin());
    let l_ = l + 0.396_337_78 * a_ + 0.215_803_76 * b_;
    let m_ = l - 0.105_561_346 * a_ - 0.063_854_17 * b_;
    let s_ = l - 0.089_484_18 * a_ - 1.291_485_5 * b_;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let b = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let gamma = |x: f32| {
        let v = if x >= 0.003_130_8 {
            1.055 * x.powf(1. / 2.4) - 0.055
        } else {
            12.92 * x
        };
        v.clamp(0., 1.)
    };
    Rgba {
        r: gamma(r),
        g: gamma(g),
        b: gamma(b),
        a: alpha.clamp(0., 1.),
    }
    .into()
}

/// `color-mix(in oklch, c p%, transparent)`: the colour at `p` opacity.
pub fn mix(c: Hsla, p: f32) -> Hsla {
    Hsla { a: c.a * p, ..c }
}

impl EditorStyle {
    /// Ayu dark (`.dark` + `[data-palette="ayu"]`).
    pub fn ayu_dark() -> Self {
        let fg = oklch(0.987, 0.002, 197.1, 1.);
        let sec = oklch(0.275, 0.011, 216.9, 1.);
        EditorStyle {
            dark: true,
            mono_family: "JetBrains Mono".into(),
            sans_family: "Inter".into(),
            font_size: px(13.),
            line_height: 1.55,
            radius_sm: px(6.),
            radius_md: px(8.),
            background: oklch(0.148, 0.004, 228.8, 1.),
            foreground: fg,
            card: oklch(0.218, 0.008, 223.9, 1.),
            card_foreground: fg,
            primary: oklch(0.925, 0.005, 214.3, 1.),
            primary_foreground: oklch(0.218, 0.008, 223.9, 1.),
            secondary: sec,
            secondary_foreground: fg,
            muted: sec,
            muted_foreground: oklch(0.723, 0.014, 214.4, 1.),
            accent: sec,
            accent_foreground: fg,
            destructive: oklch(0.704, 0.191, 22.216, 1.),
            border: oklch(1., 0., 0., 0.10),
            input: oklch(1., 0., 0., 0.15),
            ring: oklch(0.56, 0.021, 213.5, 1.),
            syntax_keyword: oklch(0.78, 0.14, 305., 1.),
            syntax_string: oklch(0.78, 0.13, 148., 1.),
            syntax_number: oklch(0.8, 0.14, 55., 1.),
            syntax_function: oklch(0.78, 0.13, 255., 1.),
            syntax_type: oklch(0.78, 0.11, 195., 1.),
            syntax_comment: oklch(0.62, 0.015, 240., 1.),
        }
    }

    /// Ayu light (`:root` + `[data-palette="ayu"]`).
    pub fn ayu_light() -> Self {
        let fg = oklch(0.148, 0.004, 228.8, 1.);
        let sec = oklch(0.963, 0.002, 197.1, 1.);
        let sec_fg = oklch(0.218, 0.008, 223.9, 1.);
        EditorStyle {
            dark: false,
            background: oklch(1., 0., 0., 1.),
            foreground: fg,
            card: oklch(1., 0., 0., 1.),
            card_foreground: fg,
            primary: oklch(0.218, 0.008, 223.9, 1.),
            primary_foreground: oklch(0.987, 0.002, 197.1, 1.),
            secondary: sec,
            secondary_foreground: sec_fg,
            muted: sec,
            muted_foreground: oklch(0.56, 0.021, 213.5, 1.),
            accent: sec,
            accent_foreground: sec_fg,
            destructive: oklch(0.577, 0.245, 27.325, 1.),
            border: oklch(0.925, 0.005, 214.3, 1.),
            input: oklch(0.925, 0.005, 214.3, 1.),
            ring: oklch(0.723, 0.014, 214.4, 1.),
            syntax_keyword: oklch(0.5, 0.19, 305., 1.),
            syntax_string: oklch(0.55, 0.14, 148., 1.),
            syntax_number: oklch(0.62, 0.16, 55., 1.),
            syntax_function: oklch(0.53, 0.17, 255., 1.),
            syntax_type: oklch(0.55, 0.13, 195., 1.),
            syntax_comment: oklch(0.6, 0.015, 240., 1.),
            ..Self::ayu_dark()
        }
    }

    /// Same palette at another font size (host `Mod±` zoom).
    pub fn with_font_size(mut self, size: Pixels) -> Self {
        self.font_size = size;
        self
    }

    /// The colour of a fuchico-markdown token.
    pub fn token(&self, t: ColorToken) -> Hsla {
        match t {
            ColorToken::Foreground => self.foreground,
            ColorToken::Muted => self.muted_foreground,
            ColorToken::Destructive => self.destructive,
            ColorToken::Primary => self.primary,
            ColorToken::SyntaxComment => self.syntax_comment,
            ColorToken::SyntaxString => self.syntax_string,
            ColorToken::SyntaxNumber => self.syntax_number,
            ColorToken::SyntaxKeyword => self.syntax_keyword,
            ColorToken::SyntaxFunction => self.syntax_function,
            ColorToken::SyntaxType => self.syntax_type,
        }
    }

    pub fn callout_accent(&self, k: CalloutKind) -> Hsla {
        self.token(k.accent())
    }

    // ---- derived colours (the original's color-mix rules) ----------------

    /// `.cm-activeLine`: foreground 5%.
    pub fn active_line(&self) -> Hsla {
        mix(self.foreground, 0.05)
    }
    /// `.cm-activeLineGutter`: foreground 6%.
    pub fn active_gutter(&self) -> Hsla {
        mix(self.foreground, 0.06)
    }
    /// `.cm-selectionBackground`: primary 30%.
    pub fn selection(&self) -> Hsla {
        mix(self.primary, 0.30)
    }
    /// Secondary selections (not distinguished by the original; slightly
    /// lighter so the primary range stands out).
    pub fn selection_secondary(&self) -> Hsla {
        mix(self.primary, 0.20)
    }
    /// Helix block cursor: codemirror-helix's
    /// `.cm-hx-block-cursor .cm-hx-cursor { background: #ccc }`, the same for
    /// every selection head, focused or not (the app's `.cm-fat-cursor` rule
    /// only applies to vim mode).
    pub fn block_cursor(&self) -> Hsla {
        Rgba {
            r: 0.8,
            g: 0.8,
            b: 0.8,
            a: 1.,
        }
        .into()
    }
    /// Secondary block cursors: identical to the primary in the original.
    pub fn block_cursor_secondary(&self) -> Hsla {
        self.block_cursor()
    }
    /// Bar cursor (`.cm-cursor { border-left: 1.2px var(--foreground) }`),
    /// primary and secondary alike.
    pub fn bar_cursor(&self) -> Hsla {
        self.foreground
    }
    /// `.cm-searchMatch` (30%) and the selected match (55%).
    pub fn search_match(&self, selected: bool) -> Hsla {
        mix(self.syntax_number, if selected { 0.55 } else { 0.30 })
    }
    /// `.cm-ruler`: foreground 12%.
    pub fn ruler(&self) -> Hsla {
        mix(self.foreground, 0.12)
    }
    /// Mode chip border (`--border` 60%), background (card 85%), text
    /// (foreground 75%).
    pub fn chip_border(&self) -> Hsla {
        mix(self.border, 0.60)
    }
    pub fn chip_background(&self) -> Hsla {
        mix(self.card, 0.85)
    }
    pub fn chip_text(&self) -> Hsla {
        mix(self.foreground, 0.75)
    }
    /// Body line height in pixels.
    pub fn line_height_px(&self) -> Pixels {
        px((f32::from(self.font_size) * self.line_height).round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(c: Hsla) -> String {
        let r: Rgba = c.into();
        let v = |x: f32| (x * 255.).round() as u8;
        format!("#{:02x}{:02x}{:02x}", v(r.r), v(r.g), v(r.b))
    }

    #[test]
    fn ayu_dark_background_matches_screenshot() {
        // globals.css `.dark --background: oklch(0.148 0.004 228.8)`.
        let s = EditorStyle::ayu_dark();
        assert_eq!(hex(s.background), "#090b0c");
        assert_eq!(hex(EditorStyle::ayu_light().background), "#ffffff");
    }

    #[test]
    fn mix_scales_alpha() {
        let s = EditorStyle::ayu_dark();
        assert!((s.selection().a - 0.30).abs() < 1e-6);
        assert!((s.border.a - 0.10).abs() < 1e-6);
    }
}
