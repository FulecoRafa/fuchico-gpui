//! Document model: a [`ropey::Rope`] plus a CodeMirror-compatible line index.
//!
//! All positions in this crate are **char offsets** (Unicode scalar values).
//! CodeMirror counts UTF-16 code units; the two agree for the BMP, which is
//! what the behavioural tests exercise. Lines are separated by `"\n"` only,
//! exactly like a CodeMirror `Text` (input is normalised from `\r\n`/`\r`).

use ropey::Rope;
use unicode_segmentation::GraphemeCursor;

/// A line of the document, CodeMirror style: `from..to` excludes the line
/// break, `number` is 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub number: usize,
    pub from: usize,
    pub to: usize,
}

impl Line {
    pub fn len(&self) -> usize {
        self.to - self.from
    }
    pub fn is_empty(&self) -> bool {
        self.to == self.from
    }
}

/// The document. Cheap to clone (rope sharing + a `Vec` of line starts).
#[derive(Debug, Clone, Default)]
pub struct Text {
    rope: Rope,
    /// Char offset of the start of every line (always starts with 0).
    line_starts: Vec<usize>,
}

/// Normalise line endings to `\n` (CodeMirror does the same on input).
pub fn normalize_newlines(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\r') {
        std::borrow::Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

impl Text {
    pub fn new(s: &str) -> Self {
        let s = normalize_newlines(s);
        let rope = Rope::from_str(&s);
        let mut line_starts = vec![0];
        for (i, c) in s.chars().enumerate() {
            if c == '\n' {
                line_starts.push(i + 1);
            }
        }
        Text { rope, line_starts }
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Length in chars.
    pub fn len(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of lines (≥ 1).
    pub fn lines(&self) -> usize {
        self.line_starts.len()
    }

    /// The 1-based line `n` (clamped to the valid range).
    pub fn line(&self, n: usize) -> Line {
        let n = n.clamp(1, self.lines());
        let from = self.line_starts[n - 1];
        let to = if n < self.lines() {
            self.line_starts[n] - 1
        } else {
            self.len()
        };
        Line {
            number: n,
            from,
            to,
        }
    }

    /// The line containing `pos` (a position at a line break belongs to the
    /// line it ends).
    pub fn line_at(&self, pos: usize) -> Line {
        let pos = pos.min(self.len());
        let idx = match self.line_starts.binary_search(&pos) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        self.line(idx + 1)
    }

    pub fn slice(&self, from: usize, to: usize) -> String {
        let len = self.len();
        let (from, to) = (from.min(len), to.min(len));
        if from >= to {
            return String::new();
        }
        self.rope.slice(from..to).to_string()
    }

    pub fn line_text(&self, line: &Line) -> String {
        self.slice(line.from, line.to)
    }

    pub fn char_at(&self, pos: usize) -> Option<char> {
        if pos < self.len() {
            Some(self.rope.char(pos))
        } else {
            None
        }
    }

    pub fn char_to_byte(&self, pos: usize) -> usize {
        self.rope.char_to_byte(pos.min(self.len()))
    }

    pub fn byte_to_char(&self, byte: usize) -> usize {
        self.rope.byte_to_char(byte.min(self.rope.len_bytes()))
    }

    /// Replace `from..to` with `insert` (already normalised), keeping the
    /// line index up to date incrementally.
    pub fn replace(&mut self, from: usize, to: usize, insert: &str) {
        debug_assert!(from <= to && to <= self.len());
        if from < to {
            self.rope.remove(from..to);
        }
        if !insert.is_empty() {
            self.rope.insert(from, insert);
        }
        let ins_len = insert.chars().count();
        // Line starts strictly inside (from, to] are removed; later ones shift.
        let first_after = self.line_starts.partition_point(|&s| s <= from);
        let first_kept = self.line_starts.partition_point(|&s| s <= to);
        let mut new_starts: Vec<usize> = Vec::new();
        for (i, c) in insert.chars().enumerate() {
            if c == '\n' {
                new_starts.push(from + i + 1);
            }
        }
        let tail: Vec<usize> = self.line_starts[first_kept..]
            .iter()
            .map(|&s| s - to + from + ins_len)
            .collect();
        self.line_starts.truncate(first_after);
        self.line_starts.extend(new_starts);
        self.line_starts.extend(tail);
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for chunk in self.rope.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.rope == other.rope
    }
}

// ---------------------------------------------------------------------------
// Grapheme clusters (CodeMirror `findClusterBreak`).

/// Char offset → byte offset within `s`.
pub fn char_to_byte_in(s: &str, pos: usize) -> usize {
    s.char_indices().nth(pos).map(|(b, _)| b).unwrap_or(s.len())
}

/// Byte offset → char offset within `s`.
pub fn byte_to_char_in(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// `findClusterBreak(text, pos, forward)` in char offsets.
pub fn find_cluster_break(text: &str, pos: usize, forward: bool) -> usize {
    let byte = char_to_byte_in(text, pos);
    let mut cursor = GraphemeCursor::new(byte, text.len(), true);
    let next = if forward {
        cursor.next_boundary(text, 0).ok().flatten()
    } else {
        cursor.prev_boundary(text, 0).ok().flatten()
    };
    match next {
        Some(b) => byte_to_char_in(text, b),
        None => pos,
    }
}

/// Number of grapheme clusters in `s`.
pub fn grapheme_count(s: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    s.graphemes(true).count()
}

/// cm-helix `nextClusterBreak`: a line break counts as one cluster.
pub fn next_cluster_break(doc: &Text, pos: usize, forward: bool) -> usize {
    if pos == if forward { doc.len() } else { 0 } {
        return pos;
    }
    let line = doc.line_at(pos);
    if forward && line.to == pos {
        return pos + 1;
    } else if !forward && line.from == pos {
        return pos - 1;
    }
    let text = doc.line_text(&line);
    find_cluster_break(&text, pos - line.from, forward) + line.from
}

// ---------------------------------------------------------------------------
// Char categories (CodeMirror `charCategorizer` without language word chars).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharCategory {
    Word,
    Space,
    Other,
}

pub fn categorize(s: &str) -> CharCategory {
    if s.chars().all(char::is_whitespace) {
        return CharCategory::Space;
    }
    if s.chars().any(|c| c.is_alphanumeric() || c == '_') {
        return CharCategory::Word;
    }
    CharCategory::Other
}

/// "WORD" categoriser used by the W/B/E extensions: anything non-blank.
pub fn categorize_long(s: &str) -> CharCategory {
    if s.chars().all(char::is_whitespace) {
        CharCategory::Space
    } else {
        CharCategory::Word
    }
}

/// CodeMirror `view.moveByChar` for one step: returns the next position and
/// the text moved over (`"\n"` when crossing a line). `None` at doc boundary.
pub fn step_char(doc: &Text, pos: usize, forward: bool) -> Option<(usize, String)> {
    let line = doc.line_at(pos);
    if forward {
        if pos == line.to {
            if pos == doc.len() {
                return None;
            }
            return Some((pos + 1, "\n".into()));
        }
    } else if pos == line.from {
        if pos == 0 {
            return None;
        }
        return Some((pos - 1, "\n".into()));
    }
    let text = doc.line_text(&line);
    let next = find_cluster_break(&text, pos - line.from, forward) + line.from;
    let (a, b) = if forward { (pos, next) } else { (next, pos) };
    Some((next, doc.slice(a, b)))
}

/// A `moveByChar` continuation check (`by(firstChar)` in CodeMirror).
pub type CharCheck = Box<dyn FnMut(&str) -> bool>;

/// `view.moveByChar(cursor, forward, by)` where the first step is always
/// taken and `check` decides whether to continue with each later step.
pub fn move_by_char_with(
    doc: &Text,
    start: usize,
    forward: bool,
    mut by: Option<&mut dyn FnMut(&str) -> CharCheck>,
) -> usize {
    let mut cur = start;
    let mut check: Option<CharCheck> = None;
    loop {
        let Some((next, ch)) = step_char(doc, cur, forward) else {
            return cur;
        };
        match check.as_mut() {
            None => match by.as_mut() {
                None => return next,
                Some(f) => check = Some(f(&ch)),
            },
            Some(c) => {
                if !c(&ch) {
                    return cur;
                }
            }
        }
        cur = next;
    }
}

/// Move `count` grapheme steps (cm-helix `cursorByChar` with count).
pub fn move_by_chars(doc: &Text, start: usize, forward: bool, count: usize) -> usize {
    if count <= 1 {
        return move_by_char_with(doc, start, forward, None);
    }
    let mut make = |_: &str| -> Box<dyn FnMut(&str) -> bool> {
        let mut counter = count;
        Box::new(move |_: &str| {
            counter -= 1;
            counter > 0
        })
    };
    move_by_char_with(doc, start, forward, Some(&mut make))
}

/// CodeMirror `view.moveByGroup` (byGroup with the given categoriser).
pub fn move_by_group_with(
    doc: &Text,
    start: usize,
    forward: bool,
    cat_fn: fn(&str) -> CharCategory,
) -> usize {
    let mut make = move |first: &str| -> Box<dyn FnMut(&str) -> bool> {
        let mut cat = cat_fn(first);
        Box::new(move |next: &str| {
            let next_cat = cat_fn(next);
            if cat == CharCategory::Space {
                cat = next_cat;
            }
            cat == next_cat
        })
    };
    move_by_char_with(doc, start, forward, Some(&mut make))
}

pub fn move_by_group(doc: &Text, start: usize, forward: bool) -> usize {
    move_by_group_with(doc, start, forward, categorize)
}

// ---------------------------------------------------------------------------
// Columns (CodeMirror `countColumn` / `findColumn`).

pub fn count_column(text: &str, tab_size: usize, to: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut n = 0;
    let mut i = 0;
    while i < to && i < chars.len() {
        if chars[i] == '\t' {
            n += tab_size - (n % tab_size);
            i += 1;
        } else {
            n += 1;
            i = find_cluster_break(text, i, true);
        }
    }
    n
}

/// Returns the char offset for column `col`; `None` when `strict` and the
/// line is too short (CodeMirror returns -1).
pub fn find_column(text: &str, col: usize, tab_size: usize, strict: bool) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut n = 0;
    loop {
        if n >= col {
            return Some(i);
        }
        if i == chars.len() {
            break;
        }
        n += if chars[i] == '\t' {
            tab_size - (n % tab_size)
        } else {
            1
        };
        i = find_cluster_break(text, i, true);
    }
    if strict { None } else { Some(chars.len()) }
}

/// Leading whitespace (`/^\s*/`) of `s`, in chars.
pub fn leading_ws_len(s: &str) -> usize {
    s.chars().take_while(|c| c.is_whitespace()).count()
}

/// Index (in chars) of the first non-whitespace char (`/\S/`), or 0.
pub fn first_non_ws(s: &str) -> usize {
    s.chars().position(|c| !c.is_whitespace()).unwrap_or(0)
}

/// `indentString(state, cols)`.
pub fn indent_string(cols: usize, tab_size: usize, unit_is_tab: bool) -> String {
    let mut out = String::new();
    let mut cols = cols;
    if unit_is_tab {
        while cols >= tab_size {
            out.push('\t');
            cols -= tab_size;
        }
    }
    for _ in 0..cols {
        out.push(' ');
    }
    out
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}
