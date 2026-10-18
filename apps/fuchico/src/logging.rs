//! The app logger, matching the original's `tauri_plugin_log::Builder::new()`
//! defaults:
//!
//! * targets: stdout and a file in the app log dir
//!   (`TargetKind::LogDir { file_name: None }`, i.e. `<productName>.log`);
//! * level: `Trace` (override with `RUST_LOG=<level>`);
//! * line format `[YYYY-MM-DD][HH:MM:SS][target][LEVEL] message`, UTC;
//! * rotation `KeepOne` at 40 000 bytes (the full file is deleted and a
//!   fresh one started), opened in append mode.
//!
//! Log dir (Tauri's `app_log_dir()`): macOS `~/Library/Logs/<identifier>`,
//! elsewhere `<data_local_dir>/<identifier>/logs`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fuchico_core::paths::BUNDLE_IDENTIFIER;
use log::{LevelFilter, Log, Metadata, Record};

pub const PRODUCT_NAME: &str = "Fuchico";
/// `DEFAULT_MAX_FILE_SIZE` of tauri-plugin-log.
pub const MAX_FILE_SIZE: u64 = 40_000;

/// Tauri's `app_log_dir()`.
pub fn log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Logs").join(BUNDLE_IDENTIFIER))
    } else {
        dirs::data_local_dir().map(|d| d.join(BUNDLE_IDENTIFIER).join("logs"))
    }
}

/// `<dir>/<productName>.log`.
pub fn log_file(dir: &Path) -> PathBuf {
    dir.join(format!("{PRODUCT_NAME}.log"))
}

/// One `KeepOne` rotating file.
struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max: u64,
}

impl RotatingFile {
    fn open(path: PathBuf, max: u64) -> std::io::Result<RotatingFile> {
        let mut f = RotatingFile { path, file: None, size: 0, max };
        f.reopen()?;
        if f.size >= f.max {
            f.rotate()?;
        }
        Ok(f)
    }

    fn reopen(&mut self) -> std::io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        if let Some(mut f) = self.file.take() {
            let _ = f.flush();
        }
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        self.reopen()
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        if self.size >= self.max {
            self.rotate()?;
        }
        if let Some(f) = &mut self.file {
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
            self.size += line.len() as u64 + 1;
        }
        Ok(())
    }
}

struct Logger {
    level: LevelFilter,
    stdout: bool,
    file: Option<Mutex<RotatingFile>>,
}

/// `[2026-10-18][12:34:56][target][INFO] message` (UTC, like the plugin's
/// default `TimezoneStrategy::UseUtc`).
pub fn format_line(now: chrono::DateTime<chrono::Utc>, target: &str, level: log::Level, msg: &str) -> String {
    format!("{}[{target}][{level}] {msg}", now.format("[%Y-%m-%d][%H:%M:%S]"))
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.level
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format_line(chrono::Utc::now(), r.target(), r.level(), &r.args().to_string());
        if self.stdout {
            let _ = writeln!(std::io::stdout().lock(), "{line}");
        }
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
        {
            let _ = f.write_line(&line);
        }
    }

    fn flush(&self) {
        let _ = std::io::stdout().flush();
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
            && let Some(file) = &mut f.file
        {
            let _ = file.flush();
        }
    }
}

fn level_from_env() -> LevelFilter {
    std::env::var("RUST_LOG").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(LevelFilter::Trace)
}

/// Installs the logger (once, at startup). A log dir that can't be created
/// leaves stdout logging only.
pub fn init() {
    let file = log_dir().and_then(|dir| {
        fs::create_dir_all(&dir).ok()?;
        RotatingFile::open(log_file(&dir), MAX_FILE_SIZE).ok()
    });
    let level = level_from_env();
    let logger = Logger { level, stdout: true, file: file.map(Mutex::new) };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn line_format_matches_the_plugin() {
        let t = chrono::Utc.with_ymd_and_hms(2026, 10, 18, 9, 5, 7).unwrap();
        assert_eq!(
            format_line(t, "fuchico::workspace", log::Level::Warn, "gone"),
            "[2026-10-18][09:05:07][fuchico::workspace][WARN] gone"
        );
    }

    #[test]
    fn log_dir_follows_tauri() {
        let d = log_dir().unwrap();
        if cfg!(target_os = "macos") {
            assert!(d.ends_with("Library/Logs/dev.fuleco.fuchico"));
        } else {
            assert!(d.ends_with("dev.fuleco.fuchico/logs"));
        }
        assert!(log_file(&d).ends_with("Fuchico.log"));
    }

    #[test]
    fn keep_one_rotation_drops_the_full_file() {
        let dir = crate::test_ui::temp_dir("log");
        let path = log_file(&dir);
        let mut f = RotatingFile::open(path.clone(), 30).unwrap();
        f.write_line("first line, 20 chars").unwrap();
        f.write_line("second line, 21 char").unwrap();
        // 42 bytes ≥ 30: the next write starts a fresh file.
        f.write_line("third").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "third\n");
        // Reopening appends.
        drop(f);
        let mut f = RotatingFile::open(path.clone(), 1000).unwrap();
        f.write_line("fourth").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "third\nfourth\n");
    }
}
