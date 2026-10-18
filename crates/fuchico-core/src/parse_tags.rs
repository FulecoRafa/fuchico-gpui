//! Port of `src/modules/tags/lib/parseTags.ts` (inline `#tag` + frontmatter
//! `tags:`). Offsets are **byte** offsets into the content (the TS version
//! used UTF-16 units).

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::frontmatter::{Value, parse_frontmatter};

#[derive(Debug, Clone, PartialEq)]
pub struct InlineTagMatch {
    /// Normalized tag text, without the leading `#`.
    pub tag: String,
    /// Offset of the `#` character.
    pub from: usize,
    /// Offset just past the last character of the tag.
    pub to: usize,
}

// JS used `(?<=^|\s)#(...)`; Rust's regex has no lookbehind, so the
// preceding-char check is done by hand in `match_inline_tags_in_line`.
static INLINE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#(\p{L}[\p{L}\p{N}_/-]*)").unwrap());
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)(`{3,}|~{3,})").unwrap());

/// Emulates `/(`+)([^`]*?)\1/g` replaced by spaces of the same length
/// (backreferences are unavailable in Rust's regex), including the engine's
/// backtracking to a shorter opening run.
fn mask_inline_code(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let mut run = 0;
        while i + run < b.len() && b[i + run] == b'`' {
            run += 1;
        }
        let mut matched_end = None;
        for k in (1..=run).rev() {
            let mut p = i + k;
            while p < b.len() && b[p] != b'`' {
                p += 1;
            }
            if p + k <= b.len() && b[p..p + k].iter().all(|&c| c == b'`') {
                matched_end = Some(p + k);
                break;
            }
        }
        match matched_end {
            Some(end) => {
                for c in &mut out[i..end] {
                    *c = b' ';
                }
                i = end;
            }
            None => i += 1,
        }
    }
    // Only ASCII backticks were replaced by ASCII spaces, and non-ASCII
    // bytes inside a masked span were replaced too: replace whole spans, so
    // this is valid UTF-8 as long as span edges are char boundaries (they
    // are: backtick bytes).
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

/// Finds every inline `#tag` in a single line. Positions relative to `line`.
pub fn match_inline_tags_in_line(line: &str) -> Vec<InlineTagMatch> {
    let masked = mask_inline_code(line);
    let mut matches = Vec::new();
    for m in INLINE_TAG_RE.captures_iter(&masked) {
        let whole = m.get(0).unwrap();
        let from = whole.start();
        let ok = masked[..from]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace());
        if !ok {
            continue;
        }
        let tag = m[1].trim_end_matches(['/', '_', '-']);
        if tag.is_empty() {
            continue;
        }
        matches.push(InlineTagMatch {
            tag: tag.to_string(),
            from,
            to: from + 1 + tag.len(),
        });
    }
    matches
}

/// Scans a whole note for inline tags, skipping fenced code blocks and
/// inline code spans.
pub fn parse_inline_tags(content: &str) -> Vec<InlineTagMatch> {
    let mut results = Vec::new();
    let mut offset = 0;
    let mut in_fence = false;
    let mut fence_marker = ' ';

    for line in content.split('\n') {
        if let Some(c) = FENCE_RE.captures(line) {
            let marker = c[2].chars().next().unwrap();
            if !in_fence {
                in_fence = true;
                fence_marker = marker;
            } else if marker == fence_marker {
                in_fence = false;
            }
            offset += line.len() + 1;
            continue;
        }
        if !in_fence {
            for m in match_inline_tags_in_line(line) {
                results.push(InlineTagMatch {
                    tag: m.tag,
                    from: offset + m.from,
                    to: offset + m.to,
                });
            }
        }
        offset += line.len() + 1;
    }
    results
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.clone(),
        Value::List(items) => items
            .iter()
            .map(value_to_string)
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// Reads `tags:` from a note's frontmatter (list or scalar forms).
pub fn frontmatter_tags(content: &str) -> Vec<String> {
    let Some(fm) = parse_frontmatter(content) else {
        return vec![];
    };
    let Some(raw) = fm.data.get("tags") else {
        return vec![];
    };
    let mut tags = Vec::new();
    match raw {
        Value::Null => {}
        Value::List(items) => {
            for item in items {
                let t = value_to_string(item);
                let t = t.trim();
                if !t.is_empty() {
                    tags.push(t.to_string());
                }
            }
        }
        Value::Str(s) => {
            tags.extend(
                s.split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|p| !p.is_empty())
                    .map(String::from),
            );
        }
        other => {
            let t = value_to_string(other);
            let t = t.trim();
            if !t.is_empty() {
                tags.push(t.to_string());
            }
        }
    }
    tags
}

/// Strips a leading `#` and trims whitespace.
pub fn normalize_tag(tag: &str) -> String {
    tag.strip_prefix('#').unwrap_or(tag).trim().to_string()
}

/// All unique tags referenced by a note (inline first, then frontmatter), in
/// first-seen order.
pub fn all_tags(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut add = |t: String| {
        if seen.insert(t.clone()) {
            out.push(t);
        }
    };
    for m in parse_inline_tags(content) {
        add(m.tag);
    }
    for t in frontmatter_tags(content) {
        let n = normalize_tag(&t);
        if !n.is_empty() {
            add(n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(line: &str) -> Vec<String> {
        match_inline_tags_in_line(line)
            .into_iter()
            .map(|m| m.tag)
            .collect()
    }

    #[test]
    fn matches_a_simple_tag() {
        assert_eq!(tags("some text #foo more text"), ["foo"]);
    }

    #[test]
    fn matches_a_tag_at_the_start_of_a_line() {
        let m = match_inline_tags_in_line("#foo rest of line");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].tag, "foo");
        assert_eq!(m[0].from, 0);
    }

    #[test]
    fn matches_nested_tags_with_slashes() {
        assert_eq!(tags("see #foo/bar for details"), ["foo/bar"]);
    }

    #[test]
    fn matches_multiple_tags_on_one_line() {
        assert_eq!(tags("#one and #two"), ["one", "two"]);
    }

    #[test]
    fn does_not_match_a_bare_atx_heading() {
        assert!(tags("# Heading").is_empty());
        assert!(tags("## Sub heading").is_empty());
        assert!(tags("###### Deep heading").is_empty());
    }

    #[test]
    fn matches_a_tag_inside_a_heading_line_after_the_heading_text() {
        assert_eq!(tags("## Heading #tag"), ["tag"]);
    }

    #[test]
    fn does_not_match_a_url_fragment() {
        assert!(tags("see example.com/#frag for info").is_empty());
    }

    #[test]
    fn does_not_match_mid_word_hits() {
        assert!(tags("foo#bar").is_empty());
    }

    #[test]
    fn does_not_match_a_bare_number() {
        assert!(tags("issue #123").is_empty());
    }

    #[test]
    fn does_not_match_inside_an_inline_code_span() {
        assert!(tags("use `#notatag` here").is_empty());
    }

    #[test]
    fn trims_trailing_punctuation_like_separators() {
        assert_eq!(tags("#foo/ end"), ["foo"]);
    }

    #[test]
    fn stops_a_tag_at_punctuation() {
        assert_eq!(tags("great post, #foo, nice"), ["foo"]);
    }

    #[test]
    fn does_not_match_when_preceded_by_a_non_whitespace_character() {
        assert!(tags("great post (#foo).").is_empty());
    }

    #[test]
    fn skips_tags_inside_fenced_code_blocks() {
        let content = ["before #real", "```", "#fake in code", "```", "after"].join("\n");
        let got: Vec<_> = parse_inline_tags(&content)
            .into_iter()
            .map(|m| m.tag)
            .collect();
        assert_eq!(got, ["real"]);
    }

    #[test]
    fn computes_correct_offsets_across_multiple_lines() {
        let content = "line one\nline two #tag here";
        let m = &parse_inline_tags(content)[0];
        assert_eq!(&content[m.from..m.to], "#tag");
    }

    #[test]
    fn handles_tilde_fences_too() {
        let content = ["~~~", "#fake", "~~~", "#real"].join("\n");
        let got: Vec<_> = parse_inline_tags(&content)
            .into_iter()
            .map(|m| m.tag)
            .collect();
        assert_eq!(got, ["real"]);
    }

    #[test]
    fn frontmatter_inline_array() {
        let c = ["---", "tags: [a, b, \"c d\"]", "---", "body"].join("\n");
        assert_eq!(frontmatter_tags(&c), ["a", "b", "c d"]);
    }

    #[test]
    fn frontmatter_block_list() {
        let c = ["---", "tags:", "  - a", "  - b", "---", "body"].join("\n");
        assert_eq!(frontmatter_tags(&c), ["a", "b"]);
    }

    #[test]
    fn frontmatter_space_separated_scalar() {
        let c = ["---", "tags: a b c", "---", "body"].join("\n");
        assert_eq!(frontmatter_tags(&c), ["a", "b", "c"]);
    }

    #[test]
    fn frontmatter_absent_or_without_tags() {
        assert!(frontmatter_tags("# just a note\nno frontmatter here").is_empty());
        assert!(frontmatter_tags(&["---", "title: hello", "---", "body"].join("\n")).is_empty());
    }

    #[test]
    fn normalize_strips_leading_hash() {
        assert_eq!(normalize_tag("#foo"), "foo");
        assert_eq!(normalize_tag("foo"), "foo");
    }

    #[test]
    fn all_tags_merges_and_dedupes() {
        let c = [
            "---",
            "tags: [shared, fm-only]",
            "---",
            "body with #shared and #inline-only",
        ]
        .join("\n");
        let got: HashSet<_> = all_tags(&c).into_iter().collect();
        let want: HashSet<_> = ["shared", "fm-only", "inline-only"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn code_span_masking_handles_double_backticks_and_unclosed() {
        assert!(tags("a ``#x`` b").is_empty());
        assert_eq!(tags("a `unclosed #x"), ["x"]);
    }
}
