//! The custom-theme CSS editor's autocomplete (spec 5 §3.5, §4.5): typing
//! `--` at a declaration start (or inside `var(`) offers every theme
//! variable with its translated description and current value (colours
//! with a swatch). On top, like the original's `cssCompletionSource`:
//! property names at a declaration start, value keywords after `:`,
//! pseudo-classes, tag names in selectors, at-rules and the `--names`
//! the sheet declares itself.

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

/// What lang-css's `cssCompletionSource` would offer at the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssSlot {
    Property,
    Value,
    PseudoClass,
    Tag,
    AtRule,
}

/// `text_before` is the whole sheet up to the cursor. Returns the byte
/// start of the word being completed and the slot, or `None`. Like lang-css
/// while typing (not an explicit request), an empty word offers nothing.
pub fn css_slot(text_before: &str) -> Option<(usize, CssSlot)> {
    let word_len: usize = text_before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .map(char::len_utf8)
        .sum();
    let start = text_before.len() - word_len;
    let word = &text_before[start..];
    let before = &text_before[..start];
    if before.ends_with('@') {
        return Some((start - 1, CssSlot::AtRule));
    }
    if word.is_empty() || word.starts_with("--") || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    // Brace depth and the current statement, comments stripped.
    let code = strip_comments(before);
    let depth = code.chars().fold(0i32, |d, c| match c {
        '{' => d + 1,
        '}' => (d - 1).max(0),
        _ => d,
    });
    let stmt_start = code.rfind(['{', '}', ';']).map_or(0, |i| i + 1);
    let stmt = &code[stmt_start..];
    if depth > 0 && !stmt.trim_start().starts_with('@') && !looks_like_nested_selector(stmt) {
        if stmt.contains(':') || stmt.contains('(') {
            return Some((start, CssSlot::Value));
        }
        return stmt.trim().is_empty().then_some((start, CssSlot::Property));
    }
    if before.ends_with(':') {
        return Some((start, CssSlot::PseudoClass));
    }
    if stmt.trim_start().starts_with('@') {
        return None;
    }
    let prev = before.chars().next_back();
    match prev {
        None | Some(' ' | '\t' | '\n' | ',' | '>' | '+' | '~' | '(' | '}' | '{' | ';') => {
            Some((start, if depth > 0 { CssSlot::Property } else { CssSlot::Tag }))
        }
        _ => None,
    }
}

/// A statement inside a block that is a nested rule's selector so far.
fn looks_like_nested_selector(stmt: &str) -> bool {
    let t = stmt.trim_start();
    t.starts_with('&') || t.starts_with('.') || t.starts_with('#') || t.starts_with('[')
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `(label, kind, text inserted)` for a slot, filtered by `prefix`.
pub fn css_options(slot: CssSlot, prefix: &str) -> Vec<(String, CompletionItemKind, String)> {
    use super::css_data as d;
    let pick = |list: &[&str], kind: CompletionItemKind, suffix: &str| -> Vec<(String, CompletionItemKind, String)> {
        list.iter()
            .filter(|s| s.starts_with(prefix))
            .map(|s| (s.to_string(), kind, format!("{s}{suffix}")))
            .collect()
    };
    match slot {
        CssSlot::Property => pick(&d::PROPERTIES, CompletionItemKind::PROPERTY, ": "),
        CssSlot::Value => {
            let mut v = pick(&d::VALUES, CompletionItemKind::KEYWORD, "");
            v.extend(pick(&d::COLORS, CompletionItemKind::CONSTANT, ""));
            v
        }
        CssSlot::PseudoClass => pick(&d::PSEUDO_CLASSES, CompletionItemKind::CLASS, ""),
        CssSlot::Tag => pick(&d::TAGS, CompletionItemKind::TYPE_PARAMETER, ""),
        CssSlot::AtRule => pick(&d::AT_RULES, CompletionItemKind::KEYWORD, ""),
    }
}

/// `--names` the sheet declares (`--x:`) that aren't theme variables
/// (lang-css's `variableNames`).
pub fn declared_variables(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("--") {
        let tail = &rest[i..];
        let len = tail.char_indices().find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-')).map_or(tail.len(), |(j, _)| j);
        let name = &tail[..len];
        if len > 2
            && tail[len..].trim_start().starts_with(':')
            && !THEME_VARIABLES.iter().any(|v| v.0 == name)
            && !out.iter().any(|n| n == name)
        {
            out.push(name.to_string());
        }
        rest = &tail[len.max(2)..];
    }
    out
}

/// The completion info for a theme variable: a colour swatch (a `<mark>`
/// filled with the colour, which the completion popover's markdown renders)
/// and the value.
pub fn variable_info(value: &str, is_color: bool) -> String {
    if is_color && value.starts_with('#') {
        format!("<mark color=\"{value}\">\u{2003}\u{2003}</mark> {value}")
    } else {
        value.to_string()
    }
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
        let edit = |start: usize, new_text: String| {
            Some(CompletionTextEdit::Edit(TextEdit {
                range: lsp_types::Range { start: rope.offset_to_position(start), end: pos },
                new_text,
            }))
        };
        let mut items: Vec<CompletionItem> = Vec::new();
        if let Some((start, in_var)) = completion_context(&line_before) {
            let prefix = line_before[start..].to_string();
            let th = AppTheme::get(cx);
            items.extend(
                THEME_VARIABLES
                    .iter()
                    .filter(|(name, _, _)| name.starts_with(prefix.as_str()) || prefix == "-")
                    .map(|(name, is_color, msg)| {
                        let info = variable_info(&current_value(th, name), *is_color);
                        CompletionItem {
                            label: name.to_string(),
                            filter_text: Some(name.to_string()),
                            kind: Some(if *is_color { CompletionItemKind::COLOR } else { CompletionItemKind::VARIABLE }),
                            detail: Some(tr(*msg, &[])),
                            documentation: (!info.is_empty()).then_some(Documentation::String(info)),
                            text_edit: edit(line_start + start, completion_text(name, in_var)),
                            ..Default::default()
                        }
                    }),
            );
            let text = rope.to_string();
            items.extend(declared_variables(&text).into_iter().filter(|n| n.starts_with(prefix.as_str()) || prefix == "-").map(
                |name| CompletionItem {
                    label: name.clone(),
                    filter_text: Some(name.clone()),
                    kind: Some(CompletionItemKind::VARIABLE),
                    text_edit: edit(line_start + start, completion_text(&name, in_var)),
                    ..Default::default()
                },
            ));
        } else {
            let text_before = rope.slice(0..offset).to_string();
            if let Some((start, slot)) = css_slot(&text_before) {
                let prefix = &text_before[start..];
                items.extend(css_options(slot, prefix).into_iter().map(|(label, kind, insert)| CompletionItem {
                    filter_text: Some(label.clone()),
                    label,
                    kind: Some(kind),
                    text_edit: edit(start, insert),
                    ..Default::default()
                }));
            }
        }
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text.chars().all(|c| c == '-' || c == '@' || c.is_alphanumeric())
    }
}

/// The original's `.cm-content { min-height: 160px }` and `maxHeight="360px"`:
/// the editor grows with its lines (12px text, 1.5 line height) in between,
/// then scrolls.
pub const EDITOR_MIN_HEIGHT: f32 = 160.;
pub const EDITOR_MAX_HEIGHT: f32 = 360.;

pub fn editor_height(lines: usize) -> f32 {
    const ROW: f32 = 12. * 1.5;
    const PADDING: f32 = 8.;
    (lines as f32 * ROW + PADDING).clamp(EDITOR_MIN_HEIGHT, EDITOR_MAX_HEIGHT)
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
    fn css_slots_like_lang_css() {
        let slot = |s: &str| css_slot(s).map(|(_, k)| k);
        assert_eq!(slot("body {\n  col"), Some(CssSlot::Property));
        assert_eq!(slot("a { color: red; back"), Some(CssSlot::Property));
        assert_eq!(slot("a { color: tra"), Some(CssSlot::Value));
        assert_eq!(slot("a { width: calc(1px + au"), Some(CssSlot::Value));
        assert_eq!(slot("a:hov"), Some(CssSlot::PseudoClass));
        assert_eq!(slot("a::bef"), Some(CssSlot::PseudoClass));
        assert_eq!(slot("di"), Some(CssSlot::Tag));
        assert_eq!(slot("a {} p > sp"), Some(CssSlot::Tag));
        assert_eq!(slot("@med"), Some(CssSlot::AtRule));
        assert_eq!(css_slot("@med").unwrap().0, 0);
        assert_eq!(slot("/* { */ di"), Some(CssSlot::Tag), "comments don't open blocks");
        assert_eq!(slot("a { color: red; "), None, "empty word");
        assert_eq!(slot("a { --pri"), None, "variables are the theme source's");
        assert_eq!(slot(".cls"), None);
        assert_eq!(slot("a { margin: 1"), None);
    }

    #[test]
    fn css_option_lists() {
        let labels = |slot, p| css_options(slot, p).into_iter().map(|o| o.0).collect::<Vec<_>>();
        let props = css_options(CssSlot::Property, "backgr");
        assert!(props.iter().any(|o| o.0 == "background-color" && o.2 == "background-color: "));
        assert!(labels(CssSlot::Value, "transp").contains(&"transparent".to_string()));
        assert!(labels(CssSlot::Value, "rebecca").contains(&"rebeccapurple".to_string()));
        assert!(labels(CssSlot::PseudoClass, "focus-v").contains(&"focus-visible".to_string()));
        assert_eq!(labels(CssSlot::AtRule, "@med"), ["@media"]);
        assert!(labels(CssSlot::Tag, "blockq").contains(&"blockquote".to_string()));
    }

    #[test]
    fn sheet_variables_and_swatch() {
        let css = "[data-palette=\"custom\"] { --primary: #fff; --my-gap: 4px; --my-gap: 5px; } a { x: var(--my-gap) }";
        assert_eq!(declared_variables(css), ["--my-gap"]);
        assert_eq!(variable_info("#112233", true), "<mark color=\"#112233\">\u{2003}\u{2003}</mark> #112233");
        assert_eq!(variable_info("6px", false), "6px");
    }

    #[test]
    fn editor_grows_between_160_and_360() {
        assert_eq!(editor_height(1), 160.);
        assert_eq!(editor_height(12), 12. * 18. + 8.);
        assert_eq!(editor_height(500), 360.);
    }

    #[test]
    fn inserted_text() {
        assert_eq!(completion_text("--ring", false), "--ring: ");
        assert_eq!(completion_text("--ring", true), "--ring");
    }
}
