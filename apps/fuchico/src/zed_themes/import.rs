//! Importing a Zed extension folder or a single theme `.json` into
//! Fuchico's config dir, so it survives uninstalling it from Zed.
//!
//! Layout: `<config>/zed-themes/<extension id>/` mirrors an extension:
//!
//! ```text
//! zed-themes/
//!   material-icon-theme/
//!     extension.toml          (copied, or written for a single .json)
//!     icon_themes/material-icon-theme.json
//!     icons/…                 (only the files the icon themes reference)
//!   one/
//!     extension.toml
//!     themes/one.json
//! ```
//!
//! Icon paths stay relative to the extension root, as in Zed. Re-importing
//! the same id replaces the previous copy (written to a hidden scratch dir
//! first, then swapped in).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::color::{ZedThemeFamily, is_color_family, parse_lenient};
use super::discover::read_extension;
use super::icons::{IconTheme, is_icon_family, referenced_paths};

/// What an import brought in.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub id: String,
    pub name: String,
    pub colors: Vec<String>,
    pub icons: Vec<String>,
    pub dest: PathBuf,
}

/// `My Theme!` → `my-theme` (ASCII letters, digits, `-`, `_`, `.`).
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches(|c| c == '-' || c == '.').to_string();
    if out.is_empty() { "theme".into() } else { out }
}

/// One theme file to copy: source, destination (relative to the new
/// extension root) and, for icon themes, the root its icons resolve from.
struct Plan {
    id: String,
    name: String,
    manifest: Option<PathBuf>,
    colors: Vec<(PathBuf, PathBuf)>,
    icons: Vec<(PathBuf, PathBuf, PathBuf)>,
}

fn read_json(p: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    parse_lenient(&text).map_err(|e| format!("{}: {e}", p.display()))
}

fn file_name(p: &Path) -> PathBuf {
    PathBuf::from(p.file_name().unwrap_or_default())
}

fn plan_dir(src: &Path) -> Result<Plan, String> {
    let ext = read_extension(src).ok_or("no themes/ or icon_themes/ JSON in that folder")?;
    Ok(Plan {
        id: slug(&ext.id),
        name: ext.name,
        manifest: Some(src.join("extension.toml")).filter(|p| p.is_file()),
        colors: ext
            .color_files
            .iter()
            .map(|f| (f.clone(), Path::new("themes").join(file_name(f))))
            .collect(),
        icons: ext
            .icon_files
            .iter()
            .map(|f| {
                (
                    f.clone(),
                    Path::new("icon_themes").join(file_name(f)),
                    src.to_path_buf(),
                )
            })
            .collect(),
    })
}

fn plan_file(src: &Path) -> Result<Plan, String> {
    let v = read_json(src)?;
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "theme".into());
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or(stem);
    let mut plan = Plan {
        id: slug(&name),
        name,
        manifest: None,
        colors: Vec::new(),
        icons: Vec::new(),
    };
    if is_color_family(&v) {
        plan.colors
            .push((src.to_path_buf(), Path::new("themes").join(file_name(src))));
    } else if is_icon_family(&v) {
        // Icons resolve from the extension root: the parent of
        // `icon_themes/`, else the JSON's own folder.
        let parent = src.parent().unwrap_or(Path::new("."));
        let root = if parent.file_name().is_some_and(|n| n == "icon_themes") {
            parent.parent().unwrap_or(parent)
        } else {
            parent
        };
        plan.icons.push((
            src.to_path_buf(),
            Path::new("icon_themes").join(file_name(src)),
            root.to_path_buf(),
        ));
    } else {
        return Err("not a Zed colour theme or icon theme file".into());
    }
    Ok(plan)
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", from.display()))
}

/// Imports `src` (an extension folder or a theme `.json`) into
/// `dest_root/<id>/`. Validates every theme file first; nothing is written
/// when none parses.
pub fn import_path(src: &Path, dest_root: &Path) -> Result<Imported, String> {
    let plan = if src.is_dir() {
        plan_dir(src)?
    } else if src.is_file() {
        plan_file(src)?
    } else {
        return Err(format!("{}: not found", src.display()));
    };

    // Validate (and collect names); unparsable files are left out.
    let mut colors = Vec::new();
    let mut color_files = Vec::new();
    for (from, rel) in &plan.colors {
        match read_json(from).and_then(|v| ZedThemeFamily::from_value(&v)) {
            Ok(f) => {
                colors.extend(f.themes.into_iter().map(|t| t.name));
                color_files.push((from, rel));
            }
            Err(e) => log::warn!("import {}: {e}", from.display()),
        }
    }
    let mut icons = Vec::new();
    let mut icon_files = Vec::new();
    for (from, rel, root) in &plan.icons {
        let parsed = std::fs::read_to_string(from)
            .map_err(|e| e.to_string())
            .and_then(|t| IconTheme::parse_family(&t, root).map(|th| (th, t)));
        match parsed {
            Ok((themes, text)) => {
                icons.extend(themes.into_iter().map(|t| t.name));
                let refs = parse_lenient(&text)
                    .map(|v| referenced_paths(&v))
                    .unwrap_or_default();
                icon_files.push((from, rel, root, refs));
            }
            Err(e) => log::warn!("import {}: {e}", from.display()),
        }
    }
    if colors.is_empty() && icons.is_empty() {
        return Err("no valid Zed themes found".into());
    }

    std::fs::create_dir_all(dest_root).map_err(|e| e.to_string())?;
    let dest = dest_root.join(&plan.id);
    let tmp = dest_root.join(format!(".import-{}-{}", plan.id, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let write = || -> Result<(), String> {
        match &plan.manifest {
            Some(m) => copy_file(m, &tmp.join("extension.toml"))?,
            None => {
                std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
                let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
                let manifest = format!(
                    "# Written by Fuchico when importing a single theme file.\nid = \"{}\"\nname = \"{}\"\nversion = \"imported\"\nschema_version = 1\n",
                    q(&plan.id),
                    q(&plan.name)
                );
                std::fs::write(tmp.join("extension.toml"), manifest).map_err(|e| e.to_string())?;
            }
        }
        for (from, rel) in &color_files {
            copy_file(from, &tmp.join(rel))?;
        }
        for (from, rel, root, refs) in &icon_files {
            copy_file(from, &tmp.join(rel))?;
            for r in refs {
                let s = root.join(r);
                if s.is_file() {
                    copy_file(&s, &tmp.join(r))?;
                } else {
                    log::warn!("import: missing icon {}", s.display());
                }
            }
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(Imported {
        id: plan.id,
        name: plan.name,
        colors,
        icons,
        dest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zed_themes::test_support::fixtures;

    #[test]
    fn imports_an_icon_extension_with_its_icons() {
        let dest = crate::test_ui::temp_dir("zed-import-icons");
        let r = import_path(&fixtures().join("tiny-icons"), &dest).unwrap();
        assert_eq!(r.id, "tiny-icons");
        assert_eq!(r.icons, vec!["Tiny Icons".to_string()]);
        let root = dest.join("tiny-icons");
        assert!(root.join("extension.toml").is_file());
        assert!(root.join("icon_themes/tiny-icons.json").is_file());
        assert!(root.join("icons/markdown.svg").is_file());
        assert!(root.join("icons/folder-templates-open.svg").is_file());
        // Referenced but missing in the source: skipped, not fatal.
        assert!(!root.join("icons/missing.svg").exists());
        // No scratch dir left behind; re-import replaces in place.
        import_path(&fixtures().join("tiny-icons"), &dest).unwrap();
        let entries: Vec<_> = std::fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["tiny-icons".to_string()]);
    }

    #[test]
    fn imports_single_json_files() {
        let dest = crate::test_ui::temp_dir("zed-import-json");
        let r = import_path(&fixtures().join("one/themes/one.json"), &dest).unwrap();
        assert_eq!(r.id, "one");
        assert_eq!(r.colors, vec!["One Dark", "One Light"]);
        assert!(dest.join("one/themes/one.json").is_file());
        let ext = read_extension(&dest.join("one")).unwrap();
        assert_eq!(ext.name, "One");
        // An icon theme JSON inside icon_themes/ brings its icons along.
        let r = import_path(
            &fixtures().join("tiny-icons/icon_themes/tiny-icons.json"),
            &dest,
        )
        .unwrap();
        assert!(r.dest.join("icons/readme.svg").is_file());
    }

    #[test]
    fn rejects_non_themes_without_writing() {
        let dest = crate::test_ui::temp_dir("zed-import-bad");
        let src = crate::test_ui::temp_dir("zed-import-bad-src");
        std::fs::write(src.join("x.json"), r#"{"hello":1}"#).unwrap();
        std::fs::write(src.join("y.json"), "{oops").unwrap();
        assert!(import_path(&src.join("x.json"), &dest).is_err());
        assert!(import_path(&src.join("y.json"), &dest).is_err());
        assert!(import_path(&src, &dest).is_err(), "no themes/ dir");
        assert!(import_path(&src.join("nope.json"), &dest).is_err());
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 0);
        assert_eq!(slug("My Theme!"), "my-theme");
        assert_eq!(slug("../.."), "theme");
    }
}
