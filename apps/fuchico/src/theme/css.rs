//! Custom-theme CSS: `--var: value;` declarations (spec 5 §4.1, §4.5).
//!
//! The original injected the text verbatim into a `:root[data-palette=custom]`
//! rule, so anything CSS accepts "worked". Here only top-level custom-property
//! declarations are honoured; comments are skipped and nested rules or other
//! properties are reported in [`ParsedCss::ignored`] so the settings UI can
//! flag them.

/// Result of [`parse_declarations`].
#[derive(Debug, Default, PartialEq)]
pub struct ParsedCss {
    /// `(--name, value)` in source order; later duplicates win when applied.
    pub decls: Vec<(String, String)>,
    /// Fragments that were not `--name: value` declarations.
    pub ignored: Vec<String>,
}

pub fn parse_declarations(css: &str) -> ParsedCss {
    let text = strip_comments(css);
    let mut out = ParsedCss::default();
    let mut depth = 0i32;
    let mut paren = 0i32;
    let mut start = 0usize;
    let mut nested_start = None;
    for (i, c) in text.char_indices() {
        match c {
            '(' => paren += 1,
            ')' => paren -= 1,
            '{' => {
                if depth == 0 {
                    nested_start = Some(start);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = nested_start.take() {
                        out.ignored.push(text[s..=i].trim().to_string());
                    }
                    start = i + 1;
                }
            }
            ';' if depth == 0 && paren == 0 => {
                push_decl(&text[start..i], &mut out);
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth == 0 {
        push_decl(&text[start..], &mut out);
    } else if let Some(s) = nested_start {
        out.ignored.push(text[s..].trim().to_string());
    }
    out
}

fn push_decl(raw: &str, out: &mut ParsedCss) {
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    match raw.split_once(':') {
        Some((name, value)) if name.trim().starts_with("--") && !value.trim().is_empty() => {
            let value = value.trim().trim_end_matches("!important").trim();
            out.decls.push((name.trim().to_string(), value.to_string()));
        }
        _ => out.ignored.push(raw.to_string()),
    }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// `--radius` value → px. Accepts `rem` (16px), `px`, `em` (16px) and bare 0.
pub fn parse_length_px(value: &str) -> Option<f32> {
    let v = value.trim();
    if let Some(n) = v.strip_suffix("rem") {
        return n.trim().parse::<f32>().ok().map(|x| x * 16.);
    }
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse().ok();
    }
    if let Some(n) = v.strip_suffix("em") {
        return n.trim().parse::<f32>().ok().map(|x| x * 16.);
    }
    match v.parse::<f32>() {
        Ok(0.) => Some(0.),
        _ => None,
    }
}

/// `--font-sans` / `--font-mono` → the first family of the stack, unquoted.
/// Generic keywords and `var(...)` yield `None` (keep the default).
pub fn first_font_family(value: &str) -> Option<String> {
    let first = value.split(',').next()?.trim();
    let name = first.trim_matches(|c| c == '"' || c == '\'').trim();
    const GENERIC: [&str; 8] = [
        "serif",
        "sans-serif",
        "monospace",
        "system-ui",
        "ui-monospace",
        "ui-sans-serif",
        "inherit",
        "initial",
    ];
    if name.is_empty() || name.starts_with("var(") || GENERIC.contains(&name.to_ascii_lowercase().as_str()) {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_declarations_and_comments() {
        let p = parse_declarations(
            "/* header */\n--background: #000;\n  --primary : oklch(0.5 0.1 30 / 50%) ;\n--x: color-mix(in oklch, red 10%, transparent)",
        );
        assert_eq!(
            p.decls,
            vec![
                ("--background".into(), "#000".into()),
                ("--primary".into(), "oklch(0.5 0.1 30 / 50%)".into()),
                ("--x".into(), "color-mix(in oklch, red 10%, transparent)".into()),
            ]
        );
        assert!(p.ignored.is_empty());
    }

    #[test]
    fn flags_non_custom_properties_and_rules() {
        let p = parse_declarations("color: red; --a: 1px; .x { --b: 2px; } --c: blue;");
        assert_eq!(p.decls.len(), 2);
        assert_eq!(p.decls[0].0, "--a");
        assert_eq!(p.decls[1].0, "--c");
        assert_eq!(p.ignored, vec!["color: red".to_string(), ".x { --b: 2px; }".to_string()]);
    }

    #[test]
    fn semicolon_inside_parens_is_kept() {
        let p = parse_declarations("--a: var(--b; x);");
        assert_eq!(p.decls[0].1, "var(--b; x)");
    }

    #[test]
    fn lengths_and_fonts() {
        assert_eq!(parse_length_px("0.625rem"), Some(10.));
        assert_eq!(parse_length_px("4px"), Some(4.));
        assert_eq!(parse_length_px("0"), Some(0.));
        assert_eq!(parse_length_px("big"), None);
        assert_eq!(first_font_family("\"Fira Code\", monospace").as_deref(), Some("Fira Code"));
        assert_eq!(first_font_family("monospace"), None);
        assert_eq!(first_font_family("var(--x)"), None);
    }
}
