//! Tiny key → JSON-file store in `fuchico_core::paths::app_config_dir()`.
//!
//! Each former `localStorage` key gets its own file named after the key
//! (`helix.editorSettings.json`, ...), holding the same JSON the web app
//! stored, so values can be imported verbatim.
//!
//! * Reads are synchronous and only happen at startup, before the first
//!   window opens ([`load`]).
//! * Writes never block the caller: they are queued to one writer thread
//!   that applies them in order and writes atomically (tmp file + rename).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub const KEY_SETTINGS: &str = "helix.editorSettings";
pub const KEY_LAST_ROOT: &str = "helix.lastRootPath";
pub const KEY_SCROLL_POSITIONS: &str = "helix.scrollPositions";
pub const KEY_SETTINGS_SECTION: &str = "helix.settingsSection";
pub const KEY_WINDOW_STATE: &str = "helix.windowState";

enum Job {
    Write(PathBuf, String),
    Remove(PathBuf),
    Barrier(Sender<()>),
}

fn dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Ok(over) = std::env::var("FUCHICO_CONFIG_DIR") {
            return Some(PathBuf::from(over));
        }
        fuchico_core::paths::app_config_dir()
    })
    .clone()
}

/// The directory config files live in (created lazily on first write).
pub fn config_dir() -> Option<PathBuf> {
    dir()
}

fn path_for(key: &str) -> Option<PathBuf> {
    dir().map(|d| d.join(format!("{key}.json")))
}

fn writer() -> &'static Sender<Job> {
    static TX: OnceLock<Sender<Job>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("fuchico-persist".into())
            .spawn(move || {
                for job in rx {
                    if let Err(err) = run(job) {
                        log::warn!("persist: {err}");
                    }
                }
            })
            .expect("spawn persist thread");
        tx
    })
}

fn run(job: Job) -> std::io::Result<()> {
    match job {
        Job::Write(path, body) => write_atomic(&path, &body),
        Job::Barrier(done) => {
            let _ = done.send(());
            Ok(())
        }
        Job::Remove(path) => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

fn write_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

/// Raw file contents for `key`, if present. Startup only.
pub fn load_raw(key: &str) -> Option<String> {
    std::fs::read_to_string(path_for(key)?).ok()
}

/// Parsed value for `key`; `None` if missing or unparsable. Startup only.
pub fn load<T: DeserializeOwned>(key: &str) -> Option<T> {
    serde_json::from_str(&load_raw(key)?).ok()
}

/// Queues a write of `value` under `key`.
pub fn save<T: Serialize + ?Sized>(key: &str, value: &T) {
    let Some(path) = path_for(key) else { return };
    match serde_json::to_string(value) {
        Ok(body) => {
            let _ = writer().send(Job::Write(path, body));
        }
        Err(err) => log::warn!("persist: serialize {key}: {err}"),
    }
}

/// Queues removal of `key` (`localStorage.removeItem`).
pub fn remove(key: &str) {
    if let Some(path) = path_for(key) {
        let _ = writer().send(Job::Remove(path));
    }
}

/// Blocks (up to 2s) until every queued write has been applied. Quit only.
pub fn flush() {
    let (tx, rx) = channel::<()>();
    if writer().send(Job::Barrier(tx)).is_ok() {
        let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
    }
}
