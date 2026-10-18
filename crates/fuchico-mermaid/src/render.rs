//! Source -> SVG.

use crate::theme::MermaidTheme;
use mermaid_rs_renderer::{ParseError, RenderOptions};
use std::fmt;

/// A render failure. `Display` is the text the original shows in its red
/// banner: a message that carries the line and a caret-marked snippet.
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

fn location(e: &ParseError) -> Option<(u32, Option<u32>)> {
    match e {
        ParseError::UnknownParticipant { line, .. } => Some((*line, None)),
        ParseError::UnclosedSubgraph { opened_at } => Some((*opened_at, None)),
        ParseError::UnexpectedToken { line, col, .. } => Some((*line, Some(*col))),
        ParseError::InvalidDirective { line, col, .. } => Some((*line, Some(*col))),
        _ => None,
    }
}

/// `Parse error on line N:` + the source line + a caret line, like pele's
/// `PeleError` messages; falls back to the bare message without a location.
pub fn format_error(source: &str, e: &ParseError) -> MermaidError {
    let msg = e.to_string();
    let Some((line, col)) = location(e) else {
        return MermaidError {
            message: msg,
            line: None,
        };
    };
    let text = source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("");
    let caret_at = col
        .map(|c| c.saturating_sub(1) as usize)
        .unwrap_or_else(|| text.chars().take_while(|c| c.is_whitespace()).count());
    let message = format!(
        "Parse error on line {line}:\n{text}\n{}^\n{msg}",
        " ".repeat(caret_at)
    );
    MermaidError {
        message,
        line: Some(line),
    }
}

/// Render mermaid `source` to an SVG string painted with `theme`'s colours.
pub fn render_svg(source: &str, theme: &MermaidTheme) -> Result<String, MermaidError> {
    if source.trim().is_empty() {
        return Err(MermaidError {
            message: "Empty diagram".into(),
            line: None,
        });
    }
    let mut options = RenderOptions::modern();
    options.theme = theme.to_renderer_theme();
    let src = source.to_string();
    // The layout engine is third-party; never let a panic take the UI down.
    let result =
        std::panic::catch_unwind(move || mermaid_rs_renderer::render_strict(&src, options));
    match result {
        Ok(Ok(svg)) => Ok(svg),
        Ok(Err(e)) => Err(format_error(source, &e)),
        Err(_) => Err(MermaidError {
            message: "The renderer failed on this diagram".into(),
            line: None,
        }),
    }
}
