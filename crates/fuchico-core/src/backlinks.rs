//! Port of `backlinksFor` from `src/modules/backlinks/lib/useBacklinks.ts`.

use crate::links::{LinkKind, LinkRef};
use crate::wikilinks::{resolve_relative_markdown_link, resolve_wikilink_target};

#[derive(Debug, Clone, PartialEq)]
pub struct Backlink {
    pub line: usize,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BacklinkGroup {
    pub path: String,
    pub refs: Vec<Backlink>,
}

/// Groups every link that resolves to `target_path` by the note it lives in.
/// Groups are sorted by path (JS `localeCompare` approximated by a
/// case-insensitive compare with a byte-order tiebreak).
pub fn backlinks_for<S: AsRef<str>>(refs: &[LinkRef], target_path: &str, vault_files: &[S]) -> Vec<BacklinkGroup> {
    let mut groups: Vec<BacklinkGroup> = Vec::new();
    for r in refs {
        if r.path == target_path {
            continue;
        }
        let resolved = match r.kind {
            LinkKind::Wiki => resolve_wikilink_target(&r.target, vault_files),
            LinkKind::Markdown => resolve_relative_markdown_link(&r.target, &r.path),
        };
        if resolved.as_deref() != Some(target_path) {
            continue;
        }
        let idx = match groups.iter().position(|g| g.path == r.path) {
            Some(i) => i,
            None => {
                groups.push(BacklinkGroup { path: r.path.clone(), refs: vec![] });
                groups.len() - 1
            }
        };
        let list = &mut groups[idx].refs;
        if !list.iter().any(|b| b.line == r.line) {
            list.push(Backlink { line: r.line, context: r.context.clone() });
        }
    }
    groups.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()).then_with(|| a.path.cmp(&b.path)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lr(path: &str, line: usize, target: &str, kind: LinkKind, ctx: &str) -> LinkRef {
        LinkRef { path: path.into(), line, target: target.into(), kind, context: ctx.into() }
    }

    #[test]
    fn resolves_wikilinks_and_relative_markdown_links_grouped_by_source() {
        let files = ["/v/a.md", "/v/sub/b.md", "/v/c.md"];
        let refs = vec![
            lr("/v/a.md", 3, "b|alias", LinkKind::Wiki, "x"),
            lr("/v/a.md", 3, "b#h", LinkKind::Wiki, "x"),
            lr("/v/c.md", 1, "./sub/b.md", LinkKind::Markdown, "y"),
            lr("/v/c.md", 2, "a", LinkKind::Wiki, "z"),
            lr("/v/sub/b.md", 9, "b", LinkKind::Wiki, "self"),
        ];
        let groups = backlinks_for(&refs, "/v/sub/b.md", &files);
        assert_eq!(
            groups,
            vec![
                BacklinkGroup { path: "/v/a.md".into(), refs: vec![Backlink { line: 3, context: "x".into() }] },
                BacklinkGroup { path: "/v/c.md".into(), refs: vec![Backlink { line: 1, context: "y".into() }] },
            ]
        );
    }
}
