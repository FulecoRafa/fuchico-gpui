//! Theme import/export file format (spec 5 §3.6, `themeFiles.ts`).

use std::sync::LazyLock;

use regex::Regex;

static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*/\*\s*fuchico-theme:\s*(.+?)\s*\*/\s*\n?").expect("valid regex"));

/// `/* fuchico-theme: <name> */\n<css.trim()>\n`, with `*/` removed from the name.
pub fn serialize_theme(name: &str, css: &str) -> String {
    format!("/* fuchico-theme: {} */\n{}\n", name.replace("*/", ""), css.trim())
}

/// `(name, css)`; `fallback_name` (usually the file stem) when there is no header.
pub fn parse_theme_file(content: &str, fallback_name: &str) -> (String, String) {
    match HEADER.captures(content) {
        Some(c) => {
            let name = c[1].trim().to_string();
            let rest = &content[c.get(0).map_or(0, |m| m.end())..];
            let name = if name.is_empty() { fallback_name.to_string() } else { name };
            (name, rest.trim().to_string())
        }
        None => (fallback_name.to_string(), content.trim().to_string()),
    }
}

/// `My Theme!` → `my-theme.fuchico-theme.css`.
pub fn theme_file_name(name: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    format!("{}.fuchico-theme.css", if slug.is_empty() { "theme" } else { slug })
}

/// File stem with `.fuchico-theme.css` or `.css` removed (case-insensitive).
/// The original's dialogs filtered on `{ name: "CSS", extensions: ["css"] }`;
/// gpui's path prompts take no filters, so the import checks the pick
/// instead.
pub fn is_css_path(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("css"))
}

/// A save target typed without an extension gets `.css`.
pub fn with_css_extension(p: std::path::PathBuf) -> std::path::PathBuf {
    if p.extension().is_some() { p } else { p.with_extension("css") }
}

pub fn theme_stem(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = base.to_ascii_lowercase();
    for ext in [".fuchico-theme.css", ".css"] {
        if lower.ends_with(ext) {
            return base[..base.len() - ext.len()].to_string();
        }
    }
    base.to_string()
}

/// `base` if unused, else `base 2`, `base 3`, ...
pub fn unique_name<'a>(base: &str, existing: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let taken = |n: &str| existing.clone().into_iter().any(|e| e == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).expect("infinite range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = serialize_theme("Night */ Owl", "  --background: #000;\n");
        assert_eq!(s, "/* fuchico-theme: Night  Owl */\n--background: #000;\n");
        assert_eq!(parse_theme_file(&s, "x"), ("Night  Owl".into(), "--background: #000;".into()));
    }

    #[test]
    fn no_header_uses_fallback() {
        assert_eq!(parse_theme_file("  --a: 1px; ", "stem"), ("stem".into(), "--a: 1px;".into()));
    }

    #[test]
    fn file_names() {
        assert_eq!(theme_file_name("My Theme!"), "my-theme.fuchico-theme.css");
        assert_eq!(theme_file_name("***"), "theme.fuchico-theme.css");
        assert_eq!(theme_stem("/a/b/Cool.Fuchico-Theme.CSS"), "Cool");
        assert_eq!(theme_stem("x.css"), "x");
        assert_eq!(theme_stem("x.txt"), "x.txt");
    }

    #[test]
    fn unique_names() {
        let names = ["My theme", "My theme 2"];
        assert_eq!(unique_name("My theme", names.iter().copied()), "My theme 3");
        assert_eq!(unique_name("Other", names.iter().copied()), "Other");
    }

    #[test]
    fn css_filter_stand_ins() {
        assert!(is_css_path("/t/Dark.fuchico-theme.css"));
        assert!(is_css_path("/t/x.CSS"));
        assert!(!is_css_path("/t/x.md"));
        assert!(!is_css_path("/t/css"));
        assert_eq!(with_css_extension("/t/mine".into()), std::path::PathBuf::from("/t/mine.css"));
        assert_eq!(with_css_extension("/t/a.css".into()), std::path::PathBuf::from("/t/a.css"));
    }
}
