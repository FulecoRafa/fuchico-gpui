//! Quick Switcher data (spec 4 §5): the vault walk (`walkVault`) and the
//! ranking over vault-relative paths. Pure / blocking; the view runs the walk
//! on the background executor.

use fuchico_core::fs::tree::{EntryKind, fs_read_dir};
use fuchico_core::fuzzy::fuzzy_match;

pub const MAX_RESULTS: usize = 200;

/// `walkVault(root)`: every non-hidden file (all types), recursing into
/// directories and skipping symlinks. A directory's files come in
/// `fs_read_dir` order, its subdirectories' contents after them.
/// Unreadable directories are skipped; an unreadable root is an error.
pub fn walk_vault(root: &str) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    let entries = fs_read_dir(root.to_string(), false)?;
    walk_entries(root, entries, &mut files);
    Ok(files)
}

fn walk_entries(
    dir: &str,
    entries: Vec<fuchico_core::fs::tree::DirEntry>,
    files: &mut Vec<String>,
) {
    let mut subdirs = Vec::new();
    for e in entries {
        let full = join(dir, &e.name);
        match e.kind {
            EntryKind::Dir => subdirs.push(full),
            EntryKind::File => files.push(full),
            EntryKind::Symlink => {}
        }
    }
    for d in subdirs {
        if let Ok(entries) = fs_read_dir(d.clone(), false) {
            walk_entries(&d, entries, files);
        }
    }
}

/// `joinPath(dir, name)`.
fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// `relativeTo(root, path)`.
pub fn relative_to(root: &str, path: &str) -> String {
    if path == root {
        return path.rsplit('/').next().unwrap_or(path).to_string();
    }
    let prefix = if root.ends_with('/') {
        root.to_string()
    } else {
        format!("{root}/")
    };
    path.strip_prefix(&prefix).unwrap_or(path).to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileRow {
    pub path: String,
    pub rel: String,
    pub indices: Vec<usize>,
    pub score: f64,
}

/// The switcher's results: walk order when the query is blank, otherwise
/// fuzzy over the relative path sorted by score; capped at [`MAX_RESULTS`].
pub fn rank_files(root: &str, files: &[String], query: &str) -> Vec<FileRow> {
    let q = query.trim();
    let entries = files.iter().map(|p| (p, relative_to(root, p)));
    if q.is_empty() {
        return entries
            .take(MAX_RESULTS)
            .map(|(p, rel)| FileRow {
                path: p.clone(),
                rel,
                indices: vec![],
                score: 0.,
            })
            .collect();
    }
    let mut ranked: Vec<FileRow> = entries
        .filter_map(|(p, rel)| {
            let m = fuzzy_match(q, &rel);
            m.matched.then(|| FileRow {
                path: p.clone(),
                rel,
                indices: m.indices,
                score: m.score,
            })
        })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    ranked.truncate(MAX_RESULTS);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative_to("/v", "/v/a/b.md"), "a/b.md");
        assert_eq!(relative_to("/v/", "/v/a.md"), "a.md");
        assert_eq!(relative_to("/v", "/v"), "v");
        assert_eq!(relative_to("/v", "/vault/x.md"), "/vault/x.md");
    }

    #[test]
    fn ranking() {
        let files: Vec<String> = ["/v/notes/zeta.md", "/v/alpha.md", "/v/project-launch.md"]
            .map(String::from)
            .into();
        let r = rank_files("/v", &files, "");
        assert_eq!(
            r.iter().map(|r| r.rel.as_str()).collect::<Vec<_>>(),
            ["notes/zeta.md", "alpha.md", "project-launch.md"]
        );
        let r = rank_files("/v", &files, "pl");
        assert_eq!(r[0].rel, "project-launch.md");
        assert_eq!(r[0].indices, vec![0, 8]);
        // Folder names match too.
        assert_eq!(rank_files("/v", &files, "notes")[0].rel, "notes/zeta.md");
        let many: Vec<String> = (0..300).map(|i| format!("/v/{i}.md")).collect();
        assert_eq!(rank_files("/v", &many, "").len(), MAX_RESULTS);
        assert_eq!(rank_files("/v", &many, "md").len(), MAX_RESULTS);
    }

    #[test]
    fn walk_lists_all_types_files_first_and_skips_hidden() {
        let dir = std::env::temp_dir().join(format!("fuchico-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        for f in [
            "b.md",
            "a.excalidraw",
            "sub/c.png",
            "sub/deeper/d.md",
            ".hidden/e.md",
            ".dot.md",
        ] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let root = dir.to_string_lossy().to_string();
        let rel: Vec<String> = walk_vault(&root)
            .unwrap()
            .iter()
            .map(|p| relative_to(&root, p))
            .collect();
        assert_eq!(
            rel,
            ["a.excalidraw", "b.md", "sub/c.png", "sub/deeper/d.md"]
        );
        assert!(walk_vault(&format!("{root}/missing")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
