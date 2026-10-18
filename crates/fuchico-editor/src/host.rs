//! The [`fuchico_helix::Host`] the view gives its engine. The engine owns
//! the host as a `Box<dyn Host>`, so the layout facts it needs (wrapped
//! rows, hidden lines, viewport) live in a [`SharedLayout`] shared with the
//! view through `Rc<RefCell<..>>`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fuchico_helix::{Host, Text, VisualLines, logical_move_vertically};
use fuchico_markdown::{SyntaxTree, is_markdown_path};

/// Layout facts published by the view after every frame.
#[derive(Debug, Default)]
pub struct SharedLayout {
    /// Soft-wrapped lines (1-based line number → char offsets where each
    /// visual row starts, first = line start). Lines missing here are one
    /// row.
    pub rows: HashMap<usize, Vec<usize>>,
    /// Hidden (folded / covered by a block widget) 1-based lines.
    pub hidden: Vec<bool>,
    /// First and last visible 1-based lines.
    pub visible: Option<(usize, usize)>,
    pub half_page: usize,
    /// System clipboard snapshot (filled by the view before keys that may
    /// read it) and the last write (drained by the view after each key).
    pub clipboard: Option<String>,
    pub clipboard_written: Option<String>,
    pub path: Option<String>,
}

impl SharedLayout {
    fn is_hidden(&self, line: usize) -> bool {
        self.hidden
            .get(line.wrapping_sub(1))
            .copied()
            .unwrap_or(false)
    }

    fn rows_of(&self, doc: &Text, line: usize) -> Vec<usize> {
        self.rows
            .get(&line)
            .cloned()
            .unwrap_or_else(|| vec![doc.line(line).from])
    }

    /// Visual row containing `pos`: (line, row index, row start, row end).
    fn row_at(&self, doc: &Text, pos: usize) -> (usize, usize, usize, usize) {
        let line = doc.line_at(pos);
        let rows = self.rows_of(doc, line.number);
        let ix = rows.iter().rposition(|&s| s <= pos).unwrap_or(0);
        let end = rows.get(ix + 1).map_or(line.to, |&n| n.saturating_sub(0));
        (line.number, ix, rows[ix], end)
    }
}

pub(crate) struct ViewHost {
    pub shared: Rc<RefCell<SharedLayout>>,
    /// Markdown syntax tree of the last queried document.
    pub tree: Option<TreeCache>,
}

/// A parsed document with its char -> byte offset table.
pub(crate) struct TreeCache {
    text: String,
    tree: SyntaxTree,
    /// `bytes[c]` = byte offset of char `c` (plus one past the end).
    bytes: Vec<usize>,
}

impl TreeCache {
    fn byte(&self, c: usize) -> usize {
        self.bytes.get(c).copied().unwrap_or(self.text.len())
    }
    fn char(&self, b: usize) -> usize {
        self.bytes.partition_point(|&x| x < b)
    }
    fn chars(&self, r: std::ops::Range<usize>) -> (usize, usize) {
        (self.char(r.start), self.char(r.end))
    }
}

impl ViewHost {
    pub fn new(shared: Rc<RefCell<SharedLayout>>) -> Self {
        Self { shared, tree: None }
    }

    /// The Markdown tree of `doc`; `None` for non-Markdown files (the
    /// original only has a Markdown language there or no tree at all).
    fn tree(&mut self, doc: &Text) -> Option<&TreeCache> {
        if !is_markdown_path(self.shared.borrow().path.as_deref()) {
            return None;
        }
        let text = doc.to_string();
        if self.tree.as_ref().is_none_or(|t| t.text != text) {
            let mut bytes: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
            bytes.push(text.len());
            let tree = SyntaxTree::parse(&text);
            self.tree = Some(TreeCache { text, tree, bytes });
        }
        self.tree.as_ref()
    }
}

impl VisualLines for ViewHost {
    fn move_vertically(
        &mut self,
        doc: &Text,
        pos: usize,
        goal: Option<usize>,
        forward: bool,
        lines: usize,
        tab_size: usize,
    ) -> (usize, Option<usize>) {
        let s = self.shared.borrow();
        if s.rows.is_empty() && !s.hidden.iter().any(|h| *h) {
            return logical_move_vertically(doc, pos, goal, forward, lines, tab_size);
        }
        if pos == if forward { doc.len() } else { 0 } {
            return (pos, goal);
        }
        // Goal = column inside the visual row (in chars).
        let (mut line, mut ix, start, _) = s.row_at(doc, pos);
        let goal = goal.unwrap_or(pos - start);
        let n = doc.lines();
        for _ in 0..lines.max(1) {
            let rows = s.rows_of(doc, line).len();
            if forward {
                if ix + 1 < rows {
                    ix += 1;
                    continue;
                }
                let mut next = line + 1;
                while next <= n && s.is_hidden(next) {
                    next += 1;
                }
                if next > n {
                    return (doc.len(), Some(goal));
                }
                line = next;
                ix = 0;
            } else {
                if ix > 0 {
                    ix -= 1;
                    continue;
                }
                let mut prev = line.saturating_sub(1);
                while prev >= 1 && s.is_hidden(prev) {
                    prev -= 1;
                }
                if prev == 0 {
                    return (0, Some(goal));
                }
                line = prev;
                ix = s.rows_of(doc, line).len() - 1;
            }
        }
        let rows = s.rows_of(doc, line);
        let l = doc.line(line);
        let rs = rows[ix];
        let re = rows
            .get(ix + 1)
            .map_or(l.to, |&e| e.saturating_sub(1).max(rs));
        (rs + goal.min(re - rs), Some(goal))
    }

    fn lines_to_boundary(&mut self, doc: &Text, pos: usize, forward: bool) -> usize {
        let line = doc.line_at(pos).number;
        if forward {
            doc.lines() - line
        } else {
            line - 1
        }
    }
}

impl Host for ViewHost {
    fn clipboard_read(&mut self) -> Result<String, String> {
        self.shared
            .borrow()
            .clipboard
            .clone()
            .ok_or_else(|| "clipboard empty".to_string())
    }
    fn clipboard_write(&mut self, text: &str) {
        let mut s = self.shared.borrow_mut();
        s.clipboard = Some(text.to_string());
        s.clipboard_written = Some(text.to_string());
    }
    fn half_page_lines(&mut self) -> usize {
        self.shared.borrow().half_page.max(1)
    }
    fn visible_lines(&mut self) -> Option<(usize, usize)> {
        self.shared.borrow().visible
    }
    fn path(&mut self) -> Option<String> {
        self.shared.borrow().path.clone()
    }
    fn syntax_parent(&mut self, doc: &Text, from: usize, to: usize) -> Option<(usize, usize)> {
        let t = self.tree(doc)?;
        t.tree.select_parent(t.byte(from), t.byte(to)).map(|r| t.chars(r))
    }
    fn syntax_sibling(&mut self, doc: &Text, from: usize, to: usize, forward: bool) -> Option<(usize, usize)> {
        let t = self.tree(doc)?;
        t.tree.sibling(t.byte(from), t.byte(to), forward).map(|r| t.chars(r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_by_visual_rows_and_skips_hidden() {
        let doc = Text::new("abcdefghij\nxy\nhidden\nlast");
        let shared = Rc::new(RefCell::new(SharedLayout::default()));
        // line 1 wraps after 5 chars; line 3 is folded away
        shared.borrow_mut().rows.insert(1, vec![0, 5]);
        shared.borrow_mut().hidden = vec![false, false, true, false];
        let mut h = ViewHost::new(shared);
        let (p, g) = h.move_vertically(&doc, 2, None, true, 1, 4);
        assert_eq!((p, g), (7, Some(2)));
        let (p, _) = h.move_vertically(&doc, p, g, true, 1, 4);
        assert_eq!(p, 13); // "xy" end (clamped)
        let (p, _) = h.move_vertically(&doc, 11, Some(0), true, 1, 4);
        assert_eq!(p, doc.line(4).from);
        let (p, _) = h.move_vertically(&doc, 7, Some(2), false, 1, 4);
        assert_eq!(p, 2);
    }
}
