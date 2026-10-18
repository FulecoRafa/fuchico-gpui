//! Folding: CodeMirror's `foldable()` semantics (fold services first: the
//! `:::fold` region service, then lang-markdown's heading service, then
//! `syntaxFolding` over the block tree), fold state, fold all / unfold all,
//! placeholders, and folding for non-Markdown code files.

use std::ops::Range;

use crate::syntax::{NodeKind, Parsed};
use crate::text::{map_pos, Assoc, LineIndex, TextEdit};
use crate::MarkdownModel;

/// What a folded range shows in place of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldPlaceholder {
    /// The default `…` pill (`cm-foldPlaceholder`).
    Ellipsis,
    /// A folded `:::fold` region: 📁 name + `"N lines"`.
    Region { name: String, line_count: usize },
}

impl FoldPlaceholder {
    /// Text of the `"N lines"` counter (regions only).
    pub fn count_label(&self) -> Option<String> {
        match self {
            FoldPlaceholder::Region { line_count, .. } => Some(format!("{line_count} lines")),
            FoldPlaceholder::Ellipsis => None,
        }
    }
}

/// The set of folded ranges (CodeMirror `foldState`). Host-owned; pass it
/// to [`MarkdownModel::decorations`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldState {
    ranges: Vec<Range<usize>>,
}

impl FoldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// `foldEffect`: adds a range (ignored if already folded).
    pub fn fold(&mut self, r: Range<usize>) {
        if r.start < r.end && !self.ranges.contains(&r) {
            self.ranges.push(r);
            self.ranges.sort_by_key(|r| (r.start, r.end));
        }
    }

    /// `unfoldEffect` for every fold intersecting `from..=to`.
    pub fn unfold_in(&mut self, from: usize, to: usize) {
        self.ranges.retain(|r| r.end < from || r.start > to);
    }

    /// `unfoldAll`.
    pub fn clear(&mut self) {
        self.ranges.clear();
    }

    /// The folded range starting exactly at `pos`, if any.
    pub fn folded_at(&self, pos: usize) -> Option<Range<usize>> {
        self.ranges.iter().find(|r| r.start == pos).cloned()
    }

    /// Outermost fold containing `pos` strictly inside.
    pub fn covering(&self, pos: usize) -> Option<Range<usize>> {
        self.ranges.iter().filter(|r| r.start < pos && pos <= r.end).max_by_key(|r| r.end - r.start).cloned()
    }

    /// Maps the folds through an edit (deleted folds are dropped), like
    /// `folded.map(tr.changes)`.
    pub fn map(&mut self, edits: &[TextEdit]) {
        self.ranges = self
            .ranges
            .iter()
            .filter_map(|r| {
                let a = map_pos(r.start, edits, Assoc::After);
                let b = map_pos(r.end, edits, Assoc::Before);
                (a < b).then_some(a..b)
            })
            .collect();
        self.ranges.sort_by_key(|r| (r.start, r.end));
        self.ranges.dedup();
    }

    /// Clears folds that strictly contain the selection head (CodeMirror
    /// does this on every selection change).
    pub fn reveal(&mut self, head: usize) {
        self.ranges.retain(|r| !(r.start < head && r.end > head));
    }
}

/// Region fold service (`regionFolding.ts`).
pub(crate) fn region_fold(text: &str, idx: &LineIndex, line: usize, start: &str, end: &str) -> Option<Range<usize>> {
    if start.trim().is_empty() || end.trim().is_empty() {
        return None;
    }
    let lr = idx.line_range(line);
    if !text[lr.clone()].trim_start().starts_with(start) {
        return None;
    }
    let mut depth = 0usize;
    for n in line + 1..idx.len() {
        let l = idx.line_range(n);
        let t = text[l.clone()].trim_start();
        if t.starts_with(start) {
            depth += 1;
        } else if t.starts_with(end) {
            if depth == 0 {
                return Some(lr.end..l.end);
            }
            depth -= 1;
        }
    }
    None
}

/// lang-markdown's `headerIndent` fold service.
fn heading_fold(p: &Parsed, idx: &LineIndex, line: usize) -> Option<Range<usize>> {
    let end = idx.line_end(line);
    for &id in &p.line_nodes[line] {
        let n = &p.nodes[id as usize];
        let NodeKind::Heading { level, .. } = n.kind else { continue };
        let parent = &p.nodes[n.parent?];
        let pos = parent.children.iter().position(|c| *c == id as usize)?;
        let mut last = id as usize;
        for &sib in &parent.children[pos + 1..] {
            if let NodeKind::Heading { level: l2, .. } = p.nodes[sib].kind
                && l2 <= level {
                    break;
                }
            last = sib;
        }
        let upto = p.nodes[last].range.end;
        if upto > end {
            return Some(end..upto);
        }
        return None;
    }
    None
}

/// `syntaxFolding`: the outermost foldable node starting on the line whose
/// fold (`lineAt(from).to .. node.to`) extends past the line.
fn syntax_fold(p: &Parsed, idx: &LineIndex, line: usize) -> Option<Range<usize>> {
    let end = idx.line_end(line);
    for &id in &p.line_nodes[line] {
        let n = &p.nodes[id as usize];
        if n.kind.is_foldable() && n.range.end > end {
            return Some(end..n.range.end);
        }
    }
    None
}

impl MarkdownModel {
    /// `foldable(state, line.from, line.to)` for 0-based `line`.
    pub fn foldable(&self, line: usize) -> Option<Range<usize>> {
        if line >= self.lines.len() {
            return None;
        }
        let s = &self.settings;
        region_fold(&self.text, &self.lines, line, &s.fold_start_marker, &s.fold_end_marker)
            .or_else(|| heading_fold(&self.parsed, &self.lines, line))
            .or_else(|| syntax_fold(&self.parsed, &self.lines, line))
    }

    /// `foldAll`: every foldable range, scanning top-down and skipping the
    /// inside of each fold.
    pub fn fold_all(&self) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut line = 0;
        while line < self.lines.len() {
            match self.foldable(line) {
                Some(r) => {
                    line = self.lines.line_of(r.end) + 1;
                    out.push(r);
                }
                None => line += 1,
            }
        }
        out
    }

    /// Toggles the fold of `line` (fold gutter click / `za`): unfolds a fold
    /// starting on the line, or folds it if foldable. Returns whether
    /// anything changed.
    pub fn toggle_fold(&self, folds: &mut FoldState, line: usize) -> bool {
        let lr = self.lines.line_range(line);
        if let Some(r) = folds.ranges.iter().find(|r| r.start >= lr.start && r.start <= lr.end).cloned() {
            folds.ranges.retain(|x| *x != r);
            return true;
        }
        match self.foldable(line) {
            Some(r) => {
                folds.fold(r);
                true
            }
            None => false,
        }
    }

    /// The placeholder shown for folded `range` (`regionFoldWidget.ts`).
    pub fn fold_placeholder(&self, range: &Range<usize>) -> FoldPlaceholder {
        let marker = &self.settings.fold_start_marker;
        if marker.trim().is_empty() || range.start == 0 {
            return FoldPlaceholder::Ellipsis;
        }
        let start_line = self.lines.line_of(range.start - 1);
        let t = self.text[self.lines.line_range(start_line)].trim_start();
        if !t.starts_with(marker.as_str()) {
            return FoldPlaceholder::Ellipsis;
        }
        let name = t[marker.len()..].trim();
        let end_line = self.lines.line_of(range.end);
        FoldPlaceholder::Region {
            name: if name.is_empty() { "Region".into() } else { name.into() },
            line_count: end_line.saturating_sub(start_line + 1),
        }
    }
}

/// How a non-Markdown file folds (our own design: CodeMirror's per-language
/// fold props are approximated by bracket matching or indentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeFoldStyle {
    /// `{}` / `[]` / `()` blocks opened on the line (JS/TS/Rust/JSON/CSS).
    Brackets,
    /// Following lines indented deeper than this one (Python/HTML).
    Indent,
}

impl CodeFoldStyle {
    pub fn for_language(name: &str) -> Self {
        match name {
            "Python" | "HTML" => CodeFoldStyle::Indent,
            _ => CodeFoldStyle::Brackets,
        }
    }
}

fn indent_width(s: &str, tab: usize) -> Option<usize> {
    if s.trim().is_empty() {
        return None;
    }
    let mut w = 0;
    for c in s.chars() {
        match c {
            ' ' => w += 1,
            '\t' => w += tab - w % tab,
            _ => break,
        }
    }
    Some(w)
}

/// Fold range for 0-based `line` of a code file.
pub fn code_foldable(text: &str, idx: &LineIndex, line: usize, style: CodeFoldStyle, tab_size: usize) -> Option<Range<usize>> {
    let lr = idx.line_range(line);
    match style {
        CodeFoldStyle::Indent => {
            let base = indent_width(&text[lr.clone()], tab_size.max(1))?;
            let mut last = None;
            for n in line + 1..idx.len() {
                let l = idx.line_range(n);
                match indent_width(&text[l.clone()], tab_size.max(1)) {
                    None => continue,
                    Some(w) if w > base => last = Some(l.end),
                    Some(_) => break,
                }
            }
            last.filter(|e| *e > lr.end).map(|e| lr.end..e)
        }
        CodeFoldStyle::Brackets => {
            // Last unmatched opening bracket on the line, skipping strings
            // and `//` comments.
            let bytes = text.as_bytes();
            let mut stack: Vec<(u8, usize)> = Vec::new();
            let mut i = lr.start;
            let mut quote: Option<u8> = None;
            while i < lr.end {
                let c = bytes[i];
                if let Some(q) = quote {
                    if c == b'\\' {
                        i += 1;
                    } else if c == q {
                        quote = None;
                    }
                } else {
                    match c {
                        b'"' | b'\'' | b'`' => quote = Some(c),
                        b'/' if bytes.get(i + 1) == Some(&b'/') => break,
                        b'{' | b'[' | b'(' => stack.push((c, i)),
                        b'}' | b']' | b')' => {
                            stack.pop();
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            let (open, at) = *stack.last()?;
            let close = match open {
                b'{' => b'}',
                b'[' => b']',
                _ => b')',
            };
            let mut depth = 0usize;
            let mut j = at + 1;
            let mut quote: Option<u8> = None;
            while j < bytes.len() {
                let c = bytes[j];
                if let Some(q) = quote {
                    if c == b'\\' {
                        j += 1;
                    } else if c == q || (c == b'\n' && q != b'`') {
                        quote = None;
                    }
                } else if c == b'/' && bytes.get(j + 1) == Some(&b'/') {
                    while j < bytes.len() && bytes[j] != b'\n' {
                        j += 1;
                    }
                    continue;
                } else if matches!(c, b'"' | b'\'' | b'`') {
                    quote = Some(c);
                } else if c == open {
                    depth += 1;
                } else if c == close {
                    if depth == 0 {
                        // Fold from the end of the line to the closing bracket.
                        return (j > lr.end && idx.line_of(j) > line).then_some(lr.end..j);
                    }
                    depth -= 1;
                }
                j += 1;
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_and_indent() {
        let t = "fn a() {\n  x;\n}\n";
        let idx = LineIndex::new(t);
        assert_eq!(code_foldable(t, &idx, 0, CodeFoldStyle::Brackets, 4), Some(8..14));
        assert_eq!(code_foldable(t, &idx, 1, CodeFoldStyle::Brackets, 4), None);
        let p = "def f():\n    a\n\n    b\nc\n";
        let idx = LineIndex::new(p);
        assert_eq!(code_foldable(p, &idx, 0, CodeFoldStyle::Indent, 4), Some(8..21));
    }

    #[test]
    fn fold_state_mapping() {
        let mut f = FoldState::new();
        f.fold(5..10);
        f.map(&[TextEdit::insert(0, "ab")]);
        assert_eq!(f.ranges(), vec![7..12]);
        f.map(&[TextEdit::new(6..13, "")]);
        assert!(f.is_empty());
        f.fold(1..9);
        f.reveal(4);
        assert!(f.is_empty());
    }
}
