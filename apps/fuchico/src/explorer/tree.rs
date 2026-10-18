//! Explorer data: per-directory load state, the flattened row list and the
//! pure rules of the original `FileExplorer.tsx` / `useFileTree.ts`
//! (selection model, type-ahead, move rules, create/rename names, import
//! naming). No GPUI here, so all of it is unit tested.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use fuchico_core::fs::tree::{EntryKind, fs_read_dir};

/// `TYPE_AHEAD_RESET_MS`.
pub const TYPE_AHEAD_RESET_MS: u64 = 700;
/// `DROP_EXPAND_MS`: spring-loaded folder delay while dragging.
pub const DROP_EXPAND_MS: u64 = 600;
/// `NOTICE_MS`: import notice lifetime.
pub const NOTICE_MS: u64 = 4000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// Symlinks to dirs are reported as dirs by core; broken symlinks are
    /// treated as files.
    pub is_dir: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirState {
    Loading,
    Loaded(Vec<Entry>),
    Error(String),
}

#[derive(Default)]
pub struct Tree(HashMap<String, DirState>);

impl Tree {
    pub fn get(&self, dir: &str) -> Option<&DirState> {
        self.0.get(dir)
    }
    pub fn set(&mut self, dir: String, state: DirState) {
        self.0.insert(dir, state);
    }
    pub fn is_loaded(&self, dir: &str) -> bool {
        matches!(self.0.get(dir), Some(DirState::Loaded(_)))
    }
    pub fn contains(&self, dir: &str) -> bool {
        self.0.contains_key(dir)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingKind {
    File,
    Dir,
}

/// `PendingCreate`: an inline "new file/folder/drawing" input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCreate {
    pub parent: String,
    pub kind: PendingKind,
    /// Appended to the typed name when it has no `.` ("New drawing" only).
    pub default_ext: Option<&'static str>,
    /// Custom placeholder ("New drawing"); otherwise "New file"/"New folder".
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Entry { entry: Entry, depth: usize, expanded: bool },
    /// The entry being renamed (inline input).
    Rename { entry: Entry, depth: usize },
    /// The pending-create input inside an expanded folder.
    Pending { depth: usize, kind: PendingKind, placeholder: Option<String> },
    /// "Loading…" / error line under an expanded folder (`depth` is the
    /// row's own depth, i.e. the folder's depth + 1).
    Status { depth: usize, text: String, error: bool },
}

impl Row {
    /// The path of an entry row (not of rename/pending/status rows).
    pub fn entry_path(&self) -> Option<&str> {
        match self {
            Row::Entry { entry, .. } => Some(&entry.path),
            _ => None,
        }
    }
}

/// Blocking: call off the UI thread.
pub fn read_dir(dir: &str) -> DirState {
    match fs_read_dir(dir.to_string(), false) {
        Ok(entries) => DirState::Loaded(
            entries
                .into_iter()
                .map(|e| Entry {
                    path: join(dir, &e.name),
                    is_dir: matches!(e.kind, EntryKind::Dir),
                    name: e.name,
                })
                .collect(),
        ),
        Err(e) => DirState::Error(e),
    }
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

/// `dirname` of the original (`"/"` for top-level paths).
pub fn parent_dir(path: &str) -> Option<String> {
    let i = path.rfind('/')?;
    Some(if i == 0 { "/".to_string() } else { path[..i].to_string() })
}

fn dirname(path: &str) -> String {
    parent_dir(path).unwrap_or_else(|| "/".to_string())
}

pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

/// Depth-first walk over loaded, expanded folders.
pub fn flatten(
    tree: &Tree,
    root: &str,
    expanded: &HashSet<String>,
    renaming: Option<&str>,
    pending: Option<&PendingCreate>,
) -> Vec<Row> {
    let mut out = Vec::new();
    let ctx = Walk { tree, expanded, renaming, pending };
    ctx.walk(root, 0, &mut out);
    out
}

struct Walk<'a> {
    tree: &'a Tree,
    expanded: &'a HashSet<String>,
    renaming: Option<&'a str>,
    pending: Option<&'a PendingCreate>,
}

impl Walk<'_> {
    fn walk(&self, dir: &str, depth: usize, out: &mut Vec<Row>) {
        let Some(DirState::Loaded(entries)) = self.tree.get(dir) else { return };
        for e in entries {
            let open = e.is_dir && self.expanded.contains(&e.path);
            if self.renaming == Some(e.path.as_str()) {
                out.push(Row::Rename { entry: e.clone(), depth });
            } else {
                out.push(Row::Entry { entry: e.clone(), depth, expanded: open });
            }
            if !open {
                continue;
            }
            if let Some(p) = self.pending.filter(|p| p.parent == e.path) {
                out.push(Row::Pending { depth: depth + 1, kind: p.kind, placeholder: p.placeholder.clone() });
            }
            match self.tree.get(&e.path) {
                Some(DirState::Loaded(_)) => self.walk(&e.path, depth + 1, out),
                Some(DirState::Error(msg)) => {
                    out.push(Row::Status { depth: depth + 1, text: msg.clone(), error: true })
                }
                Some(DirState::Loading) => {
                    out.push(Row::Status { depth: depth + 1, text: crate::t!(CommonLoading), error: false })
                }
                None => {}
            }
        }
    }
}

/// Paths of the visible entry rows, in order (`entryPaths`).
pub fn entry_paths(rows: &[Row]) -> Vec<String> {
    rows.iter().filter_map(|r| r.entry_path().map(str::to_string)).collect()
}

/// `movePath` skip rules: not into its own parent, not into itself or a
/// descendant.
pub fn can_move(from: &str, to_dir: &str) -> bool {
    to_dir != dirname(from) && !format!("{to_dir}/").starts_with(&format!("{from}/"))
}

/// Where a drop/paste on a row lands: folders receive it, files stand in
/// for their parent folder.
pub fn drop_dir_for(path: &str, is_dir: bool) -> String {
    if is_dir { path.to_string() } else { dirname(path) }
}

/// `commitCreate` name: trimmed, `None` when empty; `default_ext` appended
/// when the name has no dot.
pub fn create_name(input: &str, default_ext: Option<&str>) -> Option<String> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    Some(match default_ext {
        Some(ext) if !t.contains('.') => format!("{t}.{ext}"),
        _ => t.to_string(),
    })
}

/// `commitRename` target: `None` when the trimmed name is empty or unchanged.
pub fn rename_target(from: &str, input: &str) -> Option<String> {
    let t = input.trim();
    if t.is_empty() || t == basename(from) {
        return None;
    }
    Some(join(&dirname(from), t))
}

/// `importFiles` collision name: `name-<ts>.ext` (before the last dot when
/// its index is > 0), else `name-<ts>`.
pub fn unique_import_name(name: &str, ts: &str) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{}-{ts}{}", &name[..dot], &name[dot..]),
        _ => format!("{name}-{ts}"),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportResult {
    pub imported: Vec<String>,
    pub errors: Vec<String>,
}

/// Copies OS files into `to_dir` without overwriting (blocking: run off
/// the UI thread). `ts` is the collision suffix (`attachments::timestamp`).
pub fn import_files(files: &[PathBuf], to_dir: &str, ts: &str) -> ImportResult {
    let mut out = ImportResult::default();
    for f in files {
        let Some(name) = f.file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()) else { continue };
        let res = std::fs::read(f).map_err(|e| e.to_string()).and_then(|bytes| {
            let path = join(to_dir, name);
            match fuchico_core::fs::file::fs_write_binary_bytes(&path, &bytes) {
                Ok(()) => Ok(path),
                Err(e) if e.contains("already exists") => {
                    let path = join(to_dir, &unique_import_name(name, ts));
                    fuchico_core::fs::file::fs_write_binary_bytes(&path, &bytes).map(|_| path)
                }
                Err(e) => Err(e),
            }
        });
        match res {
            Ok(p) => out.imported.push(p),
            Err(e) => out.errors.push(format!("{name}: {e}")),
        }
    }
    out
}

/// The multi-selection model: `focus` is `selectedPath` (keyboard cursor),
/// `paths` is `selectedPaths` (in insertion order), `anchor` is where Shift
/// ranges start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub focus: Option<String>,
    pub paths: Vec<String>,
    pub anchor: Option<String>,
}

impl Selection {
    pub fn contains(&self, path: &str) -> bool {
        self.paths.iter().any(|p| p == path)
    }

    /// `selectPath(path, {shift, toggle})` over the visible `entries`.
    pub fn select(&mut self, path: &str, shift: bool, toggle: bool, entries: &[String]) {
        self.focus = Some(path.to_string());
        if shift && let Some(anchor) = &self.anchor {
            let a = entries.iter().position(|p| p == anchor);
            let b = entries.iter().position(|p| p == path);
            if let (Some(a), Some(b)) = (a, b) {
                self.paths = entries[a.min(b)..=a.max(b)].to_vec();
                return;
            }
        }
        self.anchor = Some(path.to_string());
        if toggle {
            if let Some(i) = self.paths.iter().position(|p| p == path) {
                if self.paths.len() > 1 {
                    self.paths.remove(i);
                }
            } else {
                self.paths.push(path.to_string());
            }
            return;
        }
        self.paths = vec![path.to_string()];
    }

    /// Single selection that also becomes the anchor (active file).
    pub fn select_only(&mut self, path: &str) {
        self.focus = Some(path.to_string());
        self.paths = vec![path.to_string()];
        self.anchor = Some(path.to_string());
    }

    /// Mod-a.
    pub fn select_all(&mut self, entries: &[String]) {
        self.paths = entries.to_vec();
        if self.focus.as_ref().is_none_or(|f| !entries.contains(f)) {
            self.focus = entries.first().cloned();
        }
    }

    /// `targetsFor`: the whole selection when `path` is in it, else `path`.
    pub fn targets_for(&self, path: &str) -> Vec<String> {
        if self.contains(path) { self.paths.clone() } else { vec![path.to_string()] }
    }

    /// Drops paths whose rows are no longer visible; returns whether
    /// anything changed.
    pub fn prune(&mut self, visible: &HashSet<&str>) -> bool {
        let mut changed = false;
        if self.focus.as_deref().is_some_and(|f| !visible.contains(f)) {
            self.focus = None;
            changed = true;
        }
        let before = self.paths.len();
        self.paths.retain(|p| visible.contains(p.as_str()));
        changed || before != self.paths.len()
    }
}

/// Type-ahead target. `buffer` is the updated (lowercased) buffer, `names`
/// the visible entry basenames, `current` the focused index.
pub fn type_ahead_target(buffer: &str, names: &[String], current: Option<usize>) -> Option<usize> {
    let first = buffer.chars().next()?;
    let len = buffer.chars().count();
    let cycling = len > 1 && buffer.chars().all(|c| c == first);
    let prefix: String = if cycling { first.to_string() } else { buffer.to_string() };
    let cur = current.map(|c| c as isize).unwrap_or(-1);
    let start = if cycling || len == 1 { cur + 1 } else { cur };
    let n = names.len();
    let start = start.max(0) as usize;
    (0..n).map(|step| (start + step) % n).find(|&i| names[i].to_lowercase().starts_with(&prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(dir: &str, name: &str, is_dir: bool) -> Entry {
        Entry { name: name.into(), path: join(dir, name), is_dir }
    }

    fn label(r: &Row) -> String {
        match r {
            Row::Entry { entry, depth, .. } => format!("{depth}:{}", entry.name),
            Row::Rename { entry, depth } => format!("{depth}:rename:{}", entry.name),
            Row::Pending { depth, .. } => format!("{depth}:pending"),
            Row::Status { depth, .. } => format!("{depth}:status"),
        }
    }

    fn sample() -> (Tree, HashSet<String>) {
        let mut t = Tree::default();
        t.set("/v".into(), DirState::Loaded(vec![e("/v", "a", true), e("/v", "b", true), e("/v", "z.md", false)]));
        t.set("/v/a".into(), DirState::Loaded(vec![e("/v/a", "x.md", false)]));
        t.set("/v/b".into(), DirState::Loading);
        let ex: HashSet<String> = ["/v/a", "/v/b"].iter().map(|s| s.to_string()).collect();
        (t, ex)
    }

    #[test]
    fn flattens_expanded_folders_only() {
        let (t, ex) = sample();
        let rows: Vec<_> = flatten(&t, "/v", &ex, None, None).iter().map(label).collect();
        assert_eq!(rows, ["0:a", "1:x.md", "0:b", "1:status", "0:z.md"]);
    }

    #[test]
    fn pending_comes_first_and_rename_replaces_row() {
        let (t, ex) = sample();
        let p = PendingCreate { parent: "/v/a".into(), kind: PendingKind::File, default_ext: None, placeholder: None };
        let rows = flatten(&t, "/v", &ex, Some("/v/z.md"), Some(&p));
        let labels: Vec<_> = rows.iter().map(label).collect();
        assert_eq!(labels, ["0:a", "1:pending", "1:x.md", "0:b", "1:status", "0:rename:z.md"]);
        assert_eq!(entry_paths(&rows), ["/v/a", "/v/a/x.md", "/v/b"]);
    }

    #[test]
    fn parent_and_join() {
        assert_eq!(parent_dir("/v/a.md").as_deref(), Some("/v"));
        assert_eq!(parent_dir("/a.md").as_deref(), Some("/"));
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(basename("/v/a/b.md"), "b.md");
    }

    #[test]
    fn move_rules() {
        assert!(!can_move("/v/a/x.md", "/v/a"), "own parent");
        assert!(!can_move("/v/a", "/v/a"), "itself");
        assert!(!can_move("/v/a", "/v/a/sub"), "descendant");
        assert!(can_move("/v/a", "/v/ab"), "sibling sharing a prefix");
        assert!(can_move("/v/a/x.md", "/v"));
        assert_eq!(drop_dir_for("/v/a/x.md", false), "/v/a");
        assert_eq!(drop_dir_for("/v/a", true), "/v/a");
    }

    #[test]
    fn create_and_rename_names() {
        assert_eq!(create_name("  ", None), None);
        assert_eq!(create_name(" foo ", None).as_deref(), Some("foo"));
        assert_eq!(create_name("sketch", Some("excalidraw")).as_deref(), Some("sketch.excalidraw"));
        assert_eq!(create_name("s.json", Some("excalidraw")).as_deref(), Some("s.json"));
        assert_eq!(rename_target("/v/a.md", " a.md "), None);
        assert_eq!(rename_target("/v/a.md", ""), None);
        assert_eq!(rename_target("/v/a.md", "b.md").as_deref(), Some("/v/b.md"));
        assert_eq!(rename_target("/v/a.md", "d/b.md").as_deref(), Some("/v/d/b.md"));
    }

    #[test]
    fn import_names() {
        assert_eq!(unique_import_name("pic.png", "20260101-101010"), "pic-20260101-101010.png");
        assert_eq!(unique_import_name("README", "T"), "README-T");
        assert_eq!(unique_import_name(".env", "T"), ".env-T");
        assert_eq!(unique_import_name("a.tar.gz", "T"), "a.tar-T.gz");
    }

    #[test]
    fn imports_with_timestamp_on_collision() {
        let base = std::env::temp_dir().join(format!("fuchico-ex-imp-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.png"), b"new").unwrap();
        std::fs::write(dst.join("a.png"), b"old").unwrap();
        let r = import_files(&[src.join("a.png"), src.join("missing.txt")], dst.to_str().unwrap(), "TS");
        assert_eq!(r.imported, [join(dst.to_str().unwrap(), "a-TS.png")]);
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].starts_with("missing.txt: "));
        assert_eq!(std::fs::read(dst.join("a.png")).unwrap(), b"old");
        assert_eq!(std::fs::read(dst.join("a-TS.png")).unwrap(), b"new");
        std::fs::remove_dir_all(base).ok();
    }

    fn paths(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn selection_model() {
        let entries = paths(&["/a", "/b", "/c", "/d"]);
        let mut s = Selection::default();
        s.select("/b", false, false, &entries);
        assert_eq!(s.paths, ["/b"]);
        s.select("/d", true, false, &entries);
        assert_eq!(s.paths, ["/b", "/c", "/d"]);
        assert_eq!(s.anchor.as_deref(), Some("/b"), "shift keeps the anchor");
        s.select("/a", true, false, &entries);
        assert_eq!(s.paths, ["/a", "/b"]);
        s.select("/d", false, true, &entries);
        assert_eq!(s.paths, ["/a", "/b", "/d"]);
        assert_eq!(s.anchor.as_deref(), Some("/d"));
        s.select("/a", false, true, &entries);
        assert_eq!(s.paths, ["/b", "/d"]);
        s.select("/b", false, true, &entries);
        s.select("/d", false, true, &entries);
        assert_eq!(s.paths, ["/d"], "the last selected row can't be removed");
        assert_eq!(s.targets_for("/d"), ["/d"]);
        assert_eq!(s.targets_for("/a"), ["/a"]);
        s.select_all(&entries);
        assert_eq!(s.paths, entries);
        assert_eq!(s.targets_for("/c"), entries);
        let visible: HashSet<&str> = ["/a", "/b"].into_iter().collect();
        assert!(s.prune(&visible));
        assert_eq!(s.paths, ["/a", "/b"]);
        assert_eq!(s.focus, None, "focus /d disappeared");
        let mut s = Selection::default();
        s.select_all(&entries);
        assert_eq!(s.focus.as_deref(), Some("/a"));
    }

    #[test]
    fn type_ahead() {
        let names = paths(&["alpha", "beta", "apple", "Avocado", "banana"]);
        assert_eq!(type_ahead_target("a", &names, None), Some(0));
        assert_eq!(type_ahead_target("a", &names, Some(0)), Some(2));
        assert_eq!(type_ahead_target("a", &names, Some(3)), Some(0), "wraps");
        assert_eq!(type_ahead_target("aa", &names, Some(2)), Some(3), "repeat cycles");
        assert_eq!(type_ahead_target("ap", &names, Some(2)), Some(2), "mixed starts at current");
        assert_eq!(type_ahead_target("ba", &names, Some(1)), Some(4));
        assert_eq!(type_ahead_target("zz", &names, Some(1)), None);
        assert_eq!(type_ahead_target("", &names, None), None);
    }

    #[test]
    fn reads_real_dir() {
        let d = std::env::temp_dir().join(format!("fuchico-ex-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("n.md"), "x").unwrap();
        std::fs::write(d.join(".hidden"), "x").unwrap();
        let DirState::Loaded(es) = read_dir(d.to_str().unwrap()) else { panic!() };
        let names: Vec<_> = es.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
        assert_eq!(names, [("sub", true), ("n.md", false)]);
        std::fs::remove_dir_all(d).ok();
    }
}
