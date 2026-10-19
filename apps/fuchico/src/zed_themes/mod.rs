//! Zed editor extensions as a source of colour themes and file icon themes
//! (an extra over the original Tauri app; see docs/ZED_THEMES.md).
//!
//! * Sources ([`default_roots`]): Fuchico's own imported copies in
//!   `<config dir>/zed-themes/<extension id>/` (see [`import`]) and Zed's
//!   installed extensions (see [`discover::zed_installed_dirs`]). An
//!   imported theme wins over an installed one with the same name.
//! * [`ZedThemes`] (a `Global`) is the scanned registry; [`reload`] rescans
//!   (after an import), and the theme observer re-resolves the app theme.
//! * Settings keep the *names*: `zedTheme` (with `palette: "zed"`) and
//!   `iconTheme` (absent = the built-in Lucide icons).

pub mod color;
pub mod discover;
pub mod icons;
pub mod import;

#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod ui_tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{App, Global};

pub use color::{ZedColorTheme, ZedThemeFamily};
pub use icons::IconTheme;

use crate::theme::Appearance;

/// Where a theme was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Copied into Fuchico's config dir.
    Imported,
    /// Read in place from Zed's extensions dir.
    Installed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorThemeEntry {
    pub theme: ZedColorTheme,
    /// Identifies the family (the theme file) for light/dark counterparts.
    pub family: PathBuf,
    pub extension: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IconThemeEntry {
    pub theme: Arc<IconTheme>,
    pub extension: String,
    pub source: Source,
}

/// Every Zed colour and icon theme found, deduplicated by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZedThemes {
    pub colors: Vec<Arc<ColorThemeEntry>>,
    pub icons: Vec<Arc<IconThemeEntry>>,
}

impl Global for ZedThemes {}

/// The imported-themes directory: `<config dir>/zed-themes`.
pub fn imported_dir() -> Option<PathBuf> {
    crate::persist::config_dir().map(|d| d.join("zed-themes"))
}

/// Imported copies first (they win on name clashes), then Zed's installed
/// extensions. Tests only see the imported dir of their scratch config.
pub fn default_roots() -> Vec<(PathBuf, Source)> {
    let mut roots: Vec<(PathBuf, Source)> = imported_dir()
        .into_iter()
        .map(|d| (d, Source::Imported))
        .collect();
    if !cfg!(test) {
        roots.extend(
            discover::zed_installed_dirs()
                .into_iter()
                .map(|d| (d, Source::Installed)),
        );
    }
    roots
}

/// Icon theme roots: the extension root (parent of `icon_themes/`).
fn ext_root_of(json: &Path, ext_root: &Path) -> PathBuf {
    match json.parent() {
        Some(p) if p.file_name().is_some_and(|n| n == "icon_themes") => {
            p.parent().unwrap_or(ext_root).to_path_buf()
        }
        _ => ext_root.to_path_buf(),
    }
}

impl ZedThemes {
    /// Scans `roots` (each a directory of extension directories).
    pub fn scan(roots: &[(PathBuf, Source)]) -> ZedThemes {
        let mut out = ZedThemes::default();
        for (dir, source) in roots {
            for ext in discover::list_extensions(dir) {
                out.add_extension(&ext, *source);
            }
        }
        out.colors.sort_by_key(|c| c.theme.name.to_lowercase());
        out.icons.sort_by_key(|c| c.theme.name.to_lowercase());
        out
    }

    fn add_extension(&mut self, ext: &discover::Extension, source: Source) {
        for file in &ext.color_files {
            let parsed = std::fs::read_to_string(file)
                .map_err(|e| e.to_string())
                .and_then(|t| ZedThemeFamily::parse(&t));
            match parsed {
                Ok(f) => {
                    for theme in f.themes {
                        if self.color(&theme.name).is_some() {
                            continue;
                        }
                        self.colors.push(Arc::new(ColorThemeEntry {
                            theme,
                            family: file.clone(),
                            extension: ext.id.clone(),
                            source,
                        }));
                    }
                }
                Err(e) => log::warn!("zed theme {}: {e}", file.display()),
            }
        }
        for file in &ext.icon_files {
            let root = ext_root_of(file, &ext.root);
            let parsed = std::fs::read_to_string(file)
                .map_err(|e| e.to_string())
                .and_then(|t| IconTheme::parse_family(&t, &root));
            match parsed {
                Ok(themes) => {
                    for theme in themes {
                        if self.icon(&theme.name).is_some() {
                            continue;
                        }
                        self.icons.push(Arc::new(IconThemeEntry {
                            theme: Arc::new(theme),
                            extension: ext.id.clone(),
                            source,
                        }));
                    }
                }
                Err(e) => log::warn!("zed icon theme {}: {e}", file.display()),
            }
        }
    }

    pub fn get(cx: &App) -> &ZedThemes {
        static EMPTY: ZedThemes = ZedThemes {
            colors: Vec::new(),
            icons: Vec::new(),
        };
        cx.try_global::<ZedThemes>().unwrap_or(&EMPTY)
    }

    pub fn color(&self, name: &str) -> Option<&Arc<ColorThemeEntry>> {
        self.colors.iter().find(|c| c.theme.name == name)
    }

    pub fn icon(&self, name: &str) -> Option<&Arc<IconThemeEntry>> {
        self.icons.iter().find(|c| c.theme.name == name)
    }

    /// The theme to paint with for the selected `name` when the app wants
    /// `want`: the theme itself if it matches, else its counterpart in the
    /// same family (the name with Dark ↔ Light swapped, else the first
    /// theme of that appearance), else the theme itself (fixed appearance,
    /// like Dracula).
    pub fn variant(&self, name: &str, want: Appearance) -> Option<&Arc<ColorThemeEntry>> {
        let sel = self.color(name)?;
        if sel.theme.appearance == want {
            return Some(sel);
        }
        Some(self.counterpart(sel, want).unwrap_or(sel))
    }

    fn counterpart(
        &self,
        sel: &ColorThemeEntry,
        want: Appearance,
    ) -> Option<&Arc<ColorThemeEntry>> {
        let siblings: Vec<_> = self
            .colors
            .iter()
            .filter(|c| c.family == sel.family && c.theme.appearance == want)
            .collect();
        let swapped = swap_light_dark(&sel.theme.name);
        siblings
            .iter()
            .find(|c| Some(&c.theme.name) == swapped.as_ref())
            .or_else(|| siblings.first())
            .map(|c| &**c)
    }

    /// Whether `name`'s family has a theme of the other appearance (else
    /// the light/dark mode setting does not apply).
    pub fn has_counterpart(&self, name: &str) -> bool {
        self.color(name).is_some_and(|sel| {
            let other = if sel.theme.appearance.is_dark() {
                Appearance::Light
            } else {
                Appearance::Dark
            };
            self.counterpart(sel, other).is_some()
        })
    }
}

fn swap_light_dark(name: &str) -> Option<String> {
    for (a, b) in [
        ("Dark", "Light"),
        ("Light", "Dark"),
        ("dark", "light"),
        ("light", "dark"),
    ] {
        if name.contains(a) {
            return Some(name.replacen(a, b, 1));
        }
    }
    None
}

/// Rescans every source and installs the registry (the theme observer
/// re-resolves the app theme when it changed).
pub fn reload(cx: &mut App) {
    let next = ZedThemes::scan(&default_roots());
    if cx.try_global::<ZedThemes>() != Some(&next) {
        cx.set_global(next);
        cx.refresh_windows();
    }
}

/// Whether the selected Zed theme pins the appearance (palette is Zed and
/// the theme has no light/dark counterpart), like Dracula.
pub fn fixed_appearance(s: &crate::settings::Settings, cx: &App) -> bool {
    s.palette == crate::settings::PaletteId::Zed
        && s.zed_theme.as_deref().is_some_and(|n| {
            let zed = ZedThemes::get(cx);
            zed.color(n).is_some() && !zed.has_counterpart(n)
        })
}

/// Startup scan (before `theme::init`).
pub fn init(cx: &mut App) {
    reload(cx);
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    pub fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/zed_themes/fixtures")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> (ZedThemes, PathBuf) {
        let dir = crate::test_ui::temp_dir("zed-registry");
        let fx = test_support::fixtures();
        import::import_path(&fx.join("one"), &dir).unwrap();
        import::import_path(&fx.join("tiny-icons"), &dir).unwrap();
        (ZedThemes::scan(&[(dir.clone(), Source::Imported)]), dir)
    }

    #[test]
    fn scan_finds_imported_colour_and_icon_themes() {
        let (r, dir) = registry();
        let names: Vec<_> = r.colors.iter().map(|c| c.theme.name.as_str()).collect();
        assert_eq!(names, vec!["One Dark", "One Light"]);
        assert_eq!(r.icons.len(), 1);
        let ic = r.icon("Tiny Icons").unwrap();
        assert_eq!(ic.extension, "tiny-icons");
        assert_eq!(
            ic.theme.file_icon("a.md").unwrap(),
            dir.join("tiny-icons/icons/markdown.svg")
        );
        // Imported copies shadow an installed extension with the same names.
        let both = ZedThemes::scan(&[
            (dir.clone(), Source::Imported),
            (test_support::fixtures(), Source::Installed),
        ]);
        assert_eq!(both.colors.len(), 2);
        assert!(both.colors.iter().all(|c| c.source == Source::Imported));
    }

    #[test]
    fn variant_follows_mode_within_the_family() {
        let (r, _) = registry();
        let v = |n, a| r.variant(n, a).unwrap().theme.name.clone();
        assert_eq!(v("One Dark", Appearance::Light), "One Light");
        assert_eq!(v("One Light", Appearance::Dark), "One Dark");
        assert_eq!(v("One Dark", Appearance::Dark), "One Dark");
        assert!(r.has_counterpart("One Dark"));
        assert!(r.variant("Nope", Appearance::Dark).is_none());
        assert_eq!(
            swap_light_dark("Gruvbox Dark Hard").as_deref(),
            Some("Gruvbox Light Hard")
        );
    }

    #[test]
    fn single_appearance_theme_keeps_its_appearance() {
        let dir = crate::test_ui::temp_dir("zed-single");
        import::import_path(&test_support::fixtures().join("midnight.json"), &dir).unwrap();
        let r = ZedThemes::scan(&[(dir, Source::Imported)]);
        let t = r.variant("Midnight", Appearance::Light).unwrap();
        assert_eq!(t.theme.appearance, Appearance::Dark);
        assert!(!r.has_counterpart("Midnight"));
    }

    #[test]
    fn broken_files_are_skipped() {
        let dir = crate::test_ui::temp_dir("zed-broken");
        let ext = dir.join("bad");
        std::fs::create_dir_all(ext.join("themes")).unwrap();
        std::fs::create_dir_all(ext.join("icon_themes")).unwrap();
        std::fs::write(ext.join("themes/a.json"), "{ not json").unwrap();
        std::fs::write(ext.join("icon_themes/b.json"), "[]").unwrap();
        let r = ZedThemes::scan(&[(dir, Source::Installed)]);
        assert!(r.colors.is_empty() && r.icons.is_empty());
    }
}
