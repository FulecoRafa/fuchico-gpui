//! Files handed to the app by the OS (issue #38): double-clicking a `.md`
//! after Fuchico is the default handler, `open -a Fuchico note.md`, or a
//! path on the command line.
//!
//! macOS delivers them through `RunEvent::Opened` (possibly before the
//! webview exists); Linux/Windows pass them as argv, and a second launch is
//! forwarded here by the single-instance plugin. Either way the paths are
//! queued in [`OpenRequests`] and announced with `app:open-files`; the
//! frontend drains the queue once it is ready, so nothing is lost when the
//! event fires early.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::events::{CoreEvent, EventSink};

pub const OPEN_FILES_EVENT: &str = "app:open-files";

#[derive(Default)]
pub struct OpenRequests(Mutex<Vec<String>>);

impl OpenRequests {
    /// Queues `paths` and announces them with [`CoreEvent::OpenFiles`]. The
    /// caller is responsible for focusing the main window (was
    /// `main.set_focus()` under Tauri).
    pub fn push(&self, sink: &dyn EventSink, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        {
            let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
            queue.extend(paths.iter().cloned());
        }
        sink.emit(CoreEvent::OpenFiles(paths));
    }

    /// Returns and clears every queued path (was `open_requests_take`).
    pub fn take(&self) -> Vec<String> {
        let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *queue)
    }
}

/// Picks the file paths out of a process argument list. Flags are skipped;
/// relative paths are resolved against `cwd` so a shell `fuchico note.md`
/// works.
pub fn paths_from_args<I, S>(args: I, cwd: Option<&Path>) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .map(|a| a.as_ref().to_string())
        .filter(|a| !a.starts_with('-') && !a.is_empty())
        .filter_map(|a| {
            let p = PathBuf::from(&a);
            let p = if p.is_absolute() { p } else { cwd?.join(p) };
            p.is_file().then(|| p.to_string_lossy().into_owned())
        })
        .collect()
}

/// Callbacks for [`route_open_request`] (was `OpenRequestHandlers`).
pub trait OpenRequestHandlers {
    /// Adopt `dir` as the vault.
    fn set_root(&mut self, dir: &str);
    /// Open a note inside the current vault in a tab.
    fn open_in_vault(&mut self, path: &str);
    /// Open a note from elsewhere in its own single-file window.
    fn open_elsewhere(&mut self, path: &str);
}

fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) if i > 0 => &path[..i],
        _ => "/",
    }
}

pub fn is_inside(root: &str, path: &str) -> bool {
    if root.ends_with('/') {
        path.starts_with(root)
    } else {
        path.starts_with(&format!("{root}/"))
    }
}

/// Decides where a file handed over by the OS should open (issue #38): no
/// vault yet -> its folder becomes the vault; inside the vault -> a tab;
/// anywhere else -> a standalone editor window. Returns the root in effect
/// afterwards so a batch of files is routed consistently.
pub fn route_open_request(
    path: &str,
    root: Option<&str>,
    h: &mut dyn OpenRequestHandlers,
) -> Option<String> {
    match root {
        None => {
            let dir = dirname(path).to_string();
            h.set_root(&dir);
            h.open_in_vault(path);
            Some(dir)
        }
        Some(root) => {
            if is_inside(root, path) {
                h.open_in_vault(path);
            } else {
                h.open_elsewhere(path);
            }
            Some(root.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_flags_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("note.md");
        std::fs::write(&note, "# hi").unwrap();
        let got = paths_from_args(["--flag", "note.md", "missing.md", ""], Some(dir.path()));
        assert_eq!(got, vec![note.to_string_lossy().into_owned()]);
    }

    #[test]
    fn push_queues_and_take_drains() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let q = OpenRequests::default();
        q.push(&tx, vec![]);
        assert!(rx.try_recv().is_err());
        q.push(&tx, vec!["/a.md".into()]);
        assert_eq!(
            rx.try_recv().unwrap(),
            CoreEvent::OpenFiles(vec!["/a.md".into()])
        );
        assert_eq!(q.take(), vec!["/a.md".to_string()]);
        assert!(q.take().is_empty());
    }

    #[test]
    fn relative_without_cwd_is_dropped() {
        assert!(paths_from_args(["note.md"], None).is_empty());
    }

    #[derive(Default)]
    struct Rec {
        set_root: Vec<String>,
        in_vault: Vec<String>,
        elsewhere: Vec<String>,
    }
    impl OpenRequestHandlers for Rec {
        fn set_root(&mut self, d: &str) {
            self.set_root.push(d.into());
        }
        fn open_in_vault(&mut self, p: &str) {
            self.in_vault.push(p.into());
        }
        fn open_elsewhere(&mut self, p: &str) {
            self.elsewhere.push(p.into());
        }
    }

    #[test]
    fn adopts_the_files_folder_when_no_vault_is_open() {
        let mut h = Rec::default();
        let root = route_open_request("/v/notes/a.md", None, &mut h);
        assert_eq!(root.as_deref(), Some("/v/notes"));
        assert_eq!(h.set_root, ["/v/notes"]);
        assert_eq!(h.in_vault, ["/v/notes/a.md"]);
    }

    #[test]
    fn opens_a_tab_for_files_inside_the_vault() {
        let mut h = Rec::default();
        route_open_request("/v/sub/a.md", Some("/v"), &mut h);
        assert_eq!(h.in_vault, ["/v/sub/a.md"]);
        assert!(h.elsewhere.is_empty());
    }

    #[test]
    fn opens_a_window_for_files_outside_the_vault() {
        let mut h = Rec::default();
        route_open_request("/other/a.md", Some("/v"), &mut h);
        assert_eq!(h.elsewhere, ["/other/a.md"]);
        assert!(h.set_root.is_empty());
    }

    #[test]
    fn sibling_with_same_prefix_is_not_inside() {
        assert!(!is_inside("/v", "/vault/a.md"));
        assert!(is_inside("/v/", "/v/a.md"));
    }
}
