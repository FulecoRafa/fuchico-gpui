//! Zed icon themes (`icon_themes/*.json`) and the file-icon element shared
//! by the explorer, editor tabs and the quick switcher.
//!
//! Icon paths in the JSON are relative to the *extension root* (the folder
//! holding `icon_themes/`), as in Zed. Lookup order, also as in Zed:
//! files → full name in `file_stems`, then each suffix from the longest
//! (`test.ts`) to the shortest (`ts`) in `file_suffixes`, then the
//! `default` icon; folders → `named_directory_icons[name]`, else
//! `directory_icons` (collapsed / expanded).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, StyledImage};
use serde::Deserialize;

use crate::theme::Appearance;
use crate::ui::u;

/// One icon theme with its paths resolved against the extension root.
#[derive(Debug, Default, PartialEq)]
pub struct IconTheme {
    pub name: String,
    pub appearance: Option<Appearance>,
    file_icons: HashMap<String, PathBuf>,
    file_stems: HashMap<String, String>,
    file_suffixes: HashMap<String, String>,
    dir: Option<(PathBuf, PathBuf)>,
    named_dirs: HashMap<String, (PathBuf, PathBuf)>,
}

#[derive(Deserialize)]
struct FamilyJson {
    themes: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ThemeJson {
    name: String,
    #[serde(default)]
    appearance: Option<String>,
    #[serde(default)]
    file_icons: HashMap<String, IconPath>,
    #[serde(default)]
    file_stems: HashMap<String, String>,
    #[serde(default)]
    file_suffixes: HashMap<String, String>,
    #[serde(default)]
    directory_icons: Option<DirIcons>,
    #[serde(default)]
    named_directory_icons: HashMap<String, DirIcons>,
}

#[derive(Deserialize)]
struct IconPath {
    path: String,
}

#[derive(Deserialize)]
struct DirIcons {
    collapsed: Option<String>,
    expanded: Option<String>,
}

/// Whether a parsed JSON value looks like an icon theme family.
pub fn is_icon_family(v: &serde_json::Value) -> bool {
    v.get("themes")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|t| {
            t.iter().any(|t| {
                t.get("file_icons").is_some()
                    || t.get("directory_icons").is_some()
                    || t.get("file_suffixes").is_some()
            })
        })
}

/// A relative icon path from the JSON (`./icons/x.svg`) as a safe relative
/// path: `None` for absolute paths or ones escaping the root.
pub fn rel_icon_path(s: &str) -> Option<PathBuf> {
    let p = Path::new(s.trim_start_matches("./"));
    let ok = p
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (ok && !s.is_empty()).then(|| p.to_path_buf())
}

/// Every relative icon path a family references (for copying on import).
pub fn referenced_paths(v: &serde_json::Value) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(themes) = v.get("themes").and_then(serde_json::Value::as_array) else {
        return out;
    };
    let mut push = |s: Option<&str>| {
        if let Some(p) = s.and_then(rel_icon_path) {
            out.push(p);
        }
    };
    for t in themes {
        if let Some(m) = t.get("file_icons").and_then(serde_json::Value::as_object) {
            for v in m.values() {
                push(v.get("path").and_then(serde_json::Value::as_str));
            }
        }
        let dirs = t.get("directory_icons").into_iter().chain(
            t.get("named_directory_icons")
                .and_then(serde_json::Value::as_object)
                .into_iter()
                .flat_map(|m| m.values()),
        );
        for d in dirs {
            push(d.get("collapsed").and_then(serde_json::Value::as_str));
            push(d.get("expanded").and_then(serde_json::Value::as_str));
        }
    }
    out.sort();
    out.dedup();
    out
}

impl IconTheme {
    /// Every theme of an icon theme family file; icon paths resolve against
    /// `root`. Malformed entries are skipped (logged).
    pub fn parse_family(json: &str, root: &Path) -> Result<Vec<IconTheme>, String> {
        let v = super::color::parse_lenient(json)?;
        let family: FamilyJson = serde_json::from_value(v).map_err(|e| e.to_string())?;
        let themes: Vec<IconTheme> = family
            .themes
            .into_iter()
            .enumerate()
            .filter_map(|(i, t)| match serde_json::from_value::<ThemeJson>(t) {
                Ok(t) => Some(Self::from_json(t, root)),
                Err(e) => {
                    log::warn!("icon theme #{i}: {e}, skipped");
                    None
                }
            })
            .collect();
        if themes.is_empty() {
            return Err("no usable icon themes".into());
        }
        Ok(themes)
    }

    fn from_json(t: ThemeJson, root: &Path) -> IconTheme {
        let p = |s: &str| rel_icon_path(s).map(|r| root.join(r));
        let pair = |d: &DirIcons| -> Option<(PathBuf, PathBuf)> {
            let c = d.collapsed.as_deref().and_then(p);
            let e = d.expanded.as_deref().and_then(p);
            match (c, e) {
                (Some(c), Some(e)) => Some((c, e)),
                (Some(x), None) | (None, Some(x)) => Some((x.clone(), x)),
                _ => None,
            }
        };
        IconTheme {
            name: t.name,
            appearance: match t.appearance.as_deref() {
                Some("light") => Some(Appearance::Light),
                Some("dark") => Some(Appearance::Dark),
                _ => None,
            },
            file_icons: t
                .file_icons
                .into_iter()
                .filter_map(|(k, v)| p(&v.path).map(|p| (k, p)))
                .collect(),
            file_stems: t.file_stems,
            file_suffixes: t.file_suffixes,
            dir: t.directory_icons.as_ref().and_then(pair),
            named_dirs: t
                .named_directory_icons
                .iter()
                .filter_map(|(k, d)| pair(d).map(|p| (k.clone(), p)))
                .collect(),
        }
    }

    /// Zed's file lookup (see the module docs).
    pub fn file_icon(&self, name: &str) -> Option<&Path> {
        let key = self.file_stems.get(name).or_else(|| {
            name.match_indices('.')
                .filter(|(i, _)| *i > 0)
                .find_map(|(i, _)| self.file_suffixes.get(&name[i + 1..]))
        });
        key.and_then(|k| self.file_icons.get(k))
            .or_else(|| self.file_icons.get("default"))
            .map(PathBuf::as_path)
    }

    pub fn dir_icon(&self, name: &str, expanded: bool) -> Option<&Path> {
        self.named_dirs
            .get(name)
            .or(self.dir.as_ref())
            .map(|(c, e)| if expanded { e.as_path() } else { c.as_path() })
    }
}

/// What a file icon is for.
#[derive(Clone, Copy, Debug)]
pub enum IconFor<'a> {
    File(&'a str),
    Dir { name: &'a str, expanded: bool },
}

/// The active icon theme (settings `iconTheme`), if any.
pub fn active(cx: &App) -> Option<Arc<IconTheme>> {
    let name = crate::settings::SettingsStore::try_get(cx)?
        .icon_theme
        .clone()?;
    super::ZedThemes::get(cx)
        .icon(&name)
        .map(|e| e.theme.clone())
}

/// The themed icon path for `what`, when an icon theme is active.
pub fn themed_path(what: IconFor, cx: &App) -> Option<PathBuf> {
    let t = active(cx)?;
    match what {
        IconFor::File(name) => t.file_icon(name),
        IconFor::Dir { name, expanded } => t.dir_icon(name, expanded),
    }
    .map(Path::to_path_buf)
}

/// The icon element: the themed SVG (falling back to `fallback` when it
/// fails to load or render), or `fallback` when no theme is active.
/// `id` is a debug selector prefix: `<id>:<icon file stem>` for a themed
/// icon, `<id>:lucide` otherwise (headless tests assert on it).
pub fn file_icon_el(
    what: IconFor,
    size: f32,
    id: String,
    fallback: impl Fn() -> AnyElement + 'static,
    cx: &App,
) -> AnyElement {
    match themed_path(what, cx) {
        Some(path) => {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            gpui_kit::div()
                .flex_none()
                .size(u(size))
                .debug_selector(move || format!("{id}:{stem}"))
                .child(gpui_kit::img(path).size(u(size)).with_fallback(fallback))
                .into_any_element()
        }
        None => gpui_kit::div()
            .flex_none()
            .flex()
            .items_center()
            .debug_selector(move || format!("{id}:lucide"))
            .child(fallback())
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"name":"Fam","themes":[{
        "name": "Test Icons", "appearance": "dark",
        "file_icons": {
            "default": {"path": "./icons/file.svg"},
            "markdown": {"path": "./icons/markdown.svg"},
            "ts": {"path": "./icons/ts.svg"},
            "tstest": {"path": "./icons/test-ts.svg"},
            "readme": {"path": "./icons/readme.svg"},
            "evil": {"path": "../../etc/passwd"}
        },
        "file_stems": {"README.md": "readme"},
        "file_suffixes": {"md": "markdown", "ts": "ts", "test.ts": "tstest", "x": "evil"},
        "directory_icons": {"collapsed": "./icons/folder.svg", "expanded": "./icons/folder-open.svg"},
        "named_directory_icons": {"templates": {"collapsed": "./icons/t.svg", "expanded": "./icons/t-open.svg"}}
    }, {"name": "Light Icons", "appearance": "light", "file_icons": {}},
       {"appearance": "light"}]}"#;

    #[test]
    fn resolves_like_zed() {
        let root = Path::new("/x");
        let themes = IconTheme::parse_family(JSON, root).unwrap();
        assert_eq!(themes.len(), 2, "the nameless entry is skipped");
        let t = &themes[0];
        assert_eq!(t.name, "Test Icons");
        assert_eq!(t.appearance, Some(Appearance::Dark));
        assert_eq!(themes[1].appearance, Some(Appearance::Light));
        let f = |n| t.file_icon(n).unwrap().to_path_buf();
        assert_eq!(f("note.md"), root.join("icons/markdown.svg"));
        assert_eq!(f("README.md"), root.join("icons/readme.svg"));
        assert_eq!(f("a.test.ts"), root.join("icons/test-ts.svg"));
        assert_eq!(f("a.ts"), root.join("icons/ts.svg"));
        assert_eq!(f("drawing.excalidraw"), root.join("icons/file.svg"));
        assert_eq!(f(".hidden"), root.join("icons/file.svg"));
        // Paths escaping the root are dropped → default icon.
        assert_eq!(f("a.x"), root.join("icons/file.svg"));
        let d = |n, e| t.dir_icon(n, e).unwrap().to_path_buf();
        assert_eq!(d("templates", false), root.join("icons/t.svg"));
        assert_eq!(d("templates", true), root.join("icons/t-open.svg"));
        assert_eq!(d("other", true), root.join("icons/folder-open.svg"));
        assert_eq!(d("other", false), root.join("icons/folder.svg"));
        // No folder icons, no default → nothing (callers use Lucide).
        assert!(themes[1].file_icon("a.md").is_none());
        assert!(themes[1].dir_icon("a", false).is_none());
    }

    #[test]
    fn referenced_paths_are_safe_and_unique() {
        let v: serde_json::Value = serde_json::from_str(JSON).unwrap();
        let refs = referenced_paths(&v);
        assert!(refs.contains(&PathBuf::from("icons/markdown.svg")));
        assert!(refs.contains(&PathBuf::from("icons/t-open.svg")));
        assert!(!refs.iter().any(|p| p.to_string_lossy().contains("..")));
        assert_eq!(rel_icon_path("/abs.svg"), None);
        assert!(is_icon_family(&v));
        assert!(!super::super::color::is_color_family(&v));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(IconTheme::parse_family("nope", Path::new("/")).is_err());
        assert!(IconTheme::parse_family(r#"{"themes":[]}"#, Path::new("/")).is_err());
    }
}
