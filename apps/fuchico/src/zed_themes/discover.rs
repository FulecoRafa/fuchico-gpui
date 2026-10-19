//! Finding Zed extensions on disk: `<dir>/<extension>/extension.toml` plus
//! `themes/*.json` (colour themes) and `icon_themes/*.json` (icon themes).
//! Files listed in the manifest's `themes` / `icon_themes` arrays are
//! included too. Anything unreadable is logged and skipped.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One extension directory.
#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    /// `id` from `extension.toml`, else the directory name.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub root: PathBuf,
    pub color_files: Vec<PathBuf>,
    pub icon_files: Vec<PathBuf>,
}

#[derive(Deserialize, Default)]
struct Manifest {
    id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    themes: Vec<String>,
    #[serde(default)]
    icon_themes: Vec<String>,
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
        .collect()
}

/// Reads one extension directory; `None` when it holds no theme files.
pub fn read_extension(root: &Path) -> Option<Extension> {
    let manifest: Manifest = match std::fs::read_to_string(root.join("extension.toml")) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            log::warn!("{}: extension.toml: {e}", root.display());
            Manifest::default()
        }),
        Err(_) => Manifest::default(),
    };
    let listed = |list: &[String]| -> Vec<PathBuf> {
        list.iter()
            .filter_map(|s| super::icons::rel_icon_path(s))
            .map(|p| root.join(p))
            .filter(|p| p.is_file())
            .collect()
    };
    let merge = |mut a: Vec<PathBuf>, b: Vec<PathBuf>| {
        a.extend(b);
        a.sort();
        a.dedup();
        a
    };
    let color_files = merge(json_files(&root.join("themes")), listed(&manifest.themes));
    let icon_files = merge(
        json_files(&root.join("icon_themes")),
        listed(&manifest.icon_themes),
    );
    if color_files.is_empty() && icon_files.is_empty() {
        return None;
    }
    let dir_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "extension".into());
    let id = manifest
        .id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(dir_name);
    Some(Extension {
        name: manifest
            .name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| id.clone()),
        id,
        version: manifest.version,
        root: root.to_path_buf(),
        color_files,
        icon_files,
    })
}

/// Every extension directly under `dir` (sorted by id).
pub fn list_extensions(dir: &Path) -> Vec<Extension> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Extension> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .filter_map(|p| read_extension(&p))
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Zed's installed-extensions directories for this OS:
/// macOS `~/Library/Application Support/Zed/extensions/installed`, Linux
/// `$XDG_DATA_HOME/zed/extensions/installed` (`~/.local/share/…`), Windows
/// `%LOCALAPPDATA%\Zed\extensions\installed`.
/// `FUCHICO_ZED_EXTENSIONS_DIR` replaces them (dev / tests).
pub fn zed_installed_dirs() -> Vec<PathBuf> {
    if let Some(over) = std::env::var_os("FUCHICO_ZED_EXTENSIONS_DIR") {
        return std::env::split_paths(&over).collect();
    }
    let mut out = Vec::new();
    if cfg!(target_os = "macos") {
        if let Some(d) = dirs::data_dir() {
            out.push(d.join("Zed/extensions/installed"));
        }
    } else if cfg!(target_os = "windows") {
        if let Some(d) = dirs::data_local_dir() {
            out.push(d.join("Zed\\extensions\\installed"));
        }
    } else if let Some(d) = dirs::data_dir() {
        out.push(d.join("zed/extensions/installed"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_extensions_with_manifests_and_globs() {
        let dir = crate::test_ui::temp_dir("zed-discover");
        // Fixture extension (colour themes).
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/zed_themes/fixtures");
        let one = dir.join("one");
        std::fs::create_dir_all(one.join("themes")).unwrap();
        std::fs::copy(
            fixtures.join("one/extension.toml"),
            one.join("extension.toml"),
        )
        .unwrap();
        std::fs::copy(
            fixtures.join("one/themes/one.json"),
            one.join("themes/one.json"),
        )
        .unwrap();
        // No manifest, icon theme only.
        let ic = dir.join("my-icons");
        std::fs::create_dir_all(ic.join("icon_themes")).unwrap();
        std::fs::write(ic.join("icon_themes/i.json"), "{}").unwrap();
        std::fs::write(ic.join("icon_themes/readme.txt"), "").unwrap();
        // Broken manifest + nothing else → skipped.
        let junk = dir.join("junk");
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(junk.join("extension.toml"), "id = [").unwrap();
        // Hidden dirs (import scratch) are ignored.
        std::fs::create_dir_all(dir.join(".tmp/themes")).unwrap();
        std::fs::write(dir.join(".tmp/themes/x.json"), "{}").unwrap();

        let exts = list_extensions(&dir);
        let ids: Vec<_> = exts.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["my-icons", "one-fixture"]);
        let one = &exts[1];
        assert_eq!(one.name, "One (fixture)");
        assert_eq!(one.version.as_deref(), Some("0.0.1"));
        assert_eq!(one.color_files.len(), 1, "manifest + glob dedupe");
        assert!(one.icon_files.is_empty());
        assert_eq!(exts[0].name, "my-icons");
        assert_eq!(exts[0].icon_files.len(), 1);
        assert!(list_extensions(&dir.join("missing")).is_empty());
    }
}
