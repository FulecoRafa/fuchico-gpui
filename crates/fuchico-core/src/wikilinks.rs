//! Pure parts of `src/modules/editor/lib/wikilinks.ts`: path helpers, link
//! resolution, `[[...]]` scanning and the `[[` completion list.

use std::sync::LazyLock;

use regex::Regex;

use crate::fuzzy::fuzzy_match;

fn dirname_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) if i > 0 => &path[..i],
        _ => "",
    }
}

pub fn basename_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

pub fn basename_no_ext(path: &str) -> &str {
    let base = basename_of(path);
    match base.rfind('.') {
        Some(dot) if dot > 0 => &base[..dot],
        _ => base,
    }
}

/// Joins `relative` onto the directory of `base_path`, resolving `.`/`..`.
pub fn resolve_relative_path(base_path: &str, relative: &str) -> String {
    let is_absolute = base_path.starts_with('/');
    let mut parts: Vec<&str> = dirname_of(base_path).split('/').filter(|s| !s.is_empty()).collect();
    for seg in relative.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    if is_absolute { format!("/{joined}") } else { joined }
}

static SCHEME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.-]*:").unwrap());

pub fn is_external_link(href: &str) -> bool {
    SCHEME_RE.is_match(href) || href.starts_with("//")
}

/// `decodeURIComponent`; `None` on malformed input (JS throws).
pub fn decode_uri_component(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Resolves a relative Markdown link's href against the file it appears in.
/// `None` for external links, bare anchors or empty hrefs.
pub fn resolve_relative_markdown_link(href: &str, current_path: &str) -> Option<String> {
    let trimmed = href.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || is_external_link(trimmed) {
        return None;
    }
    let path_part = trimmed.split('#').next().unwrap_or("");
    if path_part.is_empty() {
        return None;
    }
    Some(match decode_uri_component(path_part) {
        Some(decoded) => resolve_relative_path(current_path, &decoded),
        None => resolve_relative_path(current_path, path_part),
    })
}

/// Resolves a `[[target]]` wikilink to a vault file by basename (with or
/// without `.md`, case-insensitive; exact-case hit preferred). `#heading`
/// and `|alias` suffixes are stripped first.
pub fn resolve_wikilink_target<S: AsRef<str>>(target: &str, vault_files: &[S]) -> Option<String> {
    let cleaned = target.split('|').next().unwrap_or("").split('#').next().unwrap_or("").trim();
    if cleaned.is_empty() {
        return None;
    }
    let n = cleaned.len();
    let want = if n >= 3 && cleaned.is_char_boundary(n - 3) && cleaned[n - 3..].eq_ignore_ascii_case(".md") {
        &cleaned[..n - 3]
    } else {
        cleaned
    };
    let want_lower = want.to_lowercase();
    let mut ci: Option<&str> = None;
    for path in vault_files {
        let path = path.as_ref();
        let base = basename_no_ext(path);
        if base == want {
            return Some(path.to_string());
        }
        if ci.is_none() && base.to_lowercase() == want_lower {
            ci = Some(path);
        }
    }
    ci.map(String::from)
}

static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\[\]\n]+)\]\]").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub struct WikilinkSpan {
    /// Byte range of the whole `[[...]]` in the line.
    pub start: usize,
    pub end: usize,
    pub target: String,
    pub alias: Option<String>,
    /// Display text: trimmed alias or target (falls back to the target).
    pub label: String,
}

/// Finds `[[target]]` / `[[target|alias]]` in one line (the editor then skips
/// the cursor line and code spans itself).
pub fn find_wikilinks(line: &str) -> Vec<WikilinkSpan> {
    WIKILINK_RE
        .captures_iter(line)
        .map(|c| {
            let whole = c.get(0).unwrap();
            let inner = &c[1];
            let (target, alias) = match inner.find('|') {
                Some(i) => (&inner[..i], Some(&inner[i + 1..])),
                None => (inner, None),
            };
            let label = {
                let l = alias.unwrap_or(target).trim();
                if l.is_empty() { target.trim() } else { l }
            };
            WikilinkSpan {
                start: whole.start(),
                end: whole.end(),
                target: target.to_string(),
                alias: alias.map(String::from),
                label: label.to_string(),
            }
        })
        .collect()
}

static WIKILINK_TRIGGER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\[\]]*)$").unwrap());
pub const MAX_COMPLETIONS: usize = 50;

/// True if the text before the cursor ends in an open `[[...` (completion
/// trigger predicate).
pub fn wikilink_trigger(before: &str) -> bool {
    WIKILINK_TRIGGER_RE.is_match(before)
}

#[derive(Debug, Clone, PartialEq)]
pub struct WikilinkCompletion {
    pub label: String,
    pub path: String,
    pub score: f64,
}

/// The query typed after `[[` in `before` (text before the cursor), if any.
pub fn wikilink_query(before: &str) -> Option<String> {
    WIKILINK_TRIGGER_RE.captures(before).map(|c| c[1].to_string())
}

/// Completion candidates for `query`, best score first, capped at
/// [`MAX_COMPLETIONS`].
pub fn wikilink_completions<S: AsRef<str>>(query: &str, vault_files: &[S]) -> Vec<WikilinkCompletion> {
    let mut scored: Vec<WikilinkCompletion> = vault_files
        .iter()
        .filter_map(|p| {
            let path = p.as_ref();
            let label = basename_no_ext(path);
            let m = fuzzy_match(query, label);
            m.matched.then(|| WikilinkCompletion { label: label.to_string(), path: path.to_string(), score: m.score })
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(MAX_COMPLETIONS);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: [&str; 3] = ["/v/a.md", "/v/sub/Note B.md", "/v/c.md"];

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(resolve_relative_path("/v/sub/a.md", "../b.md"), "/v/b.md");
        assert_eq!(resolve_relative_path("/v/a.md", "./x/./y.md"), "/v/x/y.md");
    }

    #[test]
    fn relative_markdown_links() {
        assert_eq!(resolve_relative_markdown_link("./sub/b.md", "/v/c.md").as_deref(), Some("/v/sub/b.md"));
        assert_eq!(resolve_relative_markdown_link("a%20b.md#h", "/v/c.md").as_deref(), Some("/v/a b.md"));
        assert_eq!(resolve_relative_markdown_link("https://x.y", "/v/c.md"), None);
        assert_eq!(resolve_relative_markdown_link("#top", "/v/c.md"), None);
        assert_eq!(resolve_relative_markdown_link("  ", "/v/c.md"), None);
        assert!(is_external_link("//cdn.x/y") && is_external_link("mailto:a@b"));
    }

    #[test]
    fn wikilink_resolution_prefers_exact_case() {
        let files = ["/v/note.md", "/v/Note.md"];
        assert_eq!(resolve_wikilink_target("Note", &files).as_deref(), Some("/v/Note.md"));
        assert_eq!(resolve_wikilink_target("NOTE.md|alias", &files).as_deref(), Some("/v/note.md"));
        assert_eq!(resolve_wikilink_target("note b#h", &FILES).as_deref(), Some("/v/sub/Note B.md"));
        assert_eq!(resolve_wikilink_target("  ", &FILES), None);
        assert_eq!(resolve_wikilink_target("missing", &FILES), None);
    }

    #[test]
    fn finds_wikilinks_with_alias() {
        let spans = find_wikilinks("x [[a|Alias]] y [[b]]");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].target, "a");
        assert_eq!(spans[0].label, "Alias");
        assert_eq!(&"x [[a|Alias]] y [[b]]"[spans[0].start..spans[0].end], "[[a|Alias]]");
        assert_eq!(spans[1].label, "b");
    }

    #[test]
    fn completion_trigger_and_ranking() {
        assert!(wikilink_trigger("see [[no"));
        assert!(!wikilink_trigger("see [[done]] x"));
        assert_eq!(wikilink_query("see [[no").as_deref(), Some("no"));
        let got = wikilink_completions("nb", &FILES);
        assert_eq!(got[0].label, "Note B");
        assert_eq!(wikilink_completions("", &FILES).len(), 3);
    }
}
