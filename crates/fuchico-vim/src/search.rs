//! Search: JavaScript regular expressions (cm-vim queries are `RegExp`
//! objects with the `m` flag), `translateRegex`, `parseQuery` and the
//! adapter's `getSearchCursor` over a CodeMirror 6 `RegExpCursor`.

use fancy_regex::Regex;
use fuchico_helix::Text;

/// A compiled search query (`RegExp` with flags `m` or `im`).
#[derive(Clone)]
pub struct Query {
    /// The JavaScript source.
    pub source: String,
    pub ignore_case: bool,
    re: Regex,
}

impl std::fmt::Debug for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "/{}/{}", self.source, if self.ignore_case { "im" } else { "m" })
    }
}

impl PartialEq for Query {
    fn eq(&self, o: &Query) -> bool {
        self.source == o.source && self.ignore_case == o.ignore_case
    }
}

/// One match: char offsets and capture groups (group 0 = whole match).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub from: usize,
    pub to: usize,
    pub groups: Vec<Option<String>>,
}

impl Query {
    /// `new RegExp(source, ignoreCase ? 'im' : 'm')`. `Err` carries the
    /// message cm-vim shows (`Invalid regex: ...`).
    pub fn new(source: &str, ignore_case: bool) -> Result<Query, String> {
        let rust = js_to_rust(source, ignore_case);
        let re = Regex::new(&rust).map_err(|e| e.to_string())?;
        Ok(Query {
            source: source.to_string(),
            ignore_case,
            re,
        })
    }

    /// A literal (escaped) query.
    pub fn literal(text: &str, ignore_case: bool) -> Query {
        Query::new(&escape_regex_js(text), ignore_case).expect("escaped literal")
    }

    /// `regex.test(s)`.
    pub fn test(&self, s: &str) -> bool {
        self.re.is_match(s).unwrap_or(false)
    }

    /// `s.match(regex)` / `regex.exec(s)`: first match in a string (byte
    /// offsets are converted to char offsets).
    pub fn exec(&self, s: &str) -> Option<Match> {
        self.exec_at(s, 0)
    }

    /// Like `exec` starting at char `from` (`lastIndex`).
    pub fn exec_at(&self, s: &str, from: usize) -> Option<Match> {
        let start = char_to_byte_str(s, from)?;
        let caps = self.re.captures_from_pos(s, start).ok()??;
        let m = caps.get(0)?;
        Some(Match {
            from: s[..m.start()].chars().count(),
            to: s[..m.end()].chars().count(),
            groups: (0..caps.len())
                .map(|i| caps.get(i).map(|g| g.as_str().to_string()))
                .collect(),
        })
    }

    /// Every non-overlapping match of the string (`RegExpCursor` order;
    /// an empty match advances one char).
    pub fn all_in_str(&self, s: &str) -> Vec<Match> {
        let n = s.chars().count();
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= n {
            let Some(m) = self.exec_at(s, pos) else { break };
            pos = if m.to == m.from { m.to + 1 } else { m.to };
            out.push(m);
        }
        out
    }

    /// The first match in the document starting at or after `from` and
    /// ending at or before `to` (`RegExpCursor(doc, q, {}, from, to)`).
    pub fn next_in(&self, doc: &Text, from: usize, to: usize) -> Option<Match> {
        let s = doc.to_string();
        self.next_in_str(&s, from, to)
    }

    fn next_in_str(&self, s: &str, from: usize, to: usize) -> Option<Match> {
        let mut pos = from;
        let n = s.chars().count();
        while pos <= n {
            let m = self.exec_at(s, pos)?;
            if m.to <= to {
                return Some(m);
            }
            if m.from >= to {
                return None;
            }
            // a match running past `to`: CodeMirror's cursor clips the
            // haystack, try the next start
            pos = m.from + 1;
        }
        None
    }

    /// All matches inside `from..to` (search highlight).
    pub fn matches(&self, doc: &Text, from: usize, to: usize) -> Vec<(usize, usize)> {
        let s = doc.to_string();
        let mut out = Vec::new();
        let mut pos = from;
        while let Some(m) = self.next_in_str(&s, pos, to) {
            pos = if m.to == m.from { m.to + 1 } else { m.to };
            if m.to > m.from {
                out.push((m.from, m.to));
            }
            if pos > to {
                break;
            }
        }
        out
    }
}

fn char_to_byte_str(s: &str, c: usize) -> Option<usize> {
    if c == 0 {
        return Some(0);
    }
    match s.char_indices().nth(c) {
        Some((i, _)) => Some(i),
        None if s.chars().count() == c => Some(s.len()),
        None => None,
    }
}

/// `escapeRegex` (cm-vim).
pub fn escape_regex_js(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if ".?*+$[]/\\(){}|-^".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Translate JavaScript regex syntax (with the `m` and, when given, `i`
/// flags) to `fancy_regex` syntax. `\w`, `\d`, `\b` are ASCII like in JS;
/// lone `{` / `}` are literals (cm-vim's search cursor escapes them).
pub fn js_to_rust(src: &str, ignore_case: bool) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::from(if ignore_case { "(?mi)" } else { "(?m)" });
    let mut i = 0;
    let mut in_class = false;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && i + 1 < c.len() {
            let n = c[i + 1];
            i += 2;
            match n {
                'w' => out.push_str(if in_class { "0-9A-Za-z_" } else { "[0-9A-Za-z_]" }),
                'W' => out.push_str(if in_class { "\\W" } else { "[^0-9A-Za-z_]" }),
                'd' => out.push_str(if in_class { "0-9" } else { "[0-9]" }),
                'D' => out.push_str(if in_class { "\\D" } else { "[^0-9]" }),
                'b' if !in_class => out.push_str("(?:(?<=[0-9A-Za-z_])(?![0-9A-Za-z_])|(?<![0-9A-Za-z_])(?=[0-9A-Za-z_]))"),
                'B' if !in_class => out.push_str("(?:(?<=[0-9A-Za-z_])(?=[0-9A-Za-z_])|(?<![0-9A-Za-z_])(?![0-9A-Za-z_]))"),
                'b' => out.push_str("\\x08"),
                's' | 'S' | 'n' | 'r' | 't' | 'f' | 'v' => {
                    out.push('\\');
                    out.push(n);
                }
                '0' => out.push_str("\\x00"),
                'c' if i < c.len() && c[i].is_ascii_alphabetic() => {
                    out.push_str(&format!("\\x{:02x}", (c[i] as u32) % 32));
                    i += 1;
                }
                'u' if i + 4 <= c.len() && c[i..i + 4].iter().all(|x| x.is_ascii_hexdigit()) => {
                    let h: String = c[i..i + 4].iter().collect();
                    out.push_str(&format!("\\x{{{h}}}"));
                    i += 4;
                }
                'u' if i < c.len() && c[i] == '{' => {
                    let end = c[i..].iter().position(|&x| x == '}').map(|p| p + i);
                    if let Some(e) = end {
                        let h: String = c[i + 1..e].iter().collect();
                        out.push_str(&format!("\\x{{{h}}}"));
                        i = e + 1;
                    } else {
                        out.push('u');
                    }
                }
                'x' if i + 2 <= c.len() && c[i..i + 2].iter().all(|x| x.is_ascii_hexdigit()) => {
                    let h: String = c[i..i + 2].iter().collect();
                    out.push_str(&format!("\\x{h}"));
                    i += 2;
                }
                '1'..='9' if !in_class => {
                    out.push('\\');
                    out.push(n);
                    while i < c.len() && c[i].is_ascii_digit() {
                        out.push(c[i]);
                        i += 1;
                    }
                }
                'k' if i < c.len() && c[i] == '<' => {
                    let end = c[i..].iter().position(|&x| x == '>').map(|p| p + i);
                    if let Some(e) = end {
                        let name: String = c[i + 1..e].iter().collect();
                        out.push_str(&format!("\\k<{name}>"));
                        i = e + 1;
                    }
                }
                x if x.is_ascii_alphanumeric() => out.push(x),
                x => push_literal(&mut out, x),
            }
            continue;
        }
        i += 1;
        if in_class {
            match ch {
                ']' => {
                    in_class = false;
                    out.push(']');
                }
                '[' | '&' | '~' | '-' if ch != '-' => {
                    out.push('\\');
                    out.push(ch);
                }
                _ => out.push(ch),
            }
            continue;
        }
        match ch {
            '[' => {
                if c.get(i) == Some(&'^') && c.get(i + 1) == Some(&']') {
                    out.push_str("[\\s\\S]");
                    i += 2;
                } else if c.get(i) == Some(&']') {
                    // `[]` never matches
                    out.push_str("[^\\s\\S]");
                    i += 1;
                } else {
                    in_class = true;
                    out.push('[');
                    if c.get(i) == Some(&'^') {
                        out.push('^');
                        i += 1;
                    }
                }
            }
            '{' => {
                // a quantifier `{n}`, `{n,}`, `{n,m}` (or `{,m}`) stays
                let rest: String = c[i..].iter().collect();
                let q = rest.find('}').map(|e| &rest[..e]);
                let is_q = q.is_some_and(|q| {
                    let parts: Vec<&str> = q.splitn(2, ',').collect();
                    !q.is_empty()
                        && parts.iter().all(|p| p.chars().all(|x| x.is_ascii_digit()))
                        && !(parts[0].is_empty() && parts.len() == 1)
                        && !parts[0].is_empty()
                });
                if is_q {
                    out.push('{');
                } else {
                    out.push_str("\\{");
                }
            }
            '}' => {
                // closing a quantifier we kept?
                let open = out.rfind('{');
                let close = out.rfind('}');
                let kept = open.is_some_and(|o| {
                    o > 0 && !out[..o].ends_with('\\') && close.is_none_or(|cl| cl < o)
                });
                if kept {
                    out.push('}');
                } else {
                    out.push_str("\\}");
                }
            }
            '$' => out.push_str("(?:$)"),
            _ => out.push(ch),
        }
    }
    out
}

fn push_literal(out: &mut String, x: char) {
    if "\\.+*?()|[]{}^$#&-~/ ".contains(x) {
        if x == '/' || x == ' ' {
            out.push(x);
        } else {
            out.push('\\');
            out.push(x);
        }
    } else {
        out.push(x);
    }
}

// ---- cm-vim helpers -----------------------------------------------------

/// `findUnescapedSeparators`.
pub fn find_unescaped_separators(s: &str, sep: char) -> Vec<usize> {
    let mut escape_next = false;
    let mut out = Vec::new();
    for (i, c) in s.chars().enumerate() {
        if !escape_next && c == sep {
            out.push(i);
        }
        escape_next = !escape_next && c == '\\';
    }
    out
}

/// `splitBySeparator`: `None` = `undefined` (does not start with the
/// separator), `Some(vec![])` = no separators.
pub fn split_by_separator(s: &str, sep: char) -> Option<Vec<String>> {
    let slashes = find_unescaped_separators(s, sep);
    if slashes.is_empty() {
        return Some(Vec::new());
    }
    if slashes[0] != 0 {
        return None;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut tokens = Vec::new();
    for i in 0..slashes.len() {
        let a = slashes[i] + 1;
        let b = slashes.get(i + 1).copied().unwrap_or(chars.len());
        tokens.push(chars[a.min(chars.len())..b].iter().collect());
    }
    Some(tokens)
}

/// `translateRegex` (Vim magic syntax to JavaScript; only used with
/// `:set nopcre`).
pub fn translate_regex(s: &str) -> String {
    let modes = |c: char| -> Option<&'static str> {
        match c {
            'V' => Some("|(){+?*.[$^"),
            'M' => Some("|(){+?*.["),
            'm' => Some("|(){+?"),
            'v' => Some("<>"),
            _ => None,
        }
    };
    let escapes = |c: char| -> Option<&'static str> {
        match c {
            '>' => Some("(?<=[\\w])(?=[^\\w]|$)"),
            '<' => Some("(?<=[^\\w]|^)(?=[\\w])"),
            _ => None,
        }
    };
    let mut specials = "|(){+?";
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            let n = c[i + 1];
            i += 2;
            if n == '}' || specials.contains(n) {
                out.push(n);
            } else if let Some(m) = modes(n) {
                specials = m;
            } else if let Some(e) = escapes(n) {
                out.push_str(e);
            } else {
                out.push('\\');
                out.push(n);
            }
            continue;
        }
        let ch = c[i];
        i += 1;
        if "[|(){+*?.$^<>".contains(ch) && specials.contains(ch) {
            match escapes(ch) {
                Some(e) => out.push_str(e),
                None => {
                    out.push('\\');
                    out.push(ch);
                }
            }
        } else {
            out.push(ch);
        }
    }
    if let Some(i) = out.find("\\zs") {
        out = format!("(?<={}){}", &out[..i], &out[i + 3..]);
    }
    if let Some(i) = out.find("\\ze") {
        out = format!("{}(?={})", &out[..i], &out[i + 3..]);
    }
    out
}

/// `translateRegexReplace` (Vim replacement to JS `$1` syntax).
pub fn translate_regex_replace(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let at = |i: i64| -> Option<char> {
        if i < 0 { None } else { c.get(i as usize).copied() }
    };
    let mut escape_next = false;
    let mut out = String::new();
    let mut i: i64 = -1;
    while i < c.len() as i64 {
        let ch = at(i);
        let n = at(i + 1);
        let pair: String = ch.into_iter().chain(n).collect();
        let un = match pair.as_str() {
            "\\n" => Some('\n'),
            "\\r" => Some('\r'),
            "\\t" => Some('\t'),
            _ => None,
        };
        if let Some(u) = un {
            out.push(u);
            i += 2;
            continue;
        }
        if escape_next {
            if let Some(ch) = ch {
                out.push(ch);
            }
            escape_next = false;
        } else if ch == Some('\\') {
            escape_next = true;
            if n.is_some_and(|n| n.is_ascii_digit()) || n == Some('$') {
                out.push('$');
            } else if n != Some('/') && n != Some('\\') {
                out.push('\\');
            }
        } else {
            if ch == Some('$') {
                out.push('$');
            }
            if let Some(ch) = ch {
                out.push(ch);
            }
            if n == Some('/') {
                out.push('\\');
            }
        }
        i += 1;
    }
    out
}

/// `unescapeRegexReplace` (PCRE mode).
pub fn unescape_regex_replace(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '\\' {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let n = c.get(i + 1).copied();
        let rep = match n {
            Some('/') => Some('/'),
            Some('\\') => Some('\\'),
            Some('n') => Some('\n'),
            Some('r') => Some('\r'),
            Some('t') => Some('\t'),
            Some('&') => Some('&'),
            _ => None,
        };
        match rep {
            Some(r) => {
                out.push(r);
                i += 2;
            }
            None => {
                out.push('\\');
                i += 1;
            }
        }
    }
    out
}

/// JavaScript `String.prototype.replace` replacement patterns (`$&`, `$1`,
/// `$$`, `` $` ``, `$'`).
#[allow(dead_code)]
pub fn expand_replacement(rep: &str, m: &Match, subject: &str) -> String {
    let c: Vec<char> = rep.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' && i + 1 < c.len() {
            let n = c[i + 1];
            match n {
                '$' => {
                    out.push('$');
                    i += 2;
                    continue;
                }
                '&' => {
                    out.push_str(m.groups[0].as_deref().unwrap_or(""));
                    i += 2;
                    continue;
                }
                '`' => {
                    out.extend(subject.chars().take(m.from));
                    i += 2;
                    continue;
                }
                '\'' => {
                    out.extend(subject.chars().skip(m.to));
                    i += 2;
                    continue;
                }
                d if d.is_ascii_digit() => {
                    // two-digit group if it exists
                    if i + 2 < c.len() && c[i + 2].is_ascii_digit() {
                        let g = (d as usize - '0' as usize) * 10 + (c[i + 2] as usize - '0' as usize);
                        if g > 0 && g < m.groups.len() {
                            out.push_str(m.groups[g].as_deref().unwrap_or(""));
                            i += 3;
                            continue;
                        }
                    }
                    let g = d as usize - '0' as usize;
                    if g > 0 && g < m.groups.len() {
                        out.push_str(m.groups[g].as_deref().unwrap_or(""));
                        i += 2;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// The search cursor `cm.getSearchCursor(query, pos)` returns.
#[derive(Debug, Clone)]
pub struct SearchCursor {
    pub query: Query,
    first_offset: usize,
    pub last: Option<Match>,
    after_empty: bool,
}

const CHUNK: usize = 10000;

impl SearchCursor {
    /// `pos` is a char offset (`ch == undefined` is the caller's job).
    pub fn new(query: &Query, first_offset: usize) -> SearchCursor {
        // lone `{`/`}` are escaped by `js_to_rust`
        SearchCursor {
            query: query.clone(),
            first_offset,
            last: None,
            after_empty: false,
        }
    }

    fn prev_in_range(&self, s: &str, from: usize, to: usize) -> Option<Match> {
        let mut size = 1;
        loop {
            let start = from.max(to.saturating_sub(size * CHUNK));
            let mut range = None;
            let mut pos = start;
            while let Some(m) = self.query.next_in_str(s, pos, to) {
                pos = if m.to == m.from { m.to + 1 } else { m.to };
                range = Some(m);
                if pos > to {
                    break;
                }
            }
            if let Some(r) = &range
                && (start == from || r.from > start + 10)
            {
                return range;
            }
            if start == from {
                return None;
            }
            size += 1;
        }
    }

    /// `find(back)`: the matched text (`Some("")` for an empty match).
    pub fn find(&mut self, doc: &Text, back: bool) -> Option<String> {
        let s = doc.to_string();
        let len = doc.len();
        if back {
            let end_at = match &self.last {
                Some(l) => {
                    if self.after_empty {
                        l.to as i64 - 1
                    } else {
                        l.from as i64
                    }
                }
                None => self.first_offset as i64,
            };
            self.last = if end_at < 0 {
                None
            } else {
                self.prev_in_range(&s, 0, end_at as usize)
            };
        } else {
            let start = match &self.last {
                Some(l) => {
                    if self.after_empty {
                        l.to + 1
                    } else {
                        l.to
                    }
                }
                None => self.first_offset,
            };
            self.last = if start > len {
                None
            } else {
                self.query.next_in_str(&s, start, len)
            };
        }
        self.after_empty = self.last.as_ref().is_some_and(|l| l.from == l.to);
        self.last.as_ref().map(|l| l.groups[0].clone().unwrap_or_default())
    }

    pub fn from(&self) -> Option<usize> {
        self.last.as_ref().map(|l| l.from)
    }
    pub fn to(&self) -> Option<usize> {
        self.last.as_ref().map(|l| l.to)
    }
    /// After `replace(text)`: `last.to = last.from + text.length`.
    pub fn set_last_to(&mut self, to: usize) {
        if let Some(l) = &mut self.last {
            l.to = to;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_regex() {
        let q = Query::new("a{2}", false).unwrap();
        assert!(q.test("xaay"));
        let q = Query::new("{", false).unwrap();
        assert!(q.test("a{b"));
        let q = Query::new("\\bfoo\\b", false).unwrap();
        assert!(q.test("a foo b"));
        assert!(!q.test("afoob"));
        let q = Query::new("^b$", false).unwrap();
        assert!(q.test("a\nb\nc"));
        let q = Query::new("\\/", false).unwrap();
        assert!(q.test("a/b"));
        let q = Query::new("[a-c]+", true).unwrap();
        assert_eq!(q.exec("xxBCa").map(|m| (m.from, m.to)), Some((2, 5)));
    }

    #[test]
    fn replace_patterns() {
        assert_eq!(translate_regex_replace("\\1x"), "$1x");
        assert_eq!(translate_regex_replace("a$b"), "a$$b");
        let q = Query::new("(b)", false).unwrap();
        let m = q.exec("abc").unwrap();
        assert_eq!(expand_replacement("[$1$&]", &m, "abc"), "[bb]");
    }

    #[test]
    fn cursor() {
        let doc = Text::new("foo bar foo\nfoo");
        let q = Query::new("foo", false).unwrap();
        let mut c = SearchCursor::new(&q, 1);
        assert!(c.find(&doc, false).is_some());
        assert_eq!(c.from(), Some(8));
        assert!(c.find(&doc, false).is_some());
        assert_eq!(c.from(), Some(12));
        assert!(c.find(&doc, false).is_none());
        let mut c = SearchCursor::new(&q, 8);
        assert!(c.find(&doc, true).is_some());
        assert_eq!(c.from(), Some(0));
    }
}
