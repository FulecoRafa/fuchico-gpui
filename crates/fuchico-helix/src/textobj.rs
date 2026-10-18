//! Bracket matching and text objects (cm-helix `match` minor mode).

use crate::change::Change;
use crate::commands::ncb;
use crate::editor::{Editor, Tr};
use crate::mode::Mode;
use crate::selection::{Range, Selection, cm_to_internal, internal_to_cm};
use crate::text::{CharCategory, Text, categorize};

/// cm-helix `PAIRS` lookup: `(open, close)` for `c` (a non-bracket char
/// pairs with itself).
pub(crate) fn pair_for(c: char) -> (char, char) {
    match c {
        '(' | ')' => ('(', ')'),
        '{' | '}' => ('{', '}'),
        '[' | ']' => ('[', ']'),
        '<' | '>' => ('<', '>'),
        c => (c, c),
    }
}

const MAX_SCAN: usize = 10_000;

/// A matched bracket token: `end` is `None` when the scan hit the document
/// boundary (CodeMirror `{start, matched: false}`).
pub(crate) struct BracketMatch {
    pub end: Option<(usize, usize)>,
    #[allow(dead_code)]
    pub matched: bool,
}

/// CodeMirror `matchBrackets` (plain-text scan; the syntax-tree token-type
/// check is not reproduced).
pub(crate) fn match_brackets(
    doc: &Text,
    pos: usize,
    forward: bool,
    brackets: &str,
) -> Option<BracketMatch> {
    let br: Vec<char> = brackets.chars().collect();
    let start_ch = if forward {
        doc.char_at(pos)
    } else if pos > 0 {
        doc.char_at(pos - 1)
    } else {
        None
    }?;
    let bracket = br.iter().position(|c| *c == start_ch)?;
    if (bracket % 2 == 0) != forward {
        return None;
    }
    let rope = doc.rope();
    let mut depth = 0usize;
    if forward {
        for (dist, (i, ch)) in rope
            .chars_at(pos)
            .enumerate()
            .map(|(k, c)| (k, (pos + k, c)))
        {
            if dist > MAX_SCAN {
                return None;
            }
            let Some(found) = br.iter().position(|c| *c == ch) else {
                continue;
            };
            if found % 2 == 0 {
                depth += 1;
            } else if depth == 1 {
                return Some(BracketMatch {
                    end: Some((i, i + 1)),
                    matched: found >> 1 == bracket >> 1,
                });
            } else {
                depth = depth.saturating_sub(1);
            }
        }
    } else {
        let mut it = rope.chars_at(pos);
        let mut i = pos;
        let mut dist = 0;
        while let Some(ch) = it.prev() {
            i -= 1;
            dist += 1;
            if dist > MAX_SCAN {
                return None;
            }
            let Some(found) = br.iter().position(|c| *c == ch) else {
                continue;
            };
            if found % 2 == 1 {
                depth += 1;
            } else if depth == 1 {
                return Some(BracketMatch {
                    end: Some((i, i + 1)),
                    matched: found >> 1 == bracket >> 1,
                });
            } else {
                depth = depth.saturating_sub(1);
            }
        }
    }
    Some(BracketMatch {
        end: None,
        matched: false,
    })
}

const STARTS: [char; 3] = ['(', '{', '['];
const ENDS: [char; 3] = [')', '}', ']'];
const DUOS: [char; 2] = ['"', '\''];

/// cm-helix `matchInBrackets` (`mim` / `mam`) for one position.
fn match_in_brackets_at(doc: &[char], pos: usize, inclusive: bool) -> Option<Range> {
    let len = doc.len();
    let (mut start_char, mut end_char) = (None, None);
    let (mut start_pos, mut end_pos) = (0usize, 0usize);
    let mut depth: i64 = 0;
    for i in (0..pos.min(len)).rev() {
        if ENDS.contains(&doc[i]) {
            depth += 1;
        }
        if STARTS.contains(&doc[i]) || DUOS.contains(&doc[i]) {
            if depth == 0 {
                start_pos = i + 1;
                start_char = Some(doc[i]);
                break;
            } else if STARTS.contains(&doc[i]) {
                depth -= 1;
            }
        }
    }
    depth = 0;
    for (i, &ch) in doc.iter().enumerate().skip(pos) {
        if STARTS.contains(&ch) {
            depth += 1;
        }
        if ENDS.contains(&ch) || DUOS.contains(&ch) {
            if depth == 0 {
                end_pos = i;
                end_char = Some(ch);
                break;
            } else if ENDS.contains(&ch) {
                depth -= 1;
            }
        }
    }
    let (start_char, end_char) = (start_char?, end_char?);
    let mut from: i64 = -1;
    let mut to: i64 = -1;
    if pos as i64 - start_pos as i64 > end_pos as i64 - pos as i64 {
        to = end_pos as i64;
        let search = pair_for(end_char).0;
        if end_char == search {
            if let Some(i) = (0..end_pos).rev().find(|&i| doc[i] == search) {
                from = i as i64 + 1;
            }
        } else {
            for i in (0..end_pos).rev() {
                if ENDS.contains(&doc[i]) {
                    depth += 1;
                }
                if STARTS.contains(&doc[i]) {
                    if depth == 0 {
                        if doc[i] == search {
                            from = i as i64 + 1;
                            break;
                        }
                    } else {
                        depth -= 1;
                    }
                }
            }
        }
    } else {
        from = start_pos as i64;
        let search = pair_for(start_char).1;
        if start_char == search {
            if let Some(i) = (start_pos..len).find(|&i| doc[i] == search) {
                to = i as i64;
            }
        } else {
            for (i, &ch) in doc.iter().enumerate().skip(start_pos) {
                if STARTS.contains(&ch) {
                    depth += 1;
                }
                if ENDS.contains(&ch) {
                    if depth == 0 && ch == search {
                        to = i as i64;
                        break;
                    } else {
                        depth -= 1;
                    }
                }
            }
        }
    }
    if from < 0 || to < 0 {
        return None;
    }
    let (mut from, mut to) = (from as usize, to as usize);
    if inclusive {
        if to < len {
            to += 1;
        }
        from = from.saturating_sub(1);
    }
    Some(Range::new(from, to))
}

/// cm-helix `findEmptyLine`.
fn find_empty_line(doc: &Text, start: usize, forward: bool) -> Option<crate::text::Line> {
    if forward {
        (start..=doc.lines())
            .map(|n| doc.line(n))
            .find(|l| l.is_empty())
    } else {
        (1..=start)
            .rev()
            .map(|n| doc.line(n))
            .find(|l| l.is_empty())
    }
}

impl Editor {
    fn brackets(&self) -> &'static str {
        if self.config.extensions {
            "()[]{}<>"
        } else {
            "()[]{}"
        }
    }

    /// `mm`.
    pub(crate) fn match_bracket(&mut self) {
        let normal = self.mode.ty == crate::mode::ModeType::Normal;
        let doc = &self.doc;
        let brackets = self.brackets();
        let main_internal = cm_to_internal(self.sel.main(), doc);
        let ranges: Vec<Range> = self
            .sel
            .ranges()
            .iter()
            .map(|&r| {
                let internal = cm_to_internal(r, doc);
                let collapsed = internal_to_cm(Range::cursor(internal.head), doc);
                let ch = doc.slice(collapsed.from(), collapsed.to());
                let mut chars = ch.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return r;
                };
                if !matches!(
                    c,
                    '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>' | '"' | '\''
                ) {
                    return r;
                }
                let open = matches!(c, '(' | '{' | '[' | '<');
                let pos = collapsed.head + usize::from(!open);
                let Some((f, t)) = match_brackets(doc, pos, open, brackets).and_then(|m| m.end)
                else {
                    return r;
                };
                if normal {
                    Range::new(f, t)
                } else {
                    internal_to_cm(Range::new(main_internal.anchor, f), doc)
                }
            })
            .collect();
        let m = self.reset_mode();
        self.dispatch(
            Tr::sel(Selection::create(ranges, self.sel.main_index()))
                .with_mode(m)
                .scrolled(),
        );
    }

    /// The range `ma<c>`/`mi<c>` selects for `r`, or `None`.
    fn delimiter_range(&self, r: Range, c: char, inclusive: bool) -> Option<Range> {
        let doc = &self.doc;
        if self.config.extensions {
            match c {
                'w' | 'W' => return Some(self.word_object(r, c == 'W', inclusive)),
                '"' | '\'' | '`' => return self.quote_object(r, c, inclusive),
                _ => {}
            }
        }
        let (open, close) = pair_for(c);
        let brackets = self.brackets();
        // forward search for open|close from range.head
        let is_delim = |ch: char| ch == open || ch == close;
        let head = r.head;
        let next = doc
            .rope()
            .chars_at(head)
            .position(is_delim)
            .map(|i| head + i)?;
        let mut forward = true;
        let mut tok = next;
        if open != close && doc.char_at(next) == Some(open) {
            let mut it = doc.rope().chars_at(head);
            let mut i = head;
            let mut last = None;
            while let Some(ch) = it.prev() {
                i -= 1;
                if is_delim(ch) {
                    last = Some(i);
                    break;
                }
            }
            let last = last?;
            if doc.char_at(last) == Some(close) {
                return None;
            }
            forward = false;
            tok = last;
        }
        let m = match_brackets(doc, if forward { tok + 1 } else { tok }, !forward, brackets)?;
        let (mf, mt) = m.end?;
        let (mut start, mut end) = if forward {
            ((mf, mt), (tok, tok + 1))
        } else {
            ((tok, tok + 1), (mf, mt))
        };
        if !inclusive {
            start = (start.1, ncb(doc, start.1, true));
            end = (ncb(doc, end.0, false), end.0);
        }
        Some(if r.is_forward() || r.is_atomic() {
            Range::new(start.0, end.1)
        } else {
            Range::new(end.1, start.0)
        })
    }

    /// `miw`/`maw` (extension): the word (or WORD) under the block cursor;
    /// `maw` also takes the trailing (else leading) whitespace.
    fn word_object(&self, r: Range, long: bool, around: bool) -> Range {
        let doc = &self.doc;
        let cat = |s: &str| {
            if long {
                crate::text::categorize_long(s)
            } else {
                categorize(s)
            }
        };
        let head = cm_to_internal(r, doc).head;
        let line = doc.line_at(head);
        let chars: Vec<char> = doc.line_text(&line).chars().collect();
        if chars.is_empty() {
            return r;
        }
        let i = (head - line.from).min(chars.len() - 1);
        let k = cat(&chars[i].to_string());
        let mut a = i;
        while a > 0 && cat(&chars[a - 1].to_string()) == k {
            a -= 1;
        }
        let mut b = i + 1;
        while b < chars.len() && cat(&chars[b].to_string()) == k {
            b += 1;
        }
        if around && k != CharCategory::Space {
            let mut e = b;
            while e < chars.len() && chars[e].is_whitespace() {
                e += 1;
            }
            if e > b {
                b = e;
            } else {
                while a > 0 && chars[a - 1].is_whitespace() {
                    a -= 1;
                }
            }
        }
        Range::new(line.from + a, line.from + b)
    }

    /// `mi"` / `ma"` (extension): the quoted span on the cursor's line.
    fn quote_object(&self, r: Range, q: char, around: bool) -> Option<Range> {
        let doc = &self.doc;
        let head = cm_to_internal(r, doc).head;
        let line = doc.line_at(head);
        let quotes: Vec<usize> = doc
            .line_text(&line)
            .chars()
            .enumerate()
            .filter(|(_, c)| *c == q)
            .map(|(i, _)| line.from + i)
            .collect();
        let pair = quotes
            .as_chunks::<2>()
            .0
            .iter()
            .find(|p| p[0] <= head && head <= p[1])?;
        Some(if around {
            Range::new(pair[0], pair[1] + 1)
        } else {
            Range::new(pair[0] + 1, pair[1])
        })
    }

    /// cm-helix `extendToDelimiters` (`ma<c>` / `mi<c>`).
    pub(crate) fn extend_to_delimiters(&mut self, c: char, inclusive: bool) {
        let m = self.reset_mode();
        match c {
            'm' => {
                let doc: Vec<char> = self.doc.to_string().chars().collect();
                let ranges: Vec<Range> = self
                    .sel
                    .ranges()
                    .iter()
                    .filter_map(|r| match_in_brackets_at(&doc, r.from(), inclusive))
                    .collect();
                if !ranges.is_empty() {
                    let main = self.sel.main_index().min(ranges.len() - 1);
                    self.dispatch(Tr::sel(Selection::create(ranges, main)));
                }
                self.dispatch(Tr::mode(m));
            }
            'p' => {
                let doc = &self.doc;
                let ranges: Vec<Range> = self
                    .sel
                    .ranges()
                    .iter()
                    .map(|r| {
                        let n = doc.line_at(r.from()).number;
                        let start = find_empty_line(doc, n, false)
                            .map(|l| l.to + 1)
                            .unwrap_or(0);
                        let end = find_empty_line(doc, n, true)
                            .map(|l| l.to)
                            .unwrap_or(doc.len());
                        Range::new(start.min(doc.len()), end)
                    })
                    .collect();
                let main = self.sel.main_index();
                self.dispatch(Tr::sel(Selection::create(ranges, main)));
                self.dispatch(Tr::mode(m));
            }
            _ => {
                let ranges: Vec<Range> = self
                    .sel
                    .ranges()
                    .iter()
                    .map(|&r| self.delimiter_range(r, c, inclusive).unwrap_or(r))
                    .collect();
                let main = self.sel.main_index();
                self.dispatch(Tr::sel(Selection::create(ranges, main)).with_mode(m));
            }
        }
    }

    /// `md<c>` (extension): delete the surrounding pair.
    pub(crate) fn delete_surround(&mut self, c: char) {
        self.change_surround(c, None);
    }

    /// `mr<from><to>` (extension): replace the surrounding pair.
    pub(crate) fn replace_surround(&mut self, from: char, to: char) {
        self.change_surround(from, Some(to));
    }

    fn change_surround(&mut self, c: char, with: Option<char>) {
        let mut changes = Vec::new();
        for &r in self.sel.ranges() {
            let probe = if r.is_empty() {
                r
            } else {
                cm_to_internal(r, &self.doc)
            };
            let Some(around) = self.delimiter_range(
                Range::new(probe.from(), probe.to().max(probe.from() + 1)),
                c,
                true,
            ) else {
                continue;
            };
            let (f, t) = (around.from(), around.to());
            if t < f + 2 {
                continue;
            }
            let (o, cl) = match with {
                Some(w) => {
                    let (o, cl) = pair_for(w);
                    (o.to_string(), cl.to_string())
                }
                None => (String::new(), String::new()),
            };
            changes.push(Change::new(f, f + 1, o));
            changes.push(Change::new(t - 1, t, cl));
        }
        // overlapping pairs (several cursors in one pair) are applied once
        changes.sort_by_key(|c| c.from);
        changes.dedup_by_key(|c| c.from);
        let tr = Tr::changes(crate::change::ChangeSet::new(changes)).with_mode(Mode::NORMAL);
        self.dispatch(tr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_bracket_scan() {
        let doc = Text::new("a(b[c]d)e");
        let m = match_brackets(&doc, 1, true, "()[]{}").unwrap();
        assert_eq!(m.end, Some((7, 8)));
        assert!(m.matched);
        let m = match_brackets(&doc, 8, false, "()[]{}").unwrap();
        assert_eq!(m.end, Some((1, 2)));
        assert!(match_brackets(&doc, 0, true, "()[]{}").is_none());
        let m = match_brackets(&Text::new("(a]"), 0, true, "()[]{}").unwrap();
        assert!(!m.matched);
        let m = match_brackets(&Text::new("(a"), 0, true, "()[]{}").unwrap();
        assert!(m.end.is_none());
    }

    #[test]
    fn in_brackets() {
        let d: Vec<char> = "f(a, [b], c)".chars().collect();
        assert_eq!(match_in_brackets_at(&d, 3, false), Some(Range::new(2, 11)));
        assert_eq!(match_in_brackets_at(&d, 3, true), Some(Range::new(1, 12)));
        assert_eq!(match_in_brackets_at(&d, 6, false), Some(Range::new(6, 7)));
        assert_eq!(
            match_in_brackets_at(&"none".chars().collect::<Vec<_>>(), 2, false),
            None
        );
    }
}
