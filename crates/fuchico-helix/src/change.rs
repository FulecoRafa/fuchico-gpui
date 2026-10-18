//! Change sets with CodeMirror position-mapping semantics.

use crate::text::{Text, char_len};

/// A single replacement in the coordinates of the document *before* the
/// change set is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

impl Change {
    pub fn new(from: usize, to: usize, insert: impl Into<String>) -> Self {
        Change {
            from,
            to,
            insert: insert.into(),
        }
    }
    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Change::new(at, at, text)
    }
    pub fn delete(from: usize, to: usize) -> Self {
        Change::new(from, to, "")
    }
}

/// A set of non-overlapping changes, sorted by position. Insertions at the
/// same position keep their specification order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    changes: Vec<Change>,
}

/// One section of the change set (`len` chars of the old doc, replaced by
/// `ins` chars when `ins` is `Some`).
#[derive(Debug, Clone, Copy)]
struct Section {
    len: usize,
    ins: Option<usize>,
}

impl ChangeSet {
    pub fn new(mut changes: Vec<Change>) -> Self {
        changes.retain(|c| c.from != c.to || !c.insert.is_empty());
        // Stable sort keeps spec order for equal positions.
        changes.sort_by_key(|c| c.from);
        // Merge overlapping deletions conservatively (later replaces from end
        // of the previous one) so the set stays well formed.
        let mut out: Vec<Change> = Vec::with_capacity(changes.len());
        for c in changes {
            if let Some(last) = out.last_mut()
                && c.from < last.to
            {
                let to = c.to.max(last.to);
                last.insert.push_str(&c.insert);
                last.to = to;
                continue;
            }
            out.push(c);
        }
        ChangeSet { changes: out }
    }

    pub fn empty() -> Self {
        ChangeSet::default()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    fn sections(&self, doc_len: Option<usize>) -> Vec<Section> {
        let mut out: Vec<Section> = Vec::new();
        let mut pos = 0;
        for c in &self.changes {
            if c.from > pos {
                out.push(Section {
                    len: c.from - pos,
                    ins: None,
                });
            }
            let ins = char_len(&c.insert);
            // Consecutive pure insertions at one position form one section
            // (CodeMirror `addSection`); anything else stays separate.
            if let Some(last) = out.last_mut()
                && let Some(prev_ins) = last.ins
                && c.from == pos
                && last.len == 0
                && c.from == c.to
            {
                last.ins = Some(prev_ins + ins);
                pos = c.to;
                continue;
            }
            out.push(Section {
                len: c.to - c.from,
                ins: Some(ins),
            });
            pos = c.to;
        }
        if let Some(len) = doc_len
            && len > pos
        {
            out.push(Section {
                len: len - pos,
                ins: None,
            });
        }
        out
    }

    /// CodeMirror `ChangeDesc.mapPos(pos, assoc)` (simple mode).
    pub fn map_pos(&self, pos: usize, assoc: i32) -> usize {
        let mut pos_a = 0usize;
        let mut pos_b = 0usize;
        for s in self.sections(None) {
            let end_a = pos_a + s.len;
            match s.ins {
                None => {
                    if end_a > pos {
                        return pos_b + (pos - pos_a);
                    }
                    pos_b += s.len;
                }
                Some(ins) => {
                    if end_a > pos || (end_a == pos && assoc < 0 && s.len == 0) {
                        return if pos == pos_a || assoc < 0 {
                            pos_b
                        } else {
                            pos_b + ins
                        };
                    }
                    pos_b += ins;
                }
            }
            pos_a = end_a;
        }
        pos_b + pos.saturating_sub(pos_a)
    }

    /// Length change of the document.
    pub fn delta(&self) -> isize {
        self.changes
            .iter()
            .map(|c| char_len(&c.insert) as isize - (c.to - c.from) as isize)
            .sum()
    }

    /// Apply to `doc` (changes are applied back to front).
    pub fn apply(&self, doc: &mut Text) {
        for c in self.changes.iter().rev() {
            doc.replace(c.from, c.to, &c.insert);
        }
    }

    /// Byte/char description of every change, relative to `old`.
    pub fn describe(&self, old: &Text) -> Vec<TextEdit> {
        let mut delta: isize = 0;
        let mut delta_bytes: isize = 0;
        self.changes
            .iter()
            .map(|c| {
                let from_byte = old.char_to_byte(c.from);
                let to_byte = old.char_to_byte(c.to);
                let ins_chars = char_len(&c.insert);
                let e = TextEdit {
                    from: c.from,
                    to: c.to,
                    from_byte,
                    to_byte,
                    new_from: (c.from as isize + delta) as usize,
                    new_from_byte: (from_byte as isize + delta_bytes) as usize,
                    insert: c.insert.clone(),
                };
                delta += ins_chars as isize - (c.to - c.from) as isize;
                delta_bytes += c.insert.len() as isize - (to_byte - from_byte) as isize;
                e
            })
            .collect()
    }
}

/// One edit of a document change, for incremental re-parsing by the host.
///
/// `from..to` / `from_byte..to_byte` are in the **old** document;
/// `new_from` / `new_from_byte` are where the inserted text starts in the
/// **new** document. Edits are sorted and non-overlapping; applying them
/// back-to-front on the old text yields the new text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub from: usize,
    pub to: usize,
    pub from_byte: usize,
    pub to_byte: usize,
    pub new_from: usize,
    pub new_from_byte: usize,
    pub insert: String,
}

impl TextEdit {
    pub fn new_to(&self) -> usize {
        self.new_from + char_len(&self.insert)
    }
    pub fn new_to_byte(&self) -> usize {
        self.new_from_byte + self.insert.len()
    }
}

/// Minimal single replacement turning `old` into `new` (common prefix and
/// suffix stripped). Used for undo/redo restores.
pub fn diff(old: &Text, new: &Text) -> ChangeSet {
    let a: Vec<char> = old.to_string().chars().collect();
    let b: Vec<char> = new.to_string().chars().collect();
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }
    if pre == a.len() && pre == b.len() {
        return ChangeSet::empty();
    }
    let insert: String = b[pre..b.len() - suf].iter().collect();
    ChangeSet::new(vec![Change::new(pre, a.len() - suf, insert)])
}
