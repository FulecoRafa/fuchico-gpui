//! Explorer data: per-directory load state and the flattened row list.
//! Pure (no GPUI) so it is unit tested.

use std::collections::{HashMap, HashSet};

use fuchico_core::fs::tree::{EntryKind, fs_read_dir};

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Entry { entry: Entry, depth: usize, expanded: bool },
    /// "Loading…" / error line under an expanded folder.
    Status { depth: usize, text: String, error: bool },
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

pub fn parent_dir(path: &str) -> Option<String> {
    let i = path.rfind('/')?;
    Some(if i == 0 { "/".to_string() } else { path[..i].to_string() })
}

/// Depth-first walk over loaded, expanded folders.
pub fn flatten(tree: &Tree, root: &str, expanded: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    walk(tree, root, 0, expanded, &mut out);
    out
}

fn walk(tree: &Tree, dir: &str, depth: usize, expanded: &HashSet<String>, out: &mut Vec<Row>) {
    let Some(DirState::Loaded(entries)) = tree.get(dir) else { return };
    for e in entries {
        let open = e.is_dir && expanded.contains(&e.path);
        out.push(Row::Entry { entry: e.clone(), depth, expanded: open });
        if !open {
            continue;
        }
        match tree.get(&e.path) {
            Some(DirState::Loaded(_)) => walk(tree, &e.path, depth + 1, expanded, out),
            Some(DirState::Error(msg)) => out.push(Row::Status { depth, text: msg.clone(), error: true }),
            _ => out.push(Row::Status { depth, text: crate::t!(CommonLoading), error: false }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(dir: &str, name: &str, is_dir: bool) -> Entry {
        Entry { name: name.into(), path: join(dir, name), is_dir }
    }

    #[test]
    fn flattens_expanded_folders_only() {
        let mut t = Tree::default();
        t.set("/v".into(), DirState::Loaded(vec![e("/v", "a", true), e("/v", "b", true), e("/v", "z.md", false)]));
        t.set("/v/a".into(), DirState::Loaded(vec![e("/v/a", "x.md", false)]));
        let mut ex = HashSet::new();
        ex.insert("/v/a".to_string());
        ex.insert("/v/b".to_string());
        let rows = flatten(&t, "/v", &ex);
        let names: Vec<String> = rows
            .iter()
            .map(|r| match r {
                Row::Entry { entry, depth, .. } => format!("{depth}:{}", entry.name),
                Row::Status { depth, .. } => format!("{depth}:status"),
            })
            .collect();
        assert_eq!(names, ["0:a", "1:x.md", "0:b", "0:status", "0:z.md"]);
    }

    #[test]
    fn parent_and_join() {
        assert_eq!(parent_dir("/v/a.md").as_deref(), Some("/v"));
        assert_eq!(parent_dir("/a.md").as_deref(), Some("/"));
        assert_eq!(join("/", "a"), "/a");
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
