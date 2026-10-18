//! Pure search logic ported from `@codemirror/search` 6.7.1 (the original's
//! Mod-f panel, `selectNextOccurrence`, `selectSelectionMatches`,
//! `highlightSelectionMatches` and `gotoLine`). Everything works on the
//! document text with **byte** offsets; the view converts to chars.

use std::ops::Range;

use regex::{Regex, RegexBuilder};

/// `SearchQuery` (`literal: false`, the original's config).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub search: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub regexp: bool,
    pub whole_word: bool,
}

/// `\n`, `\r`, `\t`, `\\` escapes of non-literal queries (`unquote`).
pub fn unquote(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\'
            && let Some(&n) = it.peek()
            && matches!(n, 'n' | 'r' | 't' | '\\')
        {
            it.next();
            out.push(match n {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => '\\',
            });
        } else {
            out.push(c);
        }
    }
    out
}

/// CodeMirror's default word categorizer (`\p{Alphabetic}\p{Number}_`).
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn char_before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}
fn char_after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}
fn non_word(c: Option<char>) -> bool {
    !c.is_some_and(is_word)
}

/// `stringWordTest` / `regexpWordTest`.
fn word_ok(text: &str, m: &Range<usize>) -> bool {
    (non_word(char_before(text, m.start)) || non_word(char_after(text, m.start)))
        && (non_word(char_after(text, m.end)) || non_word(char_before(text, m.end)))
}

impl SearchQuery {
    /// `query.valid`: non-empty, and a valid regex in regexp mode.
    pub fn valid(&self) -> bool {
        !self.search.is_empty() && (!self.regexp || self.compile().is_some())
    }

    fn compile(&self) -> Option<Regex> {
        let src = if self.regexp {
            self.search.clone()
        } else {
            regex::escape(&unquote(&self.search))
        };
        RegexBuilder::new(&src)
            .multi_line(true)
            .case_insensitive(!self.case_sensitive)
            .build()
            .ok()
    }

    /// A compiled matcher, `None` when the query is not valid.
    pub fn matcher(&self) -> Option<Matcher> {
        if self.search.is_empty() {
            return None;
        }
        Some(Matcher {
            re: self.compile()?,
            whole_word: self.whole_word,
            regexp: self.regexp,
            qlen: if self.regexp {
                0
            } else {
                unquote(&self.search).len()
            },
            replace: unquote(&self.replace),
        })
    }
}

/// A compiled [`SearchQuery`].
#[derive(Debug, Clone)]
pub struct Matcher {
    re: Regex,
    whole_word: bool,
    regexp: bool,
    /// Byte length of the unquoted string query (`unquoted.length`).
    qlen: usize,
    replace: String,
}

fn floor_boundary(text: &str, mut b: usize) -> usize {
    b = b.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

impl Matcher {
    /// Non-empty matches inside `[from, to]`, in order (non-overlapping).
    pub fn matches_in<'a>(
        &'a self,
        text: &'a str,
        from: usize,
        to: usize,
    ) -> impl Iterator<Item = Range<usize>> + 'a {
        let to = floor_boundary(text, to);
        let mut pos = floor_boundary(text, from);
        std::iter::from_fn(move || {
            loop {
                if pos > to {
                    return None;
                }
                let m = self.re.find_at(text, pos)?;
                if m.end() > to {
                    return None;
                }
                if m.start() == m.end() {
                    // Zero-length regex matches are skipped.
                    pos = m.end() + char_after(text, m.end()).map_or(1, char::len_utf8);
                    continue;
                }
                pos = m.end();
                let r = m.range();
                if !self.whole_word || word_ok(text, &r) {
                    return Some(r);
                }
            }
        })
    }

    fn last_in(&self, text: &str, from: usize, to: usize) -> Option<Range<usize>> {
        self.matches_in(text, from, to).last()
    }

    /// `query.nextMatch(state, curFrom, curTo)`.
    pub fn next_match(&self, text: &str, cur_from: usize, cur_to: usize) -> Option<Range<usize>> {
        if let Some(m) = self.matches_in(text, cur_to, text.len()).next() {
            return Some(m);
        }
        if self.regexp {
            return self.matches_in(text, 0, cur_from).next();
        }
        let end = (cur_from + self.qlen).min(text.len());
        let m = self.matches_in(text, 0, end).next()?;
        (m != (cur_from..cur_to)).then_some(m)
    }

    /// `query.prevMatch(state, curFrom, curTo)`.
    pub fn prev_match(&self, text: &str, cur_from: usize, cur_to: usize) -> Option<Range<usize>> {
        if self.regexp {
            return self
                .last_in(text, 0, cur_from)
                .or_else(|| self.last_in(text, cur_to, text.len()));
        }
        let m = self
            .last_in(text, 0, cur_from)
            .or_else(|| self.last_in(text, cur_to.saturating_sub(self.qlen), text.len()))?;
        (m != (cur_from..cur_to)).then_some(m)
    }

    /// `query.matchAll(state, limit)`: `None` past `limit` matches.
    pub fn match_all(&self, text: &str, limit: usize) -> Option<Vec<Range<usize>>> {
        let mut out = Vec::new();
        for m in self.matches_in(text, 0, text.len()) {
            if out.len() >= limit {
                return None;
            }
            out.push(m);
        }
        Some(out)
    }

    /// `query.getReplacement(match)`: `$&`, `$$` and `$n` in regexp mode.
    pub fn replacement(&self, text: &str, m: &Range<usize>) -> String {
        if !self.regexp {
            return self.replace.clone();
        }
        let Some(caps) = self.re.captures_at(&text[..m.end], m.start) else {
            return self.replace.clone();
        };
        let mut out = String::new();
        let b = self.replace.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'$' && i + 1 < b.len() {
                let n = b[i + 1];
                if n == b'&' {
                    out.push_str(caps.get(0).map_or("", |c| c.as_str()));
                    i += 2;
                    continue;
                }
                if n == b'$' {
                    out.push('$');
                    i += 2;
                    continue;
                }
                if n.is_ascii_digit() {
                    let mut j = i + 1;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    let digits = &self.replace[i + 1..j];
                    let mut done = false;
                    for l in (1..=digits.len()).rev() {
                        let k: usize = digits[..l].parse().unwrap_or(0);
                        if k > 0 && k < caps.len() {
                            out.push_str(caps.get(k).map_or("", |c| c.as_str()));
                            out.push_str(&digits[l..]);
                            done = true;
                            break;
                        }
                    }
                    if !done {
                        out.push_str(&self.replace[i..j]);
                    }
                    i = j;
                    continue;
                }
            }
            let c = self.replace[i..].chars().next().unwrap_or('$');
            out.push(c);
            i += c.len_utf8();
        }
        out
    }
}

/// `defaultQuery` search text: the main selection when non-empty and at
/// most 100 chars (newlines as `\n`), else `None` (keep the previous).
pub fn default_search(selected: &str) -> Option<String> {
    let n = selected.chars().count();
    (n > 0 && n <= 100).then(|| selected.replace('\n', "\\n"))
}

/// `state.wordAt(pos)` on one line of `text` (byte offsets).
pub fn word_at(text: &str, pos: usize) -> Option<Range<usize>> {
    let pos = floor_boundary(text, pos);
    let line_from = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_to = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let mut start = pos;
    while start > line_from {
        let c = char_before(text, start).unwrap();
        if !is_word(c) {
            break;
        }
        start -= c.len_utf8();
    }
    let mut end = pos;
    while end < line_to {
        let c = char_after(text, end).unwrap();
        if !is_word(c) {
            break;
        }
        end += c.len_utf8();
    }
    (start != end).then_some(start..end)
}

/// Case-sensitive, non-overlapping occurrences of `q` in `text[from..to]`.
fn occurrences(text: &str, q: &str, from: usize, to: usize) -> Vec<Range<usize>> {
    let (from, to) = (floor_boundary(text, from), floor_boundary(text, to));
    if q.is_empty() || from > to {
        return Vec::new();
    }
    text[from..to]
        .match_indices(q)
        .map(|(i, s)| from + i..from + i + s.len())
        .collect()
}

/// `highlightSelectionMatches` (defaults: min length 1, 100 matches max,
/// no word-around-cursor, not whole words). `ranges` is the selection
/// (byte ranges), `visible` the painted part of the document. Returns the
/// ranges to paint with `.cm-selectionMatch`.
pub fn selection_matches(
    text: &str,
    ranges: &[Range<usize>],
    visible: Range<usize>,
) -> Vec<Range<usize>> {
    let [r] = ranges else { return Vec::new() };
    if r.start == r.end {
        return Vec::new();
    }
    let q = &text[r.clone()];
    let len = q.chars().count();
    if !(1..=200).contains(&len) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for m in occurrences(text, q, visible.start, visible.end) {
        if m.start >= r.end || m.end <= r.start {
            out.push(m);
            if out.len() > 100 {
                return Vec::new();
            }
        }
    }
    out
}

/// What `selectNextOccurrence` (Mod-d) does to the selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextOccurrence {
    /// Some range was empty: `selectWord` (every range becomes its word,
    /// or stays a cursor). Main index unchanged.
    Words(Vec<Range<usize>>),
    /// Add this range (not as main) and scroll to its end.
    Add(Range<usize>),
}

/// `selectNextOccurrence`. `ranges` are the selection's byte ranges in
/// document order; `main` indexes them. `heads` are the range heads.
pub fn select_next_occurrence(
    text: &str,
    ranges: &[Range<usize>],
    heads: &[usize],
    main: usize,
) -> Option<NextOccurrence> {
    if ranges.iter().any(|r| r.start == r.end) {
        let words: Vec<Range<usize>> = heads
            .iter()
            .map(|&h| word_at(text, h).unwrap_or(h..h))
            .collect();
        return (words.as_slice() != ranges).then_some(NextOccurrence::Words(words));
    }
    let q = &text[ranges[0].clone()];
    if ranges.iter().any(|r| &text[r.clone()] != q) {
        return None;
    }
    let mr = &ranges[main];
    let full_word = word_at(text, heads[main]).is_some_and(|w| w == *mr);
    let last = ranges.last()?;
    let ok = |m: &Range<usize>| !full_word || word_at(text, m.start).is_some_and(|w| w == *m);
    if let Some(m) = occurrences(text, q, last.end, text.len())
        .into_iter()
        .find(ok)
    {
        return Some(NextOccurrence::Add(m));
    }
    let end = prev_boundary(text, last.start);
    occurrences(text, q, 0, end)
        .into_iter()
        .filter(|m| !ranges.iter().any(|r| r.start == m.start))
        .find(ok)
        .map(NextOccurrence::Add)
}

fn prev_boundary(text: &str, at: usize) -> usize {
    char_before(text, at).map_or(0, |c| at - c.len_utf8())
}

/// `selectSelectionMatches` (Mod-Shift-l): every occurrence of the single
/// non-empty selection, with the main one where the selection starts.
pub fn select_selection_matches(
    text: &str,
    ranges: &[Range<usize>],
) -> Option<(Vec<Range<usize>>, usize)> {
    let [r] = ranges else { return None };
    if r.start == r.end {
        return None;
    }
    let all = occurrences(text, &text[r.clone()], 0, text.len());
    if all.len() > 1001 {
        return None;
    }
    let main = all.iter().position(|m| m.start == r.start).unwrap_or(0);
    Some((all, main))
}

/// `gotoLine` input (`/^([+-])?(\d+)?(:\d+)?(%)?$/`): the 1-based target
/// line (clamped) and the column offset (raw; the caller clamps it to the
/// line length). `None` = invalid input (the dialog just closes).
pub fn parse_goto(input: &str, cur_line: usize, lines: usize) -> Option<(usize, usize)> {
    let re = Regex::new(r"^([+-])?(\d+)?(:\d+)?(%)?$").expect("static regex");
    let c = re.captures(input)?;
    let sign = c.get(1).map(|m| m.as_str());
    let ln = c
        .get(2)
        .map(|m| m.as_str().parse::<f64>().unwrap_or(f64::MAX));
    let col = c.get(3).map_or(0, |m| {
        m.as_str()[1..].parse::<usize>().unwrap_or(usize::MAX)
    });
    let percent = c.get(4).is_some();
    let mut line = ln.unwrap_or(cur_line as f64);
    if let Some(n) = ln {
        if percent {
            let mut pc = n / 100.;
            if let Some(s) = sign {
                pc = pc * if s == "-" { -1. } else { 1. } + cur_line as f64 / lines as f64;
            }
            line = (lines as f64 * pc + 0.5).floor(); // Math.round
        } else if let Some(s) = sign {
            line = n * if s == "-" { -1. } else { 1. } + cur_line as f64;
        }
    }
    let line = line.clamp(1., lines as f64) as usize;
    Some((line, col))
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    fn q(s: &str) -> SearchQuery {
        SearchQuery {
            search: s.into(),
            ..Default::default()
        }
    }

    #[test]
    fn validity_and_unquote() {
        assert!(!q("").valid());
        assert!(q("a").valid());
        let mut r = q("(");
        r.regexp = true;
        assert!(!r.valid());
        assert_eq!(unquote(r"a\nb\tc\\d\x"), "a\nb\tc\\d\\x");
        let m = q(r"a\nb").matcher().unwrap();
        assert_eq!(m.match_all("xa\nb", 10).unwrap(), vec![1..4]);
    }

    #[test]
    fn case_and_whole_word() {
        let text = "Foo foo food _foo";
        let m = q("foo").matcher().unwrap();
        assert_eq!(
            m.match_all(text, 100).unwrap(),
            vec![0..3, 4..7, 8..11, 14..17]
        );
        let mut c = q("foo");
        c.case_sensitive = true;
        assert_eq!(c.matcher().unwrap().match_all(text, 100).unwrap().len(), 3);
        let mut w = q("foo");
        w.whole_word = true;
        assert_eq!(
            w.matcher().unwrap().match_all(text, 100).unwrap(),
            vec![0..3, 4..7]
        );
        // A query that itself starts with a non-word char matches inside words.
        let mut w2 = q("-b");
        w2.whole_word = true;
        assert_eq!(
            w2.matcher().unwrap().match_all("a-b", 10).unwrap(),
            vec![1..3]
        );
    }

    #[test]
    fn regexp_and_replacement() {
        let mut r = q(r"(\w)(\d)");
        r.regexp = true;
        r.replace = "<$2$1|$&|$$|$12|$9>".into();
        let m = r.matcher().unwrap();
        let text = "a1 b2";
        assert_eq!(m.match_all(text, 10).unwrap(), vec![0..2, 3..5]);
        assert_eq!(m.replacement(text, &(0..2)), "<1a|a1|$|a2|$9>");
        // Zero-length matches are skipped.
        let mut z = q("x*");
        z.regexp = true;
        assert_eq!(
            z.matcher().unwrap().match_all("axxb", 10).unwrap(),
            vec![1..3]
        );
        assert!(m.match_all(text, 1).is_none());
    }

    #[test]
    fn next_and_prev_wrap() {
        let text = "ab ab ab";
        let m = q("ab").matcher().unwrap();
        assert_eq!(m.next_match(text, 0, 2), Some(3..5));
        assert_eq!(m.next_match(text, 6, 8), Some(0..2));
        assert_eq!(m.prev_match(text, 3, 5), Some(0..2));
        assert_eq!(m.prev_match(text, 0, 2), Some(6..8));
        // A single match is "next" of itself only when not selected.
        let one = q("zz").matcher().unwrap();
        assert_eq!(one.next_match("a zz", 2, 4), None);
        assert_eq!(one.next_match("a zz", 4, 4), Some(2..4));
        assert_eq!(one.prev_match("a zz", 2, 4), None);
    }

    #[test]
    fn default_query_rules() {
        assert_eq!(default_search(""), None);
        assert_eq!(default_search("a\nb").as_deref(), Some("a\\nb"));
        assert_eq!(default_search(&"x".repeat(101)), None);
    }

    #[test]
    fn selection_match_rules() {
        let text = "cat cat cat";
        assert_eq!(
            selection_matches(text, &[0..3], 0..text.len()),
            vec![4..7, 8..11]
        );
        assert!(selection_matches(text, &[0..3, 4..7], 0..text.len()).is_empty());
        assert!(selection_matches(text, &[2..2], 0..text.len()).is_empty());
        // Only the visible part, and partial words too (wholeWords: false).
        assert_eq!(selection_matches("ab abc", &[0..2], 3..6), vec![3..5]);
        // More than 100 matches: nothing.
        let many = "a ".repeat(150);
        assert!(selection_matches(&many, &[0..1], 0..many.len()).is_empty());
        let many = "a ".repeat(100);
        assert_eq!(selection_matches(&many, &[0..1], 0..many.len()).len(), 99);
    }

    #[test]
    fn next_occurrence() {
        let text = "foo bar foo foobar foo";
        // Empty range: select the word.
        assert_eq!(
            select_next_occurrence(text, &[1..1], &[1], 0),
            Some(NextOccurrence::Words(vec![0..3]))
        );
        // Full word: only whole-word occurrences.
        assert_eq!(
            select_next_occurrence(text, &[0..3], &[3], 0),
            Some(NextOccurrence::Add(8..11))
        );
        assert_eq!(
            select_next_occurrence(text, &[0..3, 8..11], &[3, 11], 1),
            Some(NextOccurrence::Add(19..22))
        );
        // Wraps around, skipping existing ranges.
        assert_eq!(
            select_next_occurrence(text, &[8..11, 19..22], &[11, 22], 0),
            Some(NextOccurrence::Add(0..3))
        );
        assert_eq!(
            select_next_occurrence(text, &[0..3, 8..11, 19..22], &[3, 11, 22], 0),
            None
        );
        // Partial word: any occurrence.
        assert_eq!(
            select_next_occurrence(text, &[0..2], &[2], 0),
            Some(NextOccurrence::Add(8..10))
        );
        // Different texts: nothing.
        assert_eq!(
            select_next_occurrence(text, &[0..3, 4..7], &[3, 7], 0),
            None
        );
    }

    #[test]
    fn selection_matches_command() {
        let text = "x y x y x";
        assert_eq!(
            select_selection_matches(text, &[4..5]),
            Some((vec![0..1, 4..5, 8..9], 1))
        );
        assert_eq!(select_selection_matches(text, &[4..4]), None);
        assert_eq!(select_selection_matches(text, &[0..1, 4..5]), None);
    }

    #[test]
    fn goto_parsing() {
        assert_eq!(parse_goto("7", 1, 10), Some((7, 0)));
        assert_eq!(parse_goto("99", 1, 10), Some((10, 0)));
        assert_eq!(parse_goto("0", 5, 10), Some((1, 0)));
        assert_eq!(parse_goto("+2", 5, 10), Some((7, 0)));
        assert_eq!(parse_goto("-9", 5, 10), Some((1, 0)));
        assert_eq!(parse_goto("50%", 1, 10), Some((5, 0)));
        assert_eq!(parse_goto("+10%", 5, 10), Some((6, 0)));
        assert_eq!(parse_goto("3:4", 1, 10), Some((3, 4)));
        assert_eq!(parse_goto(":4", 6, 10), Some((6, 4)));
        assert_eq!(parse_goto("", 6, 10), Some((6, 0)));
        assert_eq!(parse_goto("abc", 6, 10), None);
    }
}
