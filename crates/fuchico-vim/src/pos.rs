//! CodeMirror 5 style positions (`{line, ch}`, both 0-based).
//!
//! `ch` is a char index into the line (cm-vim uses UTF-16 code units; see
//! COMMANDS.md). Positions may be out of range (`ch == INF` means "end of
//! line", negative values happen in a few motions); they are clipped by the
//! adapter exactly where CodeMirror clips them.

/// JavaScript `Infinity` / `Number.MAX_VALUE` for `ch`.
pub const INF: i64 = i64::MAX / 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub line: i64,
    pub ch: i64,
}

impl Pos {
    pub const fn new(line: i64, ch: i64) -> Pos {
        Pos { line, ch }
    }
}

/// cm-vim `cursorIsBefore`.
pub fn before(a: Pos, b: Pos) -> bool {
    a.line < b.line || (a.line == b.line && a.ch < b.ch)
}

/// cm-vim `cursorMin` (returns the second argument on ties).
pub fn min(a: Pos, b: Pos) -> Pos {
    if before(a, b) { a } else { b }
}

/// cm-vim `cursorMax` (returns the first argument on ties).
pub fn max(a: Pos, b: Pos) -> Pos {
    if before(a, b) { b } else { a }
}

/// Variadic `cursorMin`.
pub fn min_of(ps: &[Pos]) -> Pos {
    let mut it = ps.iter().rev();
    let mut cur = *it.next().expect("non-empty");
    for p in it {
        cur = min(*p, cur);
    }
    cur
}

/// Variadic `cursorMax`.
#[allow(dead_code)]
pub fn max_of(ps: &[Pos]) -> Pos {
    let mut it = ps.iter().rev();
    let mut cur = *it.next().expect("non-empty");
    for p in it {
        cur = max(*p, cur);
    }
    cur
}

/// cm-vim `cursorIsBetween` (strict on both sides).
pub fn between(a: Pos, b: Pos, c: Pos) -> bool {
    before(a, b) && before(b, c)
}

/// cm-vim `offsetCursor`.
pub fn offset(p: Pos, dl: i64, dch: i64) -> Pos {
    Pos::new(p.line.saturating_add(dl), p.ch.saturating_add(dch))
}

// ---- JS string helpers over char slices ---------------------------------

/// `line.charAt(i)` (`None` = `""`).
pub fn char_at(line: &[char], i: i64) -> Option<char> {
    if i < 0 {
        None
    } else {
        line.get(i as usize).copied()
    }
}

/// `/\s/.test(c)` (JS whitespace).
pub fn is_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `/\s/.test(line.charAt(i))`.
#[allow(dead_code)]
pub fn ws_at(line: &[char], i: i64) -> bool {
    char_at(line, i).is_some_and(is_ws)
}

/// CodeMirror `isWordChar` (`[\w\p{Alphabetic}\p{Number}_]`).
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `/^\s*$/.test(s)`.
pub fn is_ws_string(s: &str) -> bool {
    s.chars().all(is_ws)
}

/// `line.indexOf(c, from)`; JS clamps a negative `from` to 0.
pub fn index_of(line: &[char], c: char, from: i64) -> i64 {
    let from = from.max(0) as usize;
    if from > line.len() {
        return -1;
    }
    line[from..]
        .iter()
        .position(|&x| x == c)
        .map(|i| (i + from) as i64)
        .unwrap_or(-1)
}

/// `line.lastIndexOf(c, from)`; a negative `from` behaves like 0.
pub fn last_index_of(line: &[char], c: char, from: i64) -> i64 {
    if line.is_empty() {
        return -1;
    }
    let from = from.clamp(0, line.len() as i64 - 1) as usize;
    (0..=from)
        .rev()
        .find(|&i| line[i] == c)
        .map(|i| i as i64)
        .unwrap_or(-1)
}

/// `findFirstNonWhiteSpaceCharacter`.
pub fn first_non_ws(text: &str) -> i64 {
    let mut n = 0;
    for c in text.chars() {
        if !is_ws(c) {
            return n;
        }
        n += 1;
    }
    n
}

pub fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

pub fn clen(s: &str) -> i64 {
    s.chars().count() as i64
}

/// `s.substring(a, b)` on chars, clamped.
pub fn substr(s: &str, a: i64, b: i64) -> String {
    let n = clen(s);
    let a = a.clamp(0, n);
    let b = b.clamp(0, n);
    let (a, b) = if a > b { (b, a) } else { (a, b) };
    s.chars().skip(a as usize).take((b - a) as usize).collect()
}

/// cm-vim `isUpperCase` (`\p{Lu}`).
pub fn is_upper(c: char) -> bool {
    c.is_uppercase()
}

/// cm-vim `isLowerCase` (`[a-z]`).
pub fn is_lower_ascii(c: char) -> bool {
    c.is_ascii_lowercase()
}

/// `/^\w$/` (ASCII word char).
pub fn is_latin_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
