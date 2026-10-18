//! Text helpers: line index, edits and position mapping.
//!
//! All offsets in this crate are **byte offsets** into the UTF-8 document,
//! like `fuchico-core`. Lines are **0-based** (the host converts to 1-based
//! for display).

use std::ops::Range;

/// One replacement in the *old* document: `range` is replaced by `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub insert: String,
}

impl TextEdit {
    pub fn new(range: Range<usize>, insert: impl Into<String>) -> Self {
        Self {
            range,
            insert: insert.into(),
        }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self {
            range: at..at,
            insert: text.into(),
        }
    }
}

/// Applies non-overlapping edits (offsets in the old document, any order).
pub fn apply_edits(doc: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.range.start);
    let mut out = String::with_capacity(doc.len());
    let mut last = 0;
    for e in sorted {
        out.push_str(&doc[last..e.range.start]);
        out.push_str(&e.insert);
        last = e.range.end;
    }
    out.push_str(&doc[last..]);
    out
}

/// Which side a position sticks to when text is inserted exactly at it
/// (CodeMirror's `assoc`: `-1` = stays before the insertion, `1` = moves
/// after it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Before,
    After,
}

/// Maps a position in the old document through `edits` (old-document
/// offsets, non-overlapping). A position inside a replaced range maps to the
/// start (`Before`) or end (`After`) of the replacement.
pub fn map_pos(pos: usize, edits: &[TextEdit], assoc: Assoc) -> usize {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.range.start);
    let mut delta: isize = 0;
    for e in sorted {
        let (from, to, ins) = (e.range.start, e.range.end, e.insert.len() as isize);
        let shift = |d: isize| (pos as isize + d).max(0) as usize;
        if to < pos {
            delta += ins - (to - from) as isize;
            continue;
        }
        if from > pos {
            break;
        }
        let start = (from as isize + delta).max(0) as usize;
        if from == to {
            // Pure insertion exactly at `pos`.
            match assoc {
                Assoc::Before => return shift(delta),
                Assoc::After => {
                    delta += ins;
                    continue;
                }
            }
        }
        // `pos` is within `[from, to]` of a non-empty replaced range.
        if pos == to || assoc == Assoc::After {
            return start + ins as usize;
        }
        return start;
    }
    (pos as isize + delta).max(0) as usize
}

/// Line start offsets of a document. `\n` separates lines; a trailing `\r`
/// is kept in the line's text range but excluded from [`LineIndex::line_range`]
/// content (CodeMirror treats CRLF documents the same way after normalisation;
/// Fuchico normalises to LF on load anyway).
#[derive(Debug, Clone, Default)]
pub struct LineIndex {
    starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 32 + 1);
        starts.push(0);
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self {
            starts,
            len: text.len(),
        }
    }

    /// Number of lines (an empty document has one empty line).
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn doc_len(&self) -> usize {
        self.len
    }

    /// 0-based line containing byte offset `pos` (clamped).
    pub fn line_of(&self, pos: usize) -> usize {
        match self.starts.binary_search(&pos.min(self.len)) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.starts[line.min(self.starts.len() - 1)]
    }

    /// End of the line's text, excluding the `\n`.
    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 < self.starts.len() {
            self.starts[line + 1] - 1
        } else {
            self.len
        }
    }

    /// Byte range of the line's text, excluding the `\n`.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        self.line_start(line)..self.line_end(line)
    }
}

/// Builds a [`ropey::Rope`]-free `String` from a rope (convenience for hosts
/// that keep their buffer in a rope).
pub fn rope_to_string(rope: &ropey::Rope) -> String {
    let mut s = String::with_capacity(rope.len_bytes());
    for chunk in rope.chunks() {
        s.push_str(chunk);
    }
    s
}

/// Index of the first line touched by `edits` (old-document lines).
pub fn first_edited_line(index: &LineIndex, edits: &[TextEdit]) -> Option<usize> {
    edits.iter().map(|e| index.line_of(e.range.start)).min()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_basics() {
        let idx = LineIndex::new("ab\ncd\n");
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.line_range(0), 0..2);
        assert_eq!(idx.line_range(1), 3..5);
        assert_eq!(idx.line_range(2), 6..6);
        assert_eq!(idx.line_of(0), 0);
        assert_eq!(idx.line_of(2), 0);
        assert_eq!(idx.line_of(3), 1);
        assert_eq!(idx.line_of(6), 2);
        assert_eq!(idx.line_of(99), 2);
    }

    #[test]
    fn apply_and_map() {
        let doc = "hello world";
        let edits = [TextEdit::new(0..5, "bye"), TextEdit::insert(11, "!")];
        assert_eq!(apply_edits(doc, &edits), "bye world!");
        assert_eq!(map_pos(6, &edits, Assoc::Before), 4);
        assert_eq!(map_pos(2, &edits, Assoc::Before), 0);
        assert_eq!(map_pos(2, &edits, Assoc::After), 3);
        assert_eq!(map_pos(11, &edits, Assoc::Before), 9);
        assert_eq!(map_pos(11, &edits, Assoc::After), 10);
        assert_eq!(map_pos(0, &[TextEdit::insert(0, "xx")], Assoc::Before), 0);
        assert_eq!(map_pos(0, &[TextEdit::insert(0, "xx")], Assoc::After), 2);
    }

    #[test]
    fn rope_roundtrip() {
        let r = ropey::Rope::from_str("a\nb");
        assert_eq!(rope_to_string(&r), "a\nb");
    }
}
