//! Theme tokens and their mapping onto the diagram renderer.
//!
//! The original app paints diagrams from `--pele-*` CSS variables defined in
//! `globals.css` on `.mermaid-canvas-content`:
//!
//! ```text
//! --pele-bg:var(--background)        --pele-fg:var(--foreground)
//! --pele-muted:var(--muted-foreground) --pele-line:var(--muted-foreground)
//! --pele-surface:var(--card)         --pele-surface-alt:var(--secondary)
//! --pele-border:var(--ring)          --pele-accent:var(--syntax-function)
//! --pele-error/--pele-critical:var(--destructive)
//! --pele-font:var(--font-sans)       --pele-font-mono:var(--font-mono)
//! --pele-radius:var(--radius-sm)
//! --pele-series-1..8: syntax-function, -string, -number, -keyword, -type,
//!                     destructive, syntax-comment, primary
//! ```
//!
//! [`MermaidTheme`] holds the app tokens (any CSS colour string: hex, `rgb()`,
//! `oklch()`), [`MermaidTheme::pele_vars`] resolves exactly that variable
//! table, and [`MermaidTheme::to_renderer_theme`] maps it onto
//! `mermaid-rs-renderer`'s theme.

use crate::color::{self, Rgba};
use mermaid_rs_renderer::Theme;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MermaidTheme {
    pub background: String,
    pub foreground: String,
    pub card: String,
    pub primary: String,
    pub secondary: String,
    pub muted_foreground: String,
    pub destructive: String,
    pub border: String,
    pub ring: String,
    pub accent: String,
    pub syntax_keyword: String,
    pub syntax_string: String,
    pub syntax_number: String,
    pub syntax_function: String,
    pub syntax_type: String,
    pub syntax_comment: String,
    pub font_sans: String,
    pub font_mono: String,
    /// `--radius-sm`, in px.
    pub radius_sm: u32,
    pub dark: bool,
}

const FONT_SANS: &str = "Inter Variable, Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif";
const FONT_MONO: &str = "JetBrains Mono, ui-monospace, SFMono-Regular, monospace";

impl MermaidTheme {
    /// Default light tokens (`:root` in `globals.css`).
    pub fn light() -> Self {
        let s = |v: &str| v.to_string();
        Self {
            background: s("oklch(1 0 0)"),
            foreground: s("oklch(0.148 0.004 228.8)"),
            card: s("oklch(1 0 0)"),
            primary: s("oklch(0.218 0.008 223.9)"),
            secondary: s("oklch(0.963 0.002 197.1)"),
            muted_foreground: s("oklch(0.56 0.021 213.5)"),
            destructive: s("oklch(0.577 0.245 27.325)"),
            border: s("oklch(0.925 0.005 214.3)"),
            ring: s("oklch(0.723 0.014 214.4)"),
            accent: s("oklch(0.963 0.002 197.1)"),
            syntax_keyword: s("oklch(0.5 0.19 305)"),
            syntax_string: s("oklch(0.55 0.14 148)"),
            syntax_number: s("oklch(0.62 0.16 55)"),
            syntax_function: s("oklch(0.53 0.17 255)"),
            syntax_type: s("oklch(0.55 0.13 195)"),
            syntax_comment: s("oklch(0.6 0.015 240)"),
            font_sans: s(FONT_SANS),
            font_mono: s(FONT_MONO),
            radius_sm: 6,
            dark: false,
        }
    }

    /// Default dark tokens (`:root.dark` / `prefers-color-scheme: dark`).
    pub fn dark() -> Self {
        let s = |v: &str| v.to_string();
        Self {
            background: s("oklch(0.148 0.004 228.8)"),
            foreground: s("oklch(0.987 0.002 197.1)"),
            card: s("oklch(0.218 0.008 223.9)"),
            primary: s("oklch(0.925 0.005 214.3)"),
            secondary: s("oklch(0.275 0.011 216.9)"),
            muted_foreground: s("oklch(0.723 0.014 214.4)"),
            destructive: s("oklch(0.704 0.191 22.216)"),
            border: s("oklch(1 0 0 / 10%)"),
            ring: s("oklch(0.56 0.021 213.5)"),
            accent: s("oklch(0.275 0.011 216.9)"),
            syntax_keyword: s("oklch(0.78 0.14 305)"),
            syntax_string: s("oklch(0.78 0.13 148)"),
            syntax_number: s("oklch(0.8 0.14 55)"),
            syntax_function: s("oklch(0.78 0.13 255)"),
            syntax_type: s("oklch(0.78 0.11 195)"),
            syntax_comment: s("oklch(0.62 0.015 240)"),
            dark: true,
            ..Self::light()
        }
    }

    /// Resolve a token to sRGB; unparseable values fall back to magenta so a
    /// bad token is visible instead of silently invisible.
    pub fn resolve(&self, css: &str) -> Rgba {
        color::parse(css).unwrap_or(Rgba::rgb(255, 0, 255))
    }

    /// The `--pele-*` variable table, colours resolved to sRGB.
    pub fn pele_vars(&self) -> Vec<(&'static str, String)> {
        let c = |v: &str| self.resolve(v).css();
        vec![
            ("--pele-bg", c(&self.background)),
            ("--pele-fg", c(&self.foreground)),
            ("--pele-muted", c(&self.muted_foreground)),
            ("--pele-line", c(&self.muted_foreground)),
            ("--pele-surface", c(&self.card)),
            ("--pele-surface-alt", c(&self.secondary)),
            ("--pele-border", c(&self.ring)),
            ("--pele-accent", c(&self.syntax_function)),
            ("--pele-error", c(&self.destructive)),
            ("--pele-critical", c(&self.destructive)),
            ("--pele-font", self.font_sans.clone()),
            ("--pele-font-mono", self.font_mono.clone()),
            ("--pele-radius", format!("{}px", self.radius_sm)),
            ("--pele-series-1", c(&self.syntax_function)),
            ("--pele-series-2", c(&self.syntax_string)),
            ("--pele-series-3", c(&self.syntax_number)),
            ("--pele-series-4", c(&self.syntax_keyword)),
            ("--pele-series-5", c(&self.syntax_type)),
            ("--pele-series-6", c(&self.destructive)),
            ("--pele-series-7", c(&self.syntax_comment)),
            ("--pele-series-8", c(&self.primary)),
        ]
    }

    /// The eight categorical series colours (`--pele-series-1..8`).
    pub fn series(&self) -> [Rgba; 8] {
        [
            self.resolve(&self.syntax_function),
            self.resolve(&self.syntax_string),
            self.resolve(&self.syntax_number),
            self.resolve(&self.syntax_keyword),
            self.resolve(&self.syntax_type),
            self.resolve(&self.destructive),
            self.resolve(&self.syntax_comment),
            self.resolve(&self.primary),
        ]
    }

    /// Map the pele variable table onto `mermaid-rs-renderer`'s theme.
    ///
    /// bg -> background; fg -> text colours; surface -> node fill;
    /// surface-alt -> secondary/cluster fill; border -> node/cluster/actor
    /// strokes; line/muted -> edges and lifelines; accent -> sequence note
    /// tint; series 1-8 -> pie and git-graph palettes.
    pub fn to_renderer_theme(&self) -> Theme {
        let mut t = Theme::modern();
        let bg = self.resolve(&self.background);
        let fg = self.resolve(&self.foreground).css();
        let line = self.resolve(&self.muted_foreground).css();
        let surface = self.resolve(&self.card).css();
        let surface_alt = self.resolve(&self.secondary).css();
        let border = self.resolve(&self.ring).css();
        let accent_c = self.resolve(&self.syntax_function);
        let note = accent_c.mix_over(bg, 0.14).css();
        let series = self.series();

        t.font_family = format!("{}, sans-serif", self.font_sans);
        t.background = "transparent".to_string();
        t.text_color = fg.clone();
        t.primary_text_color = fg.clone();
        t.primary_color = surface.clone();
        t.primary_border_color = border.clone();
        t.line_color = line.clone();
        t.secondary_color = surface_alt.clone();
        t.tertiary_color = surface_alt.clone();
        t.edge_label_background = bg.css();
        t.cluster_background = surface_alt.clone();
        t.cluster_border = border.clone();
        t.sequence_actor_fill = surface.clone();
        t.sequence_actor_border = border.clone();
        t.sequence_actor_line = line.clone();
        t.sequence_note_fill = note;
        t.sequence_note_border = accent_c.css();
        t.sequence_activation_fill = surface_alt;
        t.sequence_activation_border = border.clone();

        for i in 0..12 {
            t.pie_colors[i] = series[i % 8].css();
        }
        t.pie_title_text_color = fg.clone();
        t.pie_section_text_color = bg.css();
        t.pie_legend_text_color = fg.clone();
        t.pie_stroke_color = bg.css();
        t.pie_outer_stroke_color = border;
        t.pie_opacity = 1.0;
        for (i, c) in series.iter().enumerate().take(8) {
            t.git_colors[i] = c.css();
            t.git_inv_colors[i] = bg.css();
            t.git_branch_label_colors[i] = bg.css();
        }
        t.git_commit_label_color = fg;
        t.git_commit_label_background = self.resolve(&self.card).css();
        t.git_tag_label_color = self.resolve(&self.foreground).css();
        t.git_tag_label_background = t.primary_color.clone();
        t.git_tag_label_border = t.primary_border_color.clone();
        t
    }
}

impl Default for MermaidTheme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var<'a>(v: &'a [(&'static str, String)], k: &str) -> &'a str {
        &v.iter().find(|(n, _)| *n == k).unwrap().1
    }

    #[test]
    fn pele_table_matches_globals_css() {
        let t = MermaidTheme::light();
        let v = t.pele_vars();
        assert_eq!(v.len(), 21);
        assert_eq!(var(&v, "--pele-bg"), "#ffffff");
        assert_eq!(var(&v, "--pele-line"), var(&v, "--pele-muted"));
        assert_eq!(var(&v, "--pele-error"), var(&v, "--pele-critical"));
        assert_eq!(var(&v, "--pele-series-1"), var(&v, "--pele-accent"));
        assert_eq!(var(&v, "--pele-series-6"), var(&v, "--pele-error"));
        assert_eq!(var(&v, "--pele-radius"), "6px");
    }

    #[test]
    fn dark_differs_and_maps_to_renderer() {
        let l = MermaidTheme::light().to_renderer_theme();
        let d = MermaidTheme::dark().to_renderer_theme();
        assert_ne!(l.text_color, d.text_color);
        assert_eq!(d.primary_color, MermaidTheme::dark().resolve(&MermaidTheme::dark().card).css());
        assert_eq!(l.pie_colors[8], l.pie_colors[0]);
        assert_eq!(l.line_color, MermaidTheme::light().resolve("oklch(0.56 0.021 213.5)").css());
    }

    #[test]
    fn custom_tokens_flow_through() {
        let mut t = MermaidTheme::light();
        t.card = "#112233".into();
        t.ring = "#445566".into();
        let r = t.to_renderer_theme();
        assert_eq!(r.primary_color, "#112233");
        assert_eq!(r.primary_border_color, "#445566");
        assert_eq!(r.cluster_border, "#445566");
    }
}
