//! Per-app locations, laid out like Tauri's path resolver. The identifier
//! differs from the Tauri build's (`dev.fuleco.fuchico`) so both apps can be
//! installed side by side with their own data (see docs/DECISIONS.md).

use std::path::PathBuf;

pub const BUNDLE_IDENTIFIER: &str = "dev.fuleco.fuchico-gpui";

/// Tauri's `app_data_dir()`: `<data_dir>/<identifier>`
/// (macOS `~/Library/Application Support/dev.fuleco.fuchico-gpui`,
/// Linux `~/.local/share/dev.fuleco.fuchico-gpui`, Windows `%APPDATA%\dev.fuleco.fuchico-gpui`).
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
            assert!(d.ends_with("dev.fuleco.fuchico-gpui"));
        }
        if let Some(d) = app_config_dir() {
            assert!(d.ends_with("dev.fuleco.fuchico-gpui"));
        }
    }
}
