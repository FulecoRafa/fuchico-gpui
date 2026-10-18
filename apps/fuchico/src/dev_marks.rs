//! Opt-in visual markers for telling the GPUI build apart from the Tauri one
//! (not part of the original app; both are off unless their env var is set):
//!
//! - `FUCHICO_ICON_THEME=<dir>`: a Zed icon theme extension directory (e.g.
//!   `~/Library/Application Support/Zed/extensions/installed/material-icon-theme`);
//!   explorer rows use its coloured file and folder icons instead of Lucide.
//! - `FUCHICO_BADGE=1`: a small "GPUI" chip at the bottom of the activity bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::{App, Global};
use serde::Deserialize;

#[derive(Default)]
pub struct DevMarks {
    pub icon_theme: Option<IconTheme>,
    pub badge: bool,
}

impl Global for DevMarks {}

pub fn init(cx: &mut App) {
    let icon_theme = std::env::var_os("FUCHICO_ICON_THEME").and_then(|dir| {
        let dir = PathBuf::from(dir);
        match IconTheme::load(&dir) {
            Ok(t) => Some(t),
            Err(e) => {
                log::warn!("icon theme {}: {e}", dir.display());
                None
            }
        }
    });
    let badge = std::env::var_os("FUCHICO_BADGE").is_some_and(|v| v != "0");
    cx.set_global(DevMarks { icon_theme, badge });
}

pub fn icon_theme(cx: &App) -> Option<&IconTheme> {
    cx.try_global::<DevMarks>()
        .and_then(|m| m.icon_theme.as_ref())
}

pub fn badge(cx: &App) -> bool {
    cx.try_global::<DevMarks>().is_some_and(|m| m.badge)
}

/// The parts of a Zed icon theme (`icon_themes/*.json`) explorer rows need.
#[derive(Debug, Default)]
pub struct IconTheme {
    file_icons: HashMap<String, PathBuf>,
    file_stems: HashMap<String, String>,
    file_suffixes: HashMap<String, String>,
    dir: Option<(PathBuf, PathBuf)>,
    named_dirs: HashMap<String, (PathBuf, PathBuf)>,
}

#[derive(Deserialize)]
struct ThemeFamily {
    themes: Vec<ThemeJson>,
}

#[derive(Deserialize)]
struct ThemeJson {
    #[serde(default)]
    file_icons: HashMap<String, IconPath>,
    #[serde(default)]
    file_stems: HashMap<String, String>,
    #[serde(default)]
    file_suffixes: HashMap<String, String>,
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
    collapsed: String,
    expanded: String,
}

impl IconTheme {
    /// Loads the first theme of the first `icon_themes/*.json` in `root`;
    /// icon paths resolve against `root`, as Zed does.
    pub fn load(root: &Path) -> Result<Self, String> {
        let dir = root.join("icon_themes");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        let file = files.first().ok_or("no icon_themes/*.json")?;
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        Self::parse(&text, root)
    }

    pub fn parse(json: &str, root: &Path) -> Result<Self, String> {
        let family: ThemeFamily = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let t = family.themes.into_iter().next().ok_or("no themes")?;
        let p = |s: &str| root.join(s.trim_start_matches("./"));
        Ok(Self {
            file_icons: t
                .file_icons
                .into_iter()
                .map(|(k, v)| (k, p(&v.path)))
                .collect(),
            file_stems: t.file_stems,
            file_suffixes: t.file_suffixes,
            dir: t.directory_icons.map(|d| (p(&d.collapsed), p(&d.expanded))),
            named_dirs: t
                .named_directory_icons
                .into_iter()
                .map(|(k, d)| (k, (p(&d.collapsed), p(&d.expanded))))
                .collect(),
        })
    }

    /// Zed's lookup: full file name in `file_stems`, then each suffix from
    /// the longest (`test.ts`) to the shortest (`ts`), then `default`.
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

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"themes":[{
        "file_icons": {
            "default": {"path": "./icons/file.svg"},
            "markdown": {"path": "./icons/markdown.svg"},
            "ts": {"path": "./icons/ts.svg"},
            "tstest": {"path": "./icons/test-ts.svg"},
            "readme": {"path": "./icons/readme.svg"}
        },
        "file_stems": {"README.md": "readme"},
        "file_suffixes": {"md": "markdown", "ts": "ts", "test.ts": "tstest"},
        "directory_icons": {"collapsed": "./icons/folder.svg", "expanded": "./icons/folder-open.svg"},
        "named_directory_icons": {"templates": {"collapsed": "./icons/t.svg", "expanded": "./icons/t-open.svg"}}
    }]}"#;

    #[test]
    fn resolves_like_zed() {
        let root = Path::new("/x");
        let t = IconTheme::parse(JSON, root).unwrap();
        let f = |n| t.file_icon(n).unwrap().to_path_buf();
        assert_eq!(f("note.md"), root.join("icons/markdown.svg"));
        assert_eq!(f("README.md"), root.join("icons/readme.svg"));
        assert_eq!(f("a.test.ts"), root.join("icons/test-ts.svg"));
        assert_eq!(f("a.ts"), root.join("icons/ts.svg"));
        assert_eq!(f("drawing.excalidraw"), root.join("icons/file.svg"));
        assert_eq!(f(".hidden"), root.join("icons/file.svg"));
        let d = |n, e| t.dir_icon(n, e).unwrap().to_path_buf();
        assert_eq!(d("templates", false), root.join("icons/t.svg"));
        assert_eq!(d("other", true), root.join("icons/folder-open.svg"));
    }
}
