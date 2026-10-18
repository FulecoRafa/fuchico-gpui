//! Inline image previews (`EditorOptions::inline_images`, off by default).
//!
//! The original CodeMirror editor has no inline image widget: `![](x)` and
//! `![[x.png]]` stay as (styled) text. This is an opt-in addition: the
//! first image embed of a line is drawn below that line, scaled to fit.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

static MD_IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"!\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)"#).unwrap());
static EMBED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[\[([^\]|#]+)(?:[#|][^\]]*)?\]\]").unwrap());

/// Inline code spans (`` `x` ``, ``` ``x`` ```): embeds inside are literal.
static CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(`+)[^`]+?`+").unwrap());

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

/// Vertical padding around a preview.
pub(crate) const IMAGE_PAD: f32 = 6.;
/// Maximum preview height.
pub(crate) const IMAGE_MAX_H: f32 = 360.;

fn is_image(p: &str) -> bool {
    Path::new(p)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Where image paths resolve from.
pub(crate) struct ImageContext<'a> {
    pub vault_root: Option<&'a Path>,
    /// Vault-relative (or absolute) path of the note.
    pub note_path: Option<&'a str>,
    pub vault_files: &'a [String],
}

impl ImageContext<'_> {
    fn note_dir(&self) -> Option<PathBuf> {
        let note = Path::new(self.note_path?);
        let full = match self.vault_root {
            Some(root) if note.is_relative() => root.join(note),
            _ => note.to_path_buf(),
        };
        full.parent().map(Path::to_path_buf)
    }

    fn rooted(&self, rel: &str) -> PathBuf {
        match self.vault_root {
            Some(root) => root.join(rel),
            None => PathBuf::from(rel),
        }
    }

    /// `![](url)`: absolute, else relative to the note, else to the vault.
    fn resolve_markdown(&self, url: &str) -> Option<PathBuf> {
        if url.contains("://") && !url.starts_with("file://") {
            return None; // no network loads
        }
        let url = url.trim_start_matches("file://").replace("%20", " ");
        let p = Path::new(&url);
        if p.is_absolute() {
            return Some(p.to_path_buf());
        }
        if let Some(dir) = self.note_dir() {
            let c = dir.join(p);
            if c.exists() {
                return Some(c);
            }
        }
        Some(self.rooted(&url))
    }

    /// `![[name]]`: an exact vault path, else the vault file whose name
    /// matches, else next to the note.
    fn resolve_embed(&self, target: &str) -> Option<PathBuf> {
        let target = target.trim();
        if self.vault_files.iter().any(|f| f == target) {
            return Some(self.rooted(target));
        }
        let suffix = format!("/{target}");
        if let Some(f) = self.vault_files.iter().find(|f| f.ends_with(&suffix)) {
            return Some(self.rooted(f));
        }
        if let Some(dir) = self.note_dir() {
            let c = dir.join(target);
            if c.exists() {
                return Some(c);
            }
        }
        Some(self.rooted(target))
    }

    /// The first image embedded in `line` (outside code).
    pub fn first_image(&self, line: &str) -> Option<PathBuf> {
        let code: Vec<_> = CODE_SPAN.find_iter(line).map(|m| m.range()).collect();
        let outside = |start: usize| !code.iter().any(|r| r.contains(&start));
        let md = MD_IMAGE
            .captures_iter(line)
            .map(|c| (c.get(0).unwrap().start(), c[1].to_string(), false));
        let em = EMBED
            .captures_iter(line)
            .map(|c| (c.get(0).unwrap().start(), c[1].to_string(), true));
        let mut found: Vec<_> = md
            .chain(em)
            .filter(|(at, u, _)| outside(*at) && is_image(u))
            .collect();
        found.sort_by_key(|f| f.0);
        let (_, url, embed) = found.into_iter().next()?;
        if embed {
            self.resolve_embed(&url)
        } else {
            self.resolve_markdown(&url)
        }
    }
}

/// Display size of a `natural` image in a column `max_w` wide.
pub(crate) fn fit(natural: (f32, f32), max_w: f32) -> (f32, f32) {
    let (w, h) = natural;
    if w <= 0. || h <= 0. {
        return (0., 0.);
    }
    let s = (max_w.max(16.) / w).min(IMAGE_MAX_H / h).min(1.);
    ((w * s).round(), (h * s).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_markdown_and_embed_images() {
        let files = vec!["att/pic.png".to_string()];
        let cx = ImageContext {
            vault_root: Some(Path::new("/v")),
            note_path: Some("notes/n.md"),
            vault_files: &files,
        };
        assert_eq!(
            cx.first_image("see ![[pic.png]] here"),
            Some(PathBuf::from("/v/att/pic.png"))
        );
        assert_eq!(
            cx.first_image("![a](/abs/x.jpg)"),
            Some(PathBuf::from("/abs/x.jpg"))
        );
        assert_eq!(
            cx.first_image("![a](img/x%20y.png)"),
            Some(PathBuf::from("/v/img/x y.png"))
        );
        assert_eq!(cx.first_image("![a](https://x.y/z.png)"), None);
        assert_eq!(cx.first_image("![[Note]] and [link](a.png)"), None);
        assert_eq!(cx.first_image("`![[pic.png]]` code"), None);
        assert_eq!(
            cx.first_image("`![[x.png]]` then ![[pic.png]]"),
            Some(PathBuf::from("/v/att/pic.png"))
        );
    }

    #[test]
    fn fit_scales_down_only() {
        assert_eq!(fit((100., 50.), 800.), (100., 50.));
        assert_eq!(fit((1600., 400.), 800.), (800., 200.));
        assert_eq!(fit((100., 720.), 800.), (50., 360.));
    }
}
