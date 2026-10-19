//! Source -> SVG, through pele (the original's renderer).

use crate::fonts::css_family_list;
use crate::theme::MermaidTheme;
use crate::{pele, svgvars};
use std::fmt;

/// A render failure. `Display` is the text the original shows in its red
/// banner: pele's message, with the line and a caret-marked snippet when
/// the parser has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MermaidError {
    pub message: String,
    pub line: Option<u32>,
}

impl fmt::Display for MermaidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MermaidError {}

/// The `line N` pele messages carry (`Parse error on line 3:` /
/// `... at line 3`).
fn error_line(message: &str) -> Option<u32> {
    let lower = message.to_ascii_lowercase();
    let i = lower.find("line ")?;
    let digits: String = lower[i + 5..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// A rendered diagram: a self-contained SVG (no CSS variables, so resvg and
/// Typst paint it as the browser would) and its natural size in px.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagram {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    /// pele's diagram type (`flowchart`, `sequence`, ...).
    pub kind: String,
}

/// Render mermaid `source` with `theme`'s tokens.
pub fn render(source: &str, theme: &MermaidTheme) -> Result<Diagram, MermaidError> {
    let sans = css_family_list(&theme.font_sans);
    let mono = css_family_list(&theme.font_mono);
    match pele::render(source, &sans, &mono) {
        Ok(r) => {
            let mut vars = theme.pele_vars();
            for (k, v) in vars.iter_mut() {
                if matches!(*k, "--pele-font" | "--pele-font-mono") {
                    *v = css_family_list(v);
                }
            }
            Ok(Diagram {
                svg: svgvars::resolve(&r.svg, &vars),
                width: r.width,
                height: r.height,
                kind: r.kind,
            })
        }
        Err(message) => Err(MermaidError {
            line: error_line(&message),
            message,
        }),
    }
}

/// Render mermaid `source` to an SVG string painted with `theme`'s colours.
pub fn render_svg(source: &str, theme: &MermaidTheme) -> Result<String, MermaidError> {
    render(source, theme).map(|d| d.svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_error_lines() {
        assert_eq!(error_line("Parse error on line 12:\nfoo\n^"), Some(12));
        assert_eq!(
            error_line("No diagram type detected for the given text."),
            None
        );
    }
}
