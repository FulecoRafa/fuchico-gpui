//! Completion sources as pure functions, ported from the editor's shared
//! aggregator (`completion.ts`) and its providers, in the original provider
//! order: wikilink, heading, tag, task (`@due` / `@today` / `@repeat`),
//! word.
//!
//! There are NO frontmatter-key completions in the original app, so none
//! are provided here.

use std::ops::Range;
use std::sync::LazyLock;

use chrono::NaiveDate;
use fuchico_core::fuzzy::fuzzy_match;
use fuchico_core::task_helpers::{TaskKind, task_completions, task_trigger};
use fuchico_core::wikilinks::{resolve_wikilink_target, wikilink_completions};
use regex::Regex;

use crate::MarkdownModel;
use crate::text::TextEdit;

/// CodeMirror's `matchBefore` looks at most this many chars back.
const MATCH_BEFORE_WINDOW: usize = 250;
/// CodeMirror's `maxRenderedOptions` default.
pub const MAX_RENDERED_OPTIONS: usize = 100;
const MAX_TAGS: usize = 50;
const MAX_HEADINGS: usize = 50;
const MAX_WORDS: usize = 20;

static TAG_TRIGGER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([\p{L}][\p{L}\p{N}_/-]*)$").unwrap());
static WIKILINK_TRIGGER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\[\]]*)$").unwrap());
static HEADING_TRIGGER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\[\]#]*)#([^\[\]]*)$").unwrap());
static WORD_TRIGGER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9_-]{3,}$").unwrap());
static WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9_-]{3,}").unwrap());
static OPEN_WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[[^\[\]]*$").unwrap());
static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(`{3,}|~{3,})").unwrap());
static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,6}\s+(.+?)\s*#*\s*$").unwrap());

/// Which provider produced an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSource {
    Wikilink,
    Heading,
    Tag,
    Task,
    Word,
}

/// CodeMirror completion `type` (drives the icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionType {
    Text,
    Keyword,
    Variable,
}

impl CompletionType {
    pub fn as_str(self) -> &'static str {
        match self {
            CompletionType::Text => "text",
            CompletionType::Keyword => "keyword",
            CompletionType::Variable => "variable",
        }
    }
}

/// One option, with its accept action precomputed.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionOption {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionType,
    pub source: CompletionSource,
    pub boost: i32,
    /// Range the option completes (`from..pos`).
    pub range: Range<usize>,
    /// Edit applied on accept.
    pub edit: TextEdit,
    /// Cursor after accepting.
    pub cursor: usize,
    /// True if accepting carries `pickedCompletion` (suppresses the
    /// re-open trigger). False for task options, whose custom `apply` does
    /// not annotate the transaction.
    pub picked: bool,
    /// True if the option has a custom `apply` (never deduplicated).
    pub custom_apply: bool,
}

/// Host-provided inputs.
pub struct CompletionContext<'a> {
    /// Cursor position (byte offset).
    pub pos: usize,
    pub vault_files: &'a [String],
    pub all_tags: &'a [String],
    pub today: NaiveDate,
    /// Reads another note's text for `[[Note#heading` (the original's
    /// `fs_read_file`); `None` result = no completions.
    pub read_note: &'a dyn Fn(&str) -> Option<String>,
}

/// `context.matchBefore(re)`: the match of `re` (which must end in `$`) in
/// the last [`MATCH_BEFORE_WINDOW`] chars of the line before the cursor.
/// Returns the matched text's start offset within `before`.
fn match_before<'b>(before: &'b str, re: &Regex) -> Option<(usize, &'b str)> {
    let n = before.chars().count();
    let start = if n > MATCH_BEFORE_WINDOW {
        before
            .char_indices()
            .nth(n - MATCH_BEFORE_WINDOW)
            .map(|(i, _)| i)
            .unwrap_or(0)
    } else {
        0
    };
    let m = re.find(&before[start..])?;
    Some((start + m.start(), m.as_str()))
}

/// The aggregator's re-open rule: after a document change that was not an
/// accepted completion, open the popup if any provider's trigger matches
/// the line text before the cursor.
pub fn should_open_completion(before: &str, picked: bool) -> bool {
    if picked {
        return false;
    }
    WIKILINK_TRIGGER_RE.is_match(before)
        || HEADING_TRIGGER_RE.is_match(before)
        || TAG_TRIGGER_RE.is_match(before)
        || task_trigger(before)
        || (!OPEN_WIKILINK_RE.is_match(before) && WORD_TRIGGER_RE.is_match(before))
}

/// `extractHeadings` (headingCompletion.ts): ATX heading texts outside
/// fenced code.
pub fn extract_headings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fence: Option<char> = None;
    for line in text.split('\n') {
        if let Some(c) = FENCE_RE.captures(line) {
            let marker = c[1].chars().next().unwrap();
            fence = match fence {
                None => Some(marker),
                Some(f) if f == marker => None,
                f => f,
            };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some(c) = HEADING_RE.captures(line) {
            out.push(c[1].trim().to_string());
        }
    }
    out
}

/// `extractWords` (wordCompletion.ts): distinct 4+ char words, first-seen
/// order.
pub fn extract_words(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for m in WORD_RE.find_iter(text) {
        if seen.insert(m.as_str()) {
            out.push(m.as_str().to_string());
        }
    }
    out
}

/// Stable "score descending" sort + cap, like `.sort((a,b)=>b.score-a.score).slice(0,n)`.
fn rank<T>(mut v: Vec<(T, f64)>, cap: usize) -> Vec<T> {
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v.into_iter().take(cap).map(|(t, _)| t).collect()
}

fn link_apply(doc: &str, from: usize, to: usize, label: &str) -> (TextEdit, usize) {
    let has_close = doc.get(to..).is_some_and(|s| s.starts_with("]]"));
    let insert = if has_close {
        label.to_string()
    } else {
        format!("{label}]]")
    };
    let cursor = from + insert.len();
    (TextEdit::new(from..to, insert), cursor)
}

fn plain_option(
    source: CompletionSource,
    label: String,
    from: usize,
    to: usize,
) -> CompletionOption {
    let cursor = from + label.len();
    CompletionOption {
        edit: TextEdit::new(from..to, label.clone()),
        label,
        detail: None,
        kind: CompletionType::Text,
        source,
        boost: 0,
        range: from..to,
        cursor,
        picked: true,
        custom_apply: false,
    }
}

fn wikilink_source(
    doc: &str,
    line_start: usize,
    before: &str,
    ctx: &CompletionContext,
) -> Vec<CompletionOption> {
    let Some((at, m)) = match_before(before, &WIKILINK_TRIGGER_RE) else {
        return Vec::new();
    };
    let query = m.strip_prefix("[[").unwrap_or(m);
    let to = ctx.pos;
    let from = line_start + at + m.len() - query.len();
    wikilink_completions(query, ctx.vault_files)
        .into_iter()
        .map(|c| {
            let (edit, cursor) = link_apply(doc, from, to, &c.label);
            CompletionOption {
                label: c.label,
                detail: Some(c.path),
                kind: CompletionType::Text,
                source: CompletionSource::Wikilink,
                boost: 0,
                range: from..to,
                edit,
                cursor,
                picked: true,
                custom_apply: true,
            }
        })
        .collect()
}

fn heading_source(
    doc: &str,
    line_start: usize,
    before: &str,
    ctx: &CompletionContext,
) -> Vec<CompletionOption> {
    let Some((_, m)) = match_before(before, &HEADING_TRIGGER_RE) else {
        return Vec::new();
    };
    let Some(c) = HEADING_TRIGGER_RE.captures(m) else {
        return Vec::new();
    };
    let (target, query) = (&c[1], &c[2]);
    let to = ctx.pos;
    let from = to - query.len();
    let _ = line_start;
    let headings = if target.trim().is_empty() {
        extract_headings(doc)
    } else {
        let Some(path) = resolve_wikilink_target(target, ctx.vault_files) else {
            return Vec::new();
        };
        match (ctx.read_note)(&path) {
            Some(t) => extract_headings(&t),
            None => return Vec::new(),
        }
    };
    let scored = headings
        .into_iter()
        .filter_map(|h| {
            let r = fuzzy_match(query, &h);
            r.matched.then_some((h, r.score))
        })
        .collect();
    rank(scored, MAX_HEADINGS)
        .into_iter()
        .map(|h| {
            let (edit, cursor) = link_apply(doc, from, to, &h);
            CompletionOption {
                label: h,
                detail: None,
                kind: CompletionType::Text,
                source: CompletionSource::Heading,
                boost: 0,
                range: from..to,
                edit,
                cursor,
                picked: true,
                custom_apply: true,
            }
        })
        .collect()
}

fn tag_source(line_start: usize, before: &str, ctx: &CompletionContext) -> Vec<CompletionOption> {
    let Some((_, m)) = match_before(before, &TAG_TRIGGER_RE) else {
        return Vec::new();
    };
    let query = &m[m.rfind('#').map(|i| i + 1).unwrap_or(0)..];
    let to = ctx.pos;
    let from = to - query.len();
    let _ = line_start;
    let scored = ctx
        .all_tags
        .iter()
        .filter_map(|t| {
            let r = fuzzy_match(query, t);
            r.matched.then(|| (t.clone(), r.score))
        })
        .collect();
    rank(scored, MAX_TAGS)
        .into_iter()
        .map(|t| plain_option(CompletionSource::Tag, t, from, to))
        .collect()
}

fn task_source(
    line: &str,
    line_start: usize,
    before: &str,
    ctx: &CompletionContext,
) -> Vec<CompletionOption> {
    let Some(res) = task_completions(before, line, ctx.today) else {
        return Vec::new();
    };
    let from = line_start + res.from;
    let to = ctx.pos;
    let kind = match res.kind {
        TaskKind::Due => CompletionType::Keyword,
        TaskKind::Recur => CompletionType::Variable,
    };
    res.options
        .into_iter()
        .map(|o| {
            let cursor = from + o.insert.len();
            CompletionOption {
                label: o.label,
                detail: Some(o.detail),
                kind,
                source: CompletionSource::Task,
                boost: o.boost,
                range: from..to,
                edit: TextEdit::new(from..to, o.insert),
                cursor,
                picked: false,
                custom_apply: true,
            }
        })
        .collect()
}

fn word_source(
    model: &MarkdownModel,
    line_start: usize,
    before: &str,
    ctx: &CompletionContext,
) -> Vec<CompletionOption> {
    if OPEN_WIKILINK_RE.is_match(before) {
        return Vec::new();
    }
    let Some((at, query)) = match_before(before, &WORD_TRIGGER_RE) else {
        return Vec::new();
    };
    if model.is_inside_code(ctx.pos) {
        return Vec::new();
    }
    let from = line_start + at;
    let to = ctx.pos;
    let ql = query.to_lowercase();
    let scored = extract_words(model.text())
        .into_iter()
        .filter(|w| w.to_lowercase() != ql)
        .filter_map(|w| {
            let r = fuzzy_match(query, &w);
            r.matched.then_some((w, r.score))
        })
        .collect();
    rank(scored, MAX_WORDS)
        .into_iter()
        .map(|w| plain_option(CompletionSource::Word, w, from, to))
        .collect()
}

impl MarkdownModel {
    /// Runs every provider at `ctx.pos` and merges the results the way
    /// CodeMirror's `sortOptions` does for `filter: false` sources: provider
    /// order, then each source's own order (boost has no effect), consecutive
    /// duplicates (same label/detail/type, no custom apply) removed, capped
    /// at [`MAX_RENDERED_OPTIONS`].
    pub fn completions(&self, ctx: &CompletionContext) -> Vec<CompletionOption> {
        let doc = self.text();
        let pos = ctx.pos.min(doc.len());
        let line = self.lines.line_of(pos);
        let lr = self.lines.line_range(line);
        let before = &doc[lr.start..pos];
        let line_text = &doc[lr.clone()];
        let ctx = CompletionContext { pos, ..*ctx };
        let mut all = Vec::new();
        all.extend(wikilink_source(doc, lr.start, before, &ctx));
        all.extend(heading_source(doc, lr.start, before, &ctx));
        all.extend(tag_source(lr.start, before, &ctx));
        all.extend(task_source(line_text, lr.start, before, &ctx));
        all.extend(word_source(self, lr.start, before, &ctx));
        let mut out: Vec<CompletionOption> = Vec::new();
        for o in all {
            if let Some(prev) = out.last()
                && !o.custom_apply
                && !prev.custom_apply
                && prev.label == o.label
                && prev.detail == o.detail
                && prev.kind == o.kind
            {
                continue;
            }
            out.push(o);
            if out.len() == MAX_RENDERED_OPTIONS {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_skip_fences() {
        let t = "# A\n```\n# no\n~~~\n# still no\n```\n## B ##\n#nope\n";
        assert_eq!(extract_headings(t), vec!["A", "B"]);
    }

    #[test]
    fn words_distinct_in_order() {
        assert_eq!(
            extract_words("abcd the abcd efgh-ij x1234"),
            vec!["abcd", "efgh-ij", "x1234"]
        );
    }

    #[test]
    fn open_rule() {
        assert!(should_open_completion("see [[no", false));
        assert!(!should_open_completion("see [[no", true));
        assert!(should_open_completion("a #ta", false));
        assert!(should_open_completion("hello", false));
        assert!(!should_open_completion("hel", false));
        assert!(should_open_completion("- [ ] x @", false));
    }
}
