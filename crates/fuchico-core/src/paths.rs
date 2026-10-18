//! Locations shared with the Tauri build of Fuchico (bundle identifier from
//! `src-tauri/tauri.conf.json`).

use std::path::PathBuf;

pub const BUNDLE_IDENTIFIER: &str = "dev.fuleco.fuchico";

/// Tauri's `app_data_dir()`: `<data_dir>/<identifier>`
/// (macOS `~/Library/Application Support/dev.fuleco.fuchico`,
/// Linux `~/.local/share/dev.fuleco.fuchico`, Windows `%APPDATA%\dev.fuleco.fuchico`).
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(BUNDLE_IDENTIFIER))
}

/// Tauri's `app_config_dir()`: `<config_dir>/<identifier>`.
pub fn app_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(BUNDLE_IDENTIFIER))
}

/// Tauri's `app_local_data_dir()`.
pub fn app_local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(BUNDLE_IDENTIFIER))
}

/// Tauri's `app_cache_dir()`.
pub fn app_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join(BUNDLE_IDENTIFIER))
}

/// Directory holding the CalDAV `config.json` -- pass to the `caldav`
/// functions as `data_dir`.
pub fn caldav_data_dir() -> Option<PathBuf> {
    app_data_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_end_with_identifier() {
        if let Some(d) = app_data_dir() {
            assert!(d.ends_with("dev.fuleco.fuchico"));
        }
        if let Some(d) = app_config_dir() {
            assert!(d.ends_with("dev.fuleco.fuchico"));
        }
    }
}
