//! Regex search (cm-helix `searchQuery` on top of CodeMirror's
//! `RegExpCursor`): multiline (`^`/`$` match at line boundaries), case
//! sensitive iff the query contains an ASCII uppercase letter.
//!
//! cm-helix compiles the pattern as a JavaScript `RegExp`, so the engine uses
//! `fancy-regex`: on top of the `regex` crate syntax it supports look-ahead,
//! look-behind and back-references, as JS does.

use fancy_regex::Regex;

use crate::text::Text;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub source: String,
    regex: Result<Regex, String>,
}

impl SearchQuery {
    pub fn new(source: &str) -> Self {
        let regex = if source.is_empty() {
            Err("empty query".to_string())
        } else {
            let case_sensitive = source.chars().any(|c| c.is_ascii_uppercase());
            let flags = if case_sensitive { "(?m)" } else { "(?mi)" };
            Regex::new(&format!("{flags}{source}")).map_err(|e| {
                // Keep the human part of the regex crate's error.
                let msg = e.to_string();
                msg.lines()
                    .rev()
                    .find(|l| l.starts_with("error:"))
                    .map(|l| l.trim_start_matches("error:").trim().to_string())
                    .unwrap_or(msg)
            })
        };
        SearchQuery {
            source: source.to_string(),
            regex,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.regex.is_ok()
    }

    pub fn error(&self) -> Option<&str> {
        self.regex.as_ref().err().map(String::as_str)
    }

    /// All matches within `from..to` (char offsets), in document order.
    /// Empty matches are only reported after the end of the previous match
    /// (CodeMirror `RegExpCursor`).
    pub fn matches(&self, doc: &Text, from: usize, to: usize) -> Vec<(usize, usize)> {
        let Ok(re) = &self.regex else {
            return Vec::new();
        };
        let to = to.min(doc.len());
        let from = from.min(to);
        let hay = doc.slice(0, to);
        let mut out = Vec::new();
        let mut byte = doc.char_to_byte(from);
        let mut prev_to = 0usize;
        while byte <= hay.len() {
            // A runtime error (backtrack limit) ends the scan like "no match".
            let Ok(Some(m)) = re.find_from_pos(&hay, byte) else {
                break;
            };
            let mf = doc.byte_to_char(m.start());
            let mt = doc.byte_to_char(m.end());
            if mf < mt || mf > prev_to {
                out.push((mf, mt));
                prev_to = mt;
            }
            if m.end() == m.start() {
                // step over one char
                match hay[m.end()..].chars().next() {
                    Some(c) => byte = m.end() + c.len_utf8(),
                    None => break,
                }
            } else {
                byte = m.end();
            }
        }
        out
    }

    pub fn first_match(&self, doc: &Text, from: usize) -> Option<(usize, usize)> {
        self.matches(doc, from, doc.len()).into_iter().next()
    }
}

/// cm-helix `escapeRegex` (used by `*`).
pub fn escape_regex(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '|' | '\\' | '{' | '}' | '(' | ')' | '[' | ']' | '^' | '$' | '+' | '*' | '?' | '.' => {
                out.push('\\');
                out.push(c);
            }
            '-' => out.push_str("\\x2d"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(pattern: &str, doc: &str) -> Vec<(usize, usize)> {
        let t = Text::new(doc);
        SearchQuery::new(pattern).matches(&t, 0, t.len())
    }

    #[test]
    fn js_lookaround_and_backreferences_work() {
        // Look-ahead / negative look-ahead.
        assert_eq!(all(r"foo(?=bar)", "foobar foobaz"), vec![(0, 3)]);
        assert_eq!(all(r"foo(?!bar)", "foobar foobaz"), vec![(7, 10)]);
        // Look-behind / negative look-behind.
        assert_eq!(all(r"(?<=\$)\d+", "a $12 b 34"), vec![(3, 5)]);
        assert_eq!(all(r"(?<!\$)\b\d+", "a $12 b 34"), vec![(8, 10)]);
        // Back-reference.
        assert_eq!(all(r"(\w)\1", "abba cd"), vec![(1, 3)]);
    }

    #[test]
    fn lookbehind_sees_text_before_the_scan_start() {
        let t = Text::new("xab ab");
        let q = SearchQuery::new(r"(?<=x)ab");
        assert_eq!(q.matches(&t, 1, t.len()), vec![(1, 3)]);
    }

    #[test]
    fn multiline_smart_case_and_char_offsets_still_hold() {
        assert_eq!(all(r"^b", "a\nb\nB"), vec![(2, 3), (4, 5)]);
        assert_eq!(all(r"^B", "a\nb\nB"), vec![(4, 5)]);
        assert_eq!(all(r"é(?=x)", "éx éy"), vec![(0, 1)]);
        assert!(!SearchQuery::new("a(").is_valid());
    }
}
