//! Port of `extractOutline` from `src/modules/editor/lib/outline.ts`,
//! operating on a document string.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineHeader {
    pub level: usize,
    pub text: String,
    /// 1-based line number.
    pub line: usize,
    /// Byte offset of the line start.
    pub from: usize,
}

static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").unwrap());

pub fn extract_outline(doc: &str) -> Vec<OutlineHeader> {
    let mut headers = Vec::new();
    let mut from = 0;
    for (i, raw) in doc.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(m) = HEADER_RE.captures(line) {
            headers.push(OutlineHeader { level: m[1].len(), text: m[2].to_string(), line: i + 1, from });
        }
        from += raw.len() + 1;
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_headers_with_positions() {
        let doc = "# One\ntext\n## Two  \n#nospace\n####### seven\n";
        let h = extract_outline(doc);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0], OutlineHeader { level: 1, text: "One".into(), line: 1, from: 0 });
        assert_eq!(h[1], OutlineHeader { level: 2, text: "Two".into(), line: 3, from: 11 });
    }
}
