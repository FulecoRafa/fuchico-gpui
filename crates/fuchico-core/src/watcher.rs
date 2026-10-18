//! Vault file watcher. The Tauri app has no OS-level watcher: the frontend
//! calls `notifyFsChanged(dir)` after its own mutations, which emits
//! `fs:changed` with `{ dir, from }`. This module provides the equivalent for
//! changes made *outside* the app: it emits [`CoreEvent::FsChanged`] with the
//! parent directory of each changed path (deduplicated, debounced), which is
//! exactly the granularity the explorer / vault-file / backlink listeners use.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::events::{CoreEvent, EventSink};
use crate::fs::to_canon;

const DEBOUNCE: Duration = Duration::from_millis(250);
const IGNORED_DIRS: &[&str] = &["node_modules", "target", "dist", ".git"];

/// Keeps the watch alive; dropping it stops watching.
pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
}

/// Directory an event path should be reported as (`FsChanged.dir`), or
/// `None` if the path is noise (ignored dirs, atomic-write temp files).
pub fn changed_dir(path: &Path) -> Option<String> {
    if path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| IGNORED_DIRS.contains(&s))
    }) {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    // `tempfile::NamedTempFile` staging files used by `write_atomic`.
    if name.starts_with(".tmp") {
        return None;
    }
    Some(to_canon(path.parent()?))
}

fn relevant(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
    )
}

impl VaultWatcher {
    /// Watches `root` recursively and emits [`CoreEvent::FsChanged`] on `sink`.
    pub fn start(root: impl AsRef<Path>, sink: Arc<dyn EventSink>) -> notify::Result<Self> {
        let (tx, rx) = crossbeam_channel::unbounded::<Event>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(ev) = res {
                let _ = tx.send(ev);
            }
        })?;
        watcher.watch(root.as_ref(), RecursiveMode::Recursive)?;

        std::thread::Builder::new()
            .name("fuchico-watcher".into())
            .spawn(move || {
                // Ends when the watcher (and with it `tx`) is dropped.
                while let Ok(first) = rx.recv() {
                    let mut dirs: BTreeSet<String> = BTreeSet::new();
                    let mut absorb = |ev: Event| {
                        if relevant(&ev.kind) {
                            dirs.extend(ev.paths.iter().filter_map(|p| changed_dir(p)));
                        }
                    };
                    absorb(first);
                    while let Ok(next) = rx.recv_timeout(DEBOUNCE) {
                        absorb(next);
                    }
                    for dir in dirs {
                        sink.emit(CoreEvent::FsChanged { dir, from: None });
                    }
                }
            })
            .map_err(notify::Error::io)?;
        Ok(Self { _watcher: watcher })
    }
}

/// Convenience: watch with a plain `PathBuf` root.
pub fn watch_vault(root: PathBuf, sink: Arc<dyn EventSink>) -> notify::Result<VaultWatcher> {
    VaultWatcher::start(root, sink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_dir_is_parent_and_filters_noise() {
        assert_eq!(changed_dir(Path::new("/v/notes/a.md")), Some("/v/notes".to_string()));
        assert_eq!(changed_dir(Path::new("/v/.git/HEAD")), None);
        assert_eq!(changed_dir(Path::new("/v/node_modules/x/y.js")), None);
        assert_eq!(changed_dir(Path::new("/v/notes/.tmpAbC123")), None);
    }

    #[test]
    fn emits_fs_changed_for_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let _w = VaultWatcher::start(&root, Arc::new(tx)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(root.join("new.md"), "x").unwrap();
        let ev = rx.recv_timeout(Duration::from_secs(5)).expect("no event");
        match ev {
            CoreEvent::FsChanged { dir, from } => {
                assert_eq!(dir, to_canon(&root));
                assert!(from.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
