//! Regex search (cm-helix `searchQuery` on top of CodeMirror's
//! `RegExpCursor`): multiline (`^`/`$` match at line boundaries), case
//! sensitive iff the query contains an ASCII uppercase letter.
//!
//! The pattern is compiled with the Rust `regex` crate, so JavaScript-only
//! syntax (look-around, back-references) is reported as an invalid regex.

use regex::Regex;

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
            let Some(m) = re.find_at(&hay, byte) else {
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
