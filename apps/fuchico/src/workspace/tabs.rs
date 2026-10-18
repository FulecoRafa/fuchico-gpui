//! Pure tab-list logic (spec 01-shell "Tabs"). No GPUI types so it is unit
//! testable; [`super::model::AppModel`] wraps it.

/// One open tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub path: String,
    pub dirty: bool,
    /// 1-based line to reveal when the pane (re)focuses the tab.
    pub focus_line: Option<u32>,
    /// Bumped on every re-open so panes re-apply `focus_line` even when the
    /// line is unchanged.
    pub focus_token: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tabs {
    pub tabs: Vec<Tab>,
    pub active: Option<String>,
}

/// Maps `path` through a rename of `from` → `to` (the path itself, or a
/// descendant `from/...`). `None` when unaffected.
pub fn renamed(path: &str, from: &str, to: &str) -> Option<String> {
    if path == from {
        return Some(to.to_string());
    }
    let prefix = format!("{from}/");
    path.strip_prefix(&prefix).map(|rest| format!("{to}/{rest}"))
}

/// Whether `path` is `target` or lives under it.
pub fn is_same_or_under(path: &str, target: &str) -> bool {
    path == target || path.starts_with(&format!("{target}/"))
}

/// Basename split on `/` or `\`, ignoring empty parts.
pub fn basename(path: &str) -> &str {
    path.split(['/', '\\']).rfind(|s| !s.is_empty()).unwrap_or(path)
}

impl Tabs {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let a = self.active.as_deref()?;
        self.tabs.iter().find(|t| t.path == a)
    }

    pub fn index_of(&self, path: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.path == path)
    }

    /// Opens (or refocuses) `path` and activates it.
    pub fn open(&mut self, path: &str, focus_line: Option<u32>) {
        match self.tabs.iter_mut().find(|t| t.path == path) {
            Some(t) => {
                t.focus_line = focus_line;
                t.focus_token = t.focus_token.wrapping_add(1);
            }
            None => self.tabs.push(Tab {
                path: path.to_string(),
                dirty: false,
                focus_line,
                focus_token: 1,
            }),
        }
        self.active = Some(path.to_string());
    }

    pub fn activate(&mut self, path: &str) {
        if self.index_of(path).is_some() {
            self.active = Some(path.to_string());
        }
    }

    /// Closes `path`; when it was active, activates `next[min(idx, len-1)]`.
    pub fn close(&mut self, path: &str) {
        let Some(idx) = self.index_of(path) else { return };
        self.tabs.remove(idx);
        if self.active.as_deref() == Some(path) {
            self.active = self
                .tabs
                .get(idx.min(self.tabs.len().saturating_sub(1)))
                .map(|t| t.path.clone());
        }
    }

    pub fn close_others(&mut self, keep: &str) {
        self.tabs.retain(|t| t.path == keep);
        self.active = self.tabs.first().map(|t| t.path.clone());
    }

    pub fn close_all(&mut self) {
        self.tabs.clear();
        self.active = None;
    }

    /// Returns whether anything changed.
    pub fn set_dirty(&mut self, path: &str, dirty: bool) -> bool {
        match self.tabs.iter_mut().find(|t| t.path == path) {
            Some(t) if t.dirty != dirty => {
                t.dirty = dirty;
                true
            }
            _ => false,
        }
    }

    /// Remaps tabs (and the active path) after `from` was renamed to `to`.
    pub fn rename(&mut self, from: &str, to: &str) {
        for t in &mut self.tabs {
            if let Some(n) = renamed(&t.path, from, to) {
                t.path = n;
            }
        }
        if let Some(n) = self.active.as_deref().and_then(|a| renamed(a, from, to)) {
            self.active = Some(n);
        }
    }

    /// Closes every tab at or under `deleted`; if the active one went away,
    /// activates `next[min(firstRemovedIdx, len-1)]`.
    pub fn delete(&mut self, deleted: &str) {
        let first = self.tabs.iter().position(|t| is_same_or_under(&t.path, deleted));
        let Some(first) = first else { return };
        let active_gone = self.active.as_deref().is_some_and(|a| is_same_or_under(a, deleted));
        self.tabs.retain(|t| !is_same_or_under(&t.path, deleted));
        if active_gone {
            self.active = self
                .tabs
                .get(first.min(self.tabs.len().saturating_sub(1)))
                .map(|t| t.path.clone());
        }
    }

    /// Cycles the active tab by `delta` (wrapping).
    pub fn cycle(&mut self, delta: isize) {
        if self.tabs.is_empty() {
            return;
        }
        let n = self.tabs.len() as isize;
        let cur = self.active.as_deref().and_then(|a| self.index_of(a)).unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(n) as usize;
        self.active = Some(self.tabs[next].path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(t: &Tabs) -> Vec<&str> {
        t.tabs.iter().map(|t| t.path.as_str()).collect()
    }

    #[test]
    fn open_appends_then_refocuses() {
        let mut t = Tabs::default();
        t.open("/v/a.md", None);
        t.open("/v/b.md", Some(3));
        t.open("/v/a.md", Some(7));
        assert_eq!(paths(&t), ["/v/a.md", "/v/b.md"]);
        assert_eq!(t.active.as_deref(), Some("/v/a.md"));
        let a = &t.tabs[0];
        assert_eq!((a.focus_line, a.focus_token), (Some(7), 2));
        assert_eq!(t.tabs[1].focus_token, 1);
    }

    #[test]
    fn close_activates_neighbour() {
        let mut t = Tabs::default();
        for p in ["a", "b", "c"] {
            t.open(p, None);
        }
        t.activate("b");
        t.close("b");
        assert_eq!(t.active.as_deref(), Some("c"));
        t.close("c");
        assert_eq!(t.active.as_deref(), Some("a"));
        t.close("a");
        assert_eq!(t.active, None);
    }

    #[test]
    fn closing_inactive_keeps_active() {
        let mut t = Tabs::default();
        t.open("a", None);
        t.open("b", None);
        t.close("a");
        assert_eq!(t.active.as_deref(), Some("b"));
    }

    #[test]
    fn close_others_and_all() {
        let mut t = Tabs::default();
        for p in ["a", "b", "c"] {
            t.open(p, None);
        }
        t.close_others("b");
        assert_eq!(paths(&t), ["b"]);
        assert_eq!(t.active.as_deref(), Some("b"));
        t.close_all();
        assert!(t.is_empty() && t.active.is_none());
    }

    #[test]
    fn renamed_matches_prefix_with_slash_only() {
        assert_eq!(renamed("/v/a", "/v/a", "/v/b").as_deref(), Some("/v/b"));
        assert_eq!(renamed("/v/a/x.md", "/v/a", "/v/b").as_deref(), Some("/v/b/x.md"));
        assert_eq!(renamed("/v/ab.md", "/v/a", "/v/b"), None);
    }

    #[test]
    fn rename_propagates_to_tabs_and_active() {
        let mut t = Tabs::default();
        t.open("/v/d/x.md", None);
        t.open("/v/other.md", None);
        t.activate("/v/d/x.md");
        t.rename("/v/d", "/v/e");
        assert_eq!(paths(&t), ["/v/e/x.md", "/v/other.md"]);
        assert_eq!(t.active.as_deref(), Some("/v/e/x.md"));
    }

    #[test]
    fn delete_closes_subtree() {
        let mut t = Tabs::default();
        for p in ["/v/a.md", "/v/d/x.md", "/v/d/y.md", "/v/z.md"] {
            t.open(p, None);
        }
        t.activate("/v/d/y.md");
        t.delete("/v/d");
        assert_eq!(paths(&t), ["/v/a.md", "/v/z.md"]);
        assert_eq!(t.active.as_deref(), Some("/v/z.md"));
    }

    #[test]
    fn dirty_noop_when_unchanged() {
        let mut t = Tabs::default();
        t.open("a", None);
        assert!(!t.set_dirty("a", false));
        assert!(t.set_dirty("a", true));
        assert!(!t.set_dirty("a", true));
        assert!(!t.set_dirty("missing", true));
    }

    #[test]
    fn cycle_wraps() {
        let mut t = Tabs::default();
        for p in ["a", "b", "c"] {
            t.open(p, None);
        }
        t.cycle(1);
        assert_eq!(t.active.as_deref(), Some("a"));
        t.cycle(-1);
        assert_eq!(t.active.as_deref(), Some("c"));
    }

    #[test]
    fn basename_ignores_empty_parts() {
        assert_eq!(basename("/v/a.md"), "a.md");
        assert_eq!(basename("C:\\v\\b.md"), "b.md");
        assert_eq!(basename("/v/dir/"), "dir");
    }
}
