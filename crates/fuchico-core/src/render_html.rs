//! Port of `src/modules/export/lib/renderHtml.ts`: Markdown -> standalone
//! HTML document (issue #28) using `pulldown-cmark` in place of `marked`.
//!
//! Differences from `marked` that cannot be matched byte-for-byte: bare-URL
//! autolinks (GFM extended autolinks) are not produced, and whitespace /
//! attribute details of stock elements follow pulldown-cmark's HTML writer.
//! The three custom renderers (image, link, task list item) match the TS.

use std::sync::LazyLock;

use pulldown_cmark::{html, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use regex::{Captures, Regex};

use crate::frontmatter::parse_frontmatter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HrefKind {
    Image,
    Link,
}

pub type HrefResolver<'a> = &'a dyn Fn(&str, HrefKind) -> String;

pub struct RenderOptions<'a> {
    pub title: &'a str,
    /// Rewrites relative image/link hrefs. `None` leaves them alone.
    pub resolve_href: Option<HrefResolver<'a>>,
    /// Extra markup appended to `<body>`.
    pub body_extra: &'a str,
    /// Theme variable values to inline (`--background` -> `#fff`).
    pub theme_vars: &'a [(String, String)],
    pub mode: ThemeMode,
}

static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\[\]|#]+)(#[^\[\]|]*)?(?:\|([^\[\]]+))?\]\]").unwrap());
static EMBED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[\[([^\[\]|]+)(?:\|([^\[\]]+))?\]\]").unwrap());
static NON_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_]+").unwrap());

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// JS `encodeURI`.
pub fn encode_uri(s: &str) -> String {
    const KEEP: &[u8] = b";,/?:@&=+$-_.!~*'()#";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || KEEP.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `[[Note|alias]]` -> `[alias](Note.html)`, `![[img.png]]` -> image.
pub fn rewrite_wikilinks(markdown: &str) -> String {
    let s = EMBED_RE.replace_all(markdown, |c: &Captures| {
        let t = c[1].trim();
        let alt = c.get(2).map(|m| m.as_str().trim()).unwrap_or(t);
        format!("![{alt}]({})", encode_uri(t))
    });
    WIKILINK_RE
        .replace_all(&s, |c: &Captures| {
            let t = c[1].trim();
            let heading = c.get(2).map(|m| m.as_str());
            let label = match c.get(3) {
                Some(a) => a.as_str().trim().to_string(),
                None => match heading {
                    Some(h) => format!("{t}{h}"),
                    None => t.to_string(),
                },
            };
            let anchor = match heading {
                Some(h) => format!("#{}", NON_WORD_RE.replace_all(&h[1..].trim().to_lowercase(), "-")),
                None => String::new(),
            };
            format!("[{label}]({}.html{anchor})", encode_uri(t))
        })
        .into_owned()
}

const READING_CSS: &str = r#"
:root { color-scheme: light dark; }
html { background: var(--background); color: var(--foreground); }
body {
  margin: 0; padding: 48px 24px 96px;
  font-family: var(--font-sans, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif);
  font-size: 16px; line-height: 1.65;
}
main { max-width: 760px; margin: 0 auto; }
h1, h2, h3, h4, h5, h6 { line-height: 1.25; margin: 1.6em 0 0.6em; color: var(--syntax-keyword); }
h1 { font-size: 2em; margin-top: 0; } h2 { font-size: 1.5em; } h3 { font-size: 1.25em; }
a { color: var(--syntax-function); text-decoration: none; }
a:hover { text-decoration: underline; }
code, pre, kbd { font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace); font-size: 0.9em; }
code { background: var(--muted); padding: 0.1em 0.35em; border-radius: 4px; }
pre { background: var(--muted); padding: 14px 16px; border-radius: 8px; overflow-x: auto; }
pre code { background: none; padding: 0; }
blockquote { margin: 1em 0; padding: 0.2em 1em; border-left: 3px solid var(--border); color: var(--muted-foreground); }
table { border-collapse: collapse; width: 100%; margin: 1em 0; }
th, td { border: 1px solid var(--border); padding: 6px 10px; text-align: left; }
th { background: var(--muted); }
img { max-width: 100%; height: auto; }
hr { border: none; border-top: 1px solid var(--border); margin: 2em 0; }
input[type="checkbox"] { margin-right: 0.5em; }
li.task { list-style: none; margin-left: -1.4em; }
.tag { color: var(--syntax-type); }
@media print {
  body { padding: 0; font-size: 12pt; }
  pre { white-space: pre-wrap; }
  a { color: inherit; }
  .no-print { display: none !important; }
}
"#;

/// Just the Markdown -> HTML body fragment (no document shell), with the
/// image / link / task-item customisations applied.
pub fn render_body(markdown_body: &str, resolve: Option<HrefResolver>) -> String {
    let resolve_href = |h: &str, k: HrefKind| match resolve {
        Some(f) => f(h, k),
        None => h.to_string(),
    };
    let src = rewrite_wikilinks(markdown_body);
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<Event> = Parser::new_ext(&src, opts).collect();
    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    let title_attr = |t: &CowStr| if t.is_empty() { String::new() } else { format!(" title=\"{}\"", escape_html(t)) };

    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::Image { dest_url, title, .. }) => {
                let mut alt = String::new();
                let mut depth = 1;
                let mut j = i + 1;
                while j < events.len() {
                    match &events[j] {
                        Event::Start(Tag::Image { .. }) => depth += 1,
                        Event::End(TagEnd::Image) => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Event::Text(t) | Event::Code(t) => alt.push_str(t),
                        Event::SoftBreak | Event::HardBreak => alt.push(' '),
                        _ => {}
                    }
                    j += 1;
                }
                out.push(Event::Html(
                    format!(
                        "<img src=\"{}\" alt=\"{}\"{}>",
                        escape_html(&resolve_href(dest_url, HrefKind::Image)),
                        escape_html(&alt),
                        title_attr(title)
                    )
                    .into(),
                ));
                i = j + 1;
                continue;
            }
            Event::Start(Tag::Link { link_type, dest_url, title, .. }) => {
                let href = if *link_type == LinkType::Email { format!("mailto:{dest_url}") } else { dest_url.to_string() };
                out.push(Event::Html(
                    format!(
                        "<a href=\"{}\"{}>",
                        escape_html(&resolve_href(&href, HrefKind::Link)),
                        title_attr(title)
                    )
                    .into(),
                ));
            }
            Event::End(TagEnd::Link) => out.push(Event::Html("</a>".into())),
            Event::Start(Tag::Item) if matches!(events.get(i + 1), Some(Event::TaskListMarker(_))) => {
                let Some(Event::TaskListMarker(checked)) = events.get(i + 1) else { unreachable!() };
                out.push(Event::Html(
                    format!(
                        "<li class=\"task\"><input type=\"checkbox\" disabled{}> ",
                        if *checked { " checked" } else { "" }
                    )
                    .into(),
                ));
                i += 2;
                continue;
            }
            e => out.push(e.clone()),
        }
        i += 1;
    }
    let mut html_out = String::new();
    html::push_html(&mut html_out, out.into_iter());
    html_out
}

/// Full standalone HTML document for `markdown` (frontmatter stripped).
pub fn render_html(markdown: &str, opts: &RenderOptions) -> String {
    let body = match parse_frontmatter(markdown) {
        Some(fm) => &markdown[fm.body_start..],
        None => markdown,
    };
    let html_body = render_body(body, opts.resolve_href);
    let var_css = opts.theme_vars.iter().map(|(k, v)| format!("  {k}: {v};")).collect::<Vec<_>>().join("\n");
    let mode = match opts.mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    };
    format!(
        "<!doctype html>\n<html lang=\"en\" data-mode=\"{mode}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{}</title>\n<style>\n:root {{\n{var_css}\n}}\n{READING_CSS}\n</style>\n</head>\n<body>\n<main>\n{html_body}\n</main>\n{}\n</body>\n</html>\n",
        escape_html(opts.title),
        opts.body_extra
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_wikilinks_turns_wikilinks_into_relative_html_links() {
        assert_eq!(
            rewrite_wikilinks("see [[Note B|alias]] and [[C#Head ing]]"),
            "see [alias](Note%20B.html) and [C#Head ing](C.html#head-ing)"
        );
        assert_eq!(rewrite_wikilinks("![[pic.png]]"), "![pic.png](pic.png)");
    }

    #[test]
    fn renders_a_standalone_document_with_theme_vars_and_no_frontmatter() {
        let resolve = |h: &str, _k: HrefKind| format!("X:{h}");
        let vars = vec![("--background".to_string(), "white".to_string())];
        let html = render_html(
            "---\ntitle: x\n---\n# Hi\n\n- [ ] todo\n",
            &RenderOptions {
                title: "Hi",
                resolve_href: Some(&resolve),
                body_extra: "",
                theme_vars: &vars,
                mode: ThemeMode::Light,
            },
        );
        assert!(html.contains("<title>Hi</title>"));
        assert!(html.contains("--background: white;"));
        assert!(html.contains("<h1>Hi</h1>"));
        assert!(html.contains("<li class=\"task\"><input type=\"checkbox\" disabled>"));
        assert!(!html.contains("title: x"));
    }

    #[test]
    fn custom_image_and_link_renderers_resolve_and_escape() {
        let resolve = |h: &str, k: HrefKind| match k {
            HrefKind::Image => format!("img:{h}"),
            HrefKind::Link => format!("lnk:{h}"),
        };
        let out = render_body("![a \"b\"](p.png \"T\") and [x](y.md)\n", Some(&resolve));
        assert!(out.contains("<img src=\"img:p.png\" alt=\"a &quot;b&quot;\" title=\"T\">"), "{out}");
        assert!(out.contains("<a href=\"lnk:y.md\">x</a>"), "{out}");
    }

    #[test]
    fn checked_task_items_and_gfm_tables() {
        let out = render_body("- [x] done\n\n| a | b |\n|---|---|\n| 1 | 2 |\n", None);
        assert!(out.contains("<li class=\"task\"><input type=\"checkbox\" disabled checked> done"), "{out}");
        assert!(out.contains("<table>"), "{out}");
    }

    #[test]
    fn encode_uri_matches_js() {
        assert_eq!(encode_uri("a b/ü#x"), "a%20b/%C3%BC#x");
    }
}
