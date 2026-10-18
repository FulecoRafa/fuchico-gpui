//! Selections with CodeMirror semantics.
//!
//! Ranges are stored in **CodeMirror coordinates**: `anchor`/`head` are char
//! offsets and the selection end is exclusive. In normal/select mode a
//! "block cursor" on char `p` is the 1-wide range `anchor = p + 1,
//! head = p` (cm-helix keeps it backward). cm-helix's "internal" helix
//! coordinates (inclusive end) are obtained with [`cm_to_internal`] /
//! [`internal_to_cm`].

use crate::change::ChangeSet;
use crate::text::{Text, next_cluster_break};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub anchor: usize,
    pub head: usize,
    /// Goal for vertical motion (host-defined units; the default logical
    /// [`crate::VisualLines`] implementation uses text columns).
    pub goal: Option<usize>,
}

impl Range {
    pub fn new(anchor: usize, head: usize) -> Self {
        Range {
            anchor,
            head,
            goal: None,
        }
    }
    pub fn with_goal(anchor: usize, head: usize, goal: Option<usize>) -> Self {
        Range { anchor, head, goal }
    }
    pub fn cursor(pos: usize) -> Self {
        Range::new(pos, pos)
    }
    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    pub fn len(&self) -> usize {
        self.to() - self.from()
    }
    /// cm-helix `rangeIsForward`: `head > from`.
    pub fn is_forward(&self) -> bool {
        self.head > self.from()
    }
    /// cm-helix `rangeIsAtomic`: at most one char wide.
    pub fn is_atomic(&self) -> bool {
        self.len() <= 1
    }
    /// Same anchor/head (goal ignored), like CodeMirror `SelectionRange.eq`.
    pub fn same(&self, other: &Range) -> bool {
        self.anchor == other.anchor && self.head == other.head
    }

    /// `SelectionRange.map(change, assoc)`.
    pub fn map(&self, changes: &ChangeSet, assoc: i32) -> Range {
        let (from, to) = if self.is_empty() {
            let p = changes.map_pos(self.from(), assoc);
            (p, p)
        } else {
            (
                changes.map_pos(self.from(), 1),
                changes.map_pos(self.to(), -1),
            )
        };
        if self.anchor > self.head {
            Range::with_goal(to, from, self.goal)
        } else {
            Range::with_goal(from, to, self.goal)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    ranges: Vec<Range>,
    main: usize,
}

impl Selection {
    /// `EditorSelection.single(anchor, head)` (drops goal).
    pub fn single(anchor: usize, head: usize) -> Self {
        Selection {
            ranges: vec![Range::new(anchor, head)],
            main: 0,
        }
    }

    pub fn from_range(range: Range) -> Self {
        Selection {
            ranges: vec![range],
            main: 0,
        }
    }

    /// `EditorSelection.create(ranges, mainIndex)`: sorts and merges
    /// overlapping ranges.
    pub fn create(ranges: Vec<Range>, main: usize) -> Self {
        assert!(!ranges.is_empty(), "a selection needs at least one range");
        let main = main.min(ranges.len() - 1);
        let mut pos = 0;
        let mut needs_norm = false;
        for r in &ranges {
            if if r.is_empty() {
                r.from() <= pos
            } else {
                r.from() < pos
            } {
                needs_norm = true;
                break;
            }
            pos = r.to();
        }
        if !needs_norm {
            return Selection { ranges, main };
        }
        Self::normalized(ranges, main)
    }

    fn normalized(ranges: Vec<Range>, main: usize) -> Self {
        let mut indexed: Vec<(usize, Range)> = ranges.into_iter().enumerate().collect();
        indexed.sort_by_key(|(_, r)| r.from());
        let mut main_index = indexed.iter().position(|(i, _)| *i == main).unwrap_or(0);
        let mut ranges: Vec<Range> = indexed.into_iter().map(|(_, r)| r).collect();
        let mut i = 1;
        while i < ranges.len() {
            let range = ranges[i];
            let prev = ranges[i - 1];
            let overlaps = if range.is_empty() {
                range.from() <= prev.to()
            } else {
                range.from() < prev.to()
            };
            if overlaps {
                let from = prev.from();
                let to = range.to().max(prev.to());
                if i <= main_index {
                    main_index -= 1;
                }
                let merged = if range.anchor > range.head {
                    Range::new(to, from)
                } else {
                    Range::new(from, to)
                };
                ranges.splice(i - 1..=i, [merged]);
                // re-check at the same index
            } else {
                i += 1;
            }
        }
        Selection {
            ranges,
            main: main_index,
        }
    }

    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }
    pub fn main_index(&self) -> usize {
        self.main
    }
    pub fn main(&self) -> Range {
        self.ranges[self.main]
    }
    pub fn len(&self) -> usize {
        self.ranges.len()
    }
    pub fn is_empty(&self) -> bool {
        false
    }

    /// `selection.asSingle()`.
    pub fn as_single(&self) -> Selection {
        Selection {
            ranges: vec![self.main()],
            main: 0,
        }
    }

    /// `selection.addRange(range, main = true)`.
    pub fn add_range(&self, range: Range, main: bool) -> Selection {
        let mut ranges = vec![range];
        ranges.extend_from_slice(&self.ranges);
        Selection::create(ranges, if main { 0 } else { self.main + 1 })
    }

    /// `EditorSelection.map(changes)`.
    pub fn map(&self, changes: &ChangeSet, assoc: i32) -> Selection {
        Selection::create(
            self.ranges.iter().map(|r| r.map(changes, assoc)).collect(),
            self.main,
        )
    }

    /// Same ranges and main index (goal ignored), like `EditorSelection.eq`.
    pub fn same(&self, other: &Selection) -> bool {
        self.main == other.main
            && self.ranges.len() == other.ranges.len()
            && self
                .ranges
                .iter()
                .zip(&other.ranges)
                .all(|(a, b)| a.same(b))
    }

    pub fn clamp(&self, len: usize) -> Selection {
        Selection::create(
            self.ranges
                .iter()
                .map(|r| Range::with_goal(r.anchor.min(len), r.head.min(len), r.goal))
                .collect(),
            self.main,
        )
    }
}

/// cm-helix `mapSel`: with a single range the result is rebuilt with
/// `EditorSelection.single`, which drops the goal column (a reproduced quirk).
pub fn map_sel(sel: &Selection, mut f: impl FnMut(Range) -> Range) -> Selection {
    if sel.len() == 1 {
        let r = f(sel.main());
        return Selection::single(r.anchor, r.head);
    }
    Selection::create(
        sel.ranges().iter().map(|r| f(*r)).collect(),
        sel.main_index(),
    )
}

/// cm-helix `cmSelToInternal`.
pub fn cm_to_internal(range: Range, doc: &Text) -> Range {
    if range.is_empty() {
        return range;
    }
    let end = next_cluster_break(doc, range.to(), false);
    if range.is_forward() {
        Range::with_goal(range.from(), end, range.goal)
    } else {
        Range::with_goal(end, range.from(), range.goal)
    }
}

/// cm-helix `internalSelToCM`.
pub fn internal_to_cm(range: Range, doc: &Text) -> Range {
    let end = next_cluster_break(doc, range.to(), true);
    if range.is_forward() {
        Range::with_goal(range.from(), end, range.goal)
    } else {
        Range::with_goal(end, range.from(), range.goal)
    }
}
