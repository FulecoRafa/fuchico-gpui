//! The custom-theme CSS editor's autocomplete (spec 5 §3.5, §4.5): typing
//! `--` at a declaration start (or inside `var(`) offers every theme
//! variable with its translated description and current value.

use std::rc::Rc;

use gpui_kit::component::input::{CompletionProvider, EditorState, Rope, RopeExt};
use gpui_kit::{App, Context, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit, Documentation,
    TextEdit,
};

use crate::i18n::{Msg, tr};
use crate::theme::AppTheme;

/// `THEME_VARIABLES`: name, whether it is a color, description.
pub const THEME_VARIABLES: [(&str, bool, Msg); 25] = [
    ("--background", true, Msg::ThemeVarBackground),
    ("--foreground", true, Msg::ThemeVarForeground),
    ("--card", true, Msg::ThemeVarCard),
    ("--card-foreground", true, Msg::ThemeVarCardForeground),
    ("--primary", true, Msg::ThemeVarPrimary),
    ("--primary-foreground", true, Msg::ThemeVarPrimaryForeground),
    ("--secondary", true, Msg::ThemeVarSecondary),
    ("--secondary-foreground", true, Msg::ThemeVarSecondaryForeground),
    ("--muted", true, Msg::ThemeVarMuted),
    ("--muted-foreground", true, Msg::ThemeVarMutedForeground),
    ("--accent", true, Msg::ThemeVarAccent),
    ("--accent-foreground", true, Msg::ThemeVarAccentForeground),
    ("--destructive", true, Msg::ThemeVarDestructive),
    ("--border", true, Msg::ThemeVarBorder),
    ("--input", true, Msg::ThemeVarInput),
    ("--ring", true, Msg::ThemeVarRing),
    ("--syntax-keyword", true, Msg::ThemeVarSyntaxKeyword),
    ("--syntax-string", true, Msg::ThemeVarSyntaxString),
    ("--syntax-number", true, Msg::ThemeVarSyntaxNumber),
    ("--syntax-function", true, Msg::ThemeVarSyntaxFunction),
    ("--syntax-type", true, Msg::ThemeVarSyntaxType),
    ("--syntax-comment", true, Msg::ThemeVarSyntaxComment),
    ("--radius", false, Msg::ThemeVarRadius),
    ("--font-sans", false, Msg::ThemeVarFontSans),
    ("--font-mono", false, Msg::ThemeVarFontMono),
];

/// Where a completion applies, from the text of the line before the cursor:
/// `(byte start of the `-`-word, inside var())`. `None` = not offered.
pub fn completion_context(line_before: &str) -> Option<(usize, bool)> {
    let word_len: usize = line_before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .map(char::len_utf8)
        .sum();
    let start = line_before.len() - word_len;
    let word = &line_before[start..];
    if !word.starts_with('-') {
        return None;
    }
    let before = &line_before[..start];
    if before.trim_end().ends_with("var(") {
        return Some((start, true));
    }
    before.trim().is_empty().then_some((start, false))
}

/// The text a completion inserts.
pub fn completion_text(name: &str, in_var: bool) -> String {
    if in_var { name.to_string() } else { format!("{name}: ") }
}

/// Current value of a theme variable in the live theme (completion info).
fn current_value(th: &AppTheme, name: &str) -> String {
    if let Some(c) = th.colors.get(name) {
        return crate::theme::hex(c);
    }
    match name {
        "--radius" => format!("{}px", th.radius.base),
        "--font-sans" => th.ui_font.to_string(),
        "--font-mono" => th.mono_font.to_string(),
        _ => String::new(),
    }
}

struct ThemeVarCompletions;

impl CompletionProvider for ThemeVarCompletions {
    fn completions(
        &self,
        rope: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let pos = rope.offset_to_position(offset);
        let line_start = rope.line_start_offset(pos.line as usize);
        let line_before = rope.slice(line_start..offset).to_string();
        let Some((start, in_var)) = completion_context(&line_before) else {
            return Task::ready(Ok(CompletionResponse::Array(vec![])));
        };
        let prefix = line_before[start..].to_string();
        let range = lsp_types::Range { start: rope.offset_to_position(line_start + start), end: pos };
        let th = AppTheme::get(cx);
        let items = THEME_VARIABLES
            .iter()
            .filter(|(name, _, _)| name.starts_with(prefix.as_str()) || prefix == "-")
            .map(|(name, is_color, msg)| {
                let value = current_value(th, name);
                let info = if *is_color { format!("■ {value}") } else { value };
                CompletionItem {
                    label: name.to_string(),
                    filter_text: Some(name.to_string()),
                    kind: Some(if *is_color { CompletionItemKind::COLOR } else { CompletionItemKind::VARIABLE }),
                    detail: Some(tr(*msg, &[])),
                    documentation: (!info.is_empty()).then_some(Documentation::String(info)),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range,
                        new_text: completion_text(name, in_var),
                    })),
                    ..Default::default()
                }
            })
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text.chars().all(|c| c == '-' || c.is_alphanumeric())
    }
}

/// The CSS editor state: line numbers, no folding, soft wrap, completions.
pub fn new_css_editor(window: &mut Window, cx: &mut Context<EditorState>) -> EditorState {
    let mut state =
        EditorState::new(window, cx).language("css").line_number(true).folding(false).soft_wrap(true);
    state.lsp_mut().completion_provider = Some(Rc::new(ThemeVarCompletions));
    state
}

/// Only the color variables are snapshot by "New theme".
pub fn color_variable_count() -> usize {
    THEME_VARIABLES.iter().filter(|v| v.1).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorTokens;

    #[test]
    fn color_vars_match_tokens() {
        assert_eq!(color_variable_count(), ColorTokens::NAMES.len());
        for (name, is_color, _) in THEME_VARIABLES {
            assert_eq!(is_color, ColorTokens::NAMES.contains(&name), "{name}");
        }
    }

    #[test]
    fn context_at_declaration_start() {
        assert_eq!(completion_context("  --bac"), Some((2, false)));
        assert_eq!(completion_context("-"), Some((0, false)));
        assert_eq!(completion_context("--"), Some((0, false)));
    }

    #[test]
    fn context_inside_var() {
        let line = "  --primary: var(--acc";
        assert_eq!(completion_context(line), Some((17, true)));
        assert_eq!(completion_context("--a: var( --"), Some((10, true)));
    }

    #[test]
    fn no_context_elsewhere() {
        assert_eq!(completion_context("  --primary: #fff"), None);
        assert_eq!(completion_context("color: --x"), None);
        assert_eq!(completion_context("  back"), None);
        assert_eq!(completion_context(""), None);
    }

    #[test]
    fn inserted_text() {
        assert_eq!(completion_text("--ring", false), "--ring: ");
        assert_eq!(completion_text("--ring", true), "--ring");
    }
}
