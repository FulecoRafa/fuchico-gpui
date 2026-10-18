//! Code highlighting with syntect + two-face grammars, mapped onto the same
//! semantic [`StyleId`]s the original `HighlightStyle` used.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, LazyLock, Mutex};

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::languages::{FileLanguage, resolve_file_language};
use crate::style::{StyleId, StyleRun, flatten_spans};
use crate::text::{LineIndex, TextEdit};

/// The bundled syntax set: two-face's extended set (newline variant) plus
/// the `grammars/` ports, precompiled by `build.rs` and decoded on first use.
pub fn syntax_set() -> &'static SyntaxSet {
    static DUMP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/syntaxes.packdump"));
    static SET: LazyLock<SyntaxSet> = LazyLock::new(|| {
        syntect::dumps::from_uncompressed_data(DUMP).expect("bundled syntax dump")
    });
    &SET
}

/// Looks a grammar up by its syntect name.
pub fn find_grammar(name: &str) -> Option<&'static SyntaxReference> {
    syntax_set().find_syntax_by_name(name)
}

#[derive(Clone, Copy)]
enum Class {
    Id(StyleId),
    /// `meta.mapping.key` / `support.type.property-name`: wins over all.
    Override,
    /// `punctuation.definition.string|comment`: ignored (the enclosing
    /// string/comment scope colours the delimiters).
    Skip,
    None,
}

fn classify_str(s: &str) -> Class {
    let has = |p: &str| s == p || s.starts_with(&format!("{p}."));
    if has("meta.mapping.key") || has("support.type.property-name") {
        return Class::Override;
    }
    if has("punctuation.definition.string") || has("punctuation.definition.comment") {
        return Class::Skip;
    }
    let id = if has("comment") {
        StyleId::Comment
    } else if has("string") || has("constant.character") {
        StyleId::String
    } else if has("constant.numeric") || has("constant.language") || has("constant.other.symbol") {
        StyleId::Number
    } else if has("keyword") || has("storage.type") || has("storage.modifier") {
        StyleId::Keyword
    } else if has("entity.name.function") || has("support.function") || has("variable.function") {
        StyleId::Function
    } else if has("entity.name.type")
        || has("entity.name.class")
        || has("entity.name.struct")
        || has("entity.name.enum")
        || has("entity.name.trait")
        || has("support.type")
        || has("support.class")
        || has("entity.name.tag")
    {
        StyleId::Type
    } else if has("entity.other.attribute-name")
        || has("variable.other.property")
        || has("variable.other.member")
        || has("variable.other.definition")
    {
        StyleId::Property
    } else if has("markup.inserted") {
        // Not coloured by the original theme (it has no `t.inserted` /
        // `t.deleted` rule); string/destructive colours read as +/-.
        StyleId::String
    } else if has("markup.deleted") {
        StyleId::Invalid
    } else if has("markup.heading") {
        StyleId::Heading
    } else if has("markup.bold") {
        StyleId::Strong
    } else if has("markup.italic") {
        StyleId::Emphasis
    } else if has("markup.strikethrough") {
        StyleId::Strikethrough
    } else if has("markup.underline.link") {
        StyleId::Link
    } else if has("markup.quote") {
        StyleId::Quote
    } else if has("punctuation") {
        StyleId::Punctuation
    } else if has("invalid") {
        StyleId::Invalid
    } else {
        return Class::None;
    };
    Class::Id(id)
}

thread_local! {
    static CLASS_CACHE: RefCell<HashMap<Scope, Class>> = RefCell::new(HashMap::new());
}

fn classify(scope: Scope) -> Class {
    CLASS_CACHE.with(|c| {
        *c.borrow_mut()
            .entry(scope)
            .or_insert_with(|| classify_str(&scope.build_string()))
    })
}

/// Maps a scope stack to a style id (innermost scope first).
pub fn style_for_scopes(stack: &[Scope]) -> Option<StyleId> {
    let mut found = None;
    for s in stack.iter().rev() {
        match classify(*s) {
            Class::Override => return Some(StyleId::Property),
            Class::Id(id) if found.is_none() => found = Some(id),
            _ => {}
        }
    }
    found
}

/// Highlights one line, advancing `state`/`stack`. Ranges are line-local.
fn highlight_line(
    line: &str,
    state: &mut ParseState,
    stack: &mut ScopeStack,
    out: &mut Vec<(Range<usize>, StyleId)>,
) {
    let ops = match state.parse_line(line, syntax_set()) {
        Ok(ops) => ops,
        Err(_) => return,
    };
    let mut last = 0;
    for (i, op) in ops {
        if i > last {
            if let Some(id) = style_for_scopes(stack.as_slice()) {
                out.push((last..i, id));
            }
            last = i;
        }
        let _ = stack.apply(&op);
    }
    if line.len() > last
        && let Some(id) = style_for_scopes(stack.as_slice())
    {
        let end = line.trim_end_matches(['\n', '\r']).len().max(last);
        if end > last {
            out.push((last..end, id));
        }
    }
}

/// Highlights a whole snippet; ranges are offsets into `code`.
pub fn highlight_snippet(code: &str, syntax: &SyntaxReference) -> Vec<(Range<usize>, StyleId)> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::new();
    let mut off = 0;
    let mut tmp = Vec::new();
    for line in code.split_inclusive('\n') {
        tmp.clear();
        highlight_line(line, &mut state, &mut stack, &mut tmp);
        out.extend(
            tmp.drain(..)
                .map(|(r, id)| (r.start + off..r.end + off, id)),
        );
        off += line.len();
    }
    out
}

type SnippetKey = (&'static str, u64);
type Spans = Arc<Vec<(Range<usize>, StyleId)>>;

/// Cache of fenced-block highlights keyed by (grammar, content hash).
#[derive(Default)]
pub struct SnippetCache {
    map: Mutex<HashMap<SnippetKey, Spans>>,
}

const SNIPPET_CACHE_CAP: usize = 512;

impl SnippetCache {
    pub fn get(&self, grammar: &'static str, code: &str) -> Arc<Vec<(Range<usize>, StyleId)>> {
        let mut h = DefaultHasher::new();
        code.hash(&mut h);
        let key = (grammar, h.finish());
        if let Some(v) = self.map.lock().unwrap().get(&key) {
            return v.clone();
        }
        let spans = match find_grammar(grammar) {
            Some(s) => highlight_snippet(code, s),
            None => Vec::new(),
        };
        let v = Arc::new(spans);
        let mut m = self.map.lock().unwrap();
        if m.len() >= SNIPPET_CACHE_CAP {
            m.clear();
        }
        m.insert(key, v.clone());
        v
    }
}

/// Incremental highlighter for a whole non-Markdown file (JS/TS/Python/
/// Rust/HTML/CSS/JSON...). Keeps the parser state at the start of every
/// line; an edit invalidates from its first line, and lines are parsed
/// lazily up to the last requested line.
pub struct FileHighlighter {
    language: &'static FileLanguage,
    syntax: &'static SyntaxReference,
    text: String,
    lines: LineIndex,
    /// `states[i]` = state at the start of line `i` (valid for `i < states.len()`).
    states: Vec<(ParseState, ScopeStack)>,
    runs: Vec<Vec<StyleRun>>,
}

impl FileHighlighter {
    /// `None` for unknown extensions (plain text).
    pub fn for_path(path: &str, text: &str) -> Option<Self> {
        let language = resolve_file_language(path)?;
        let syntax = find_grammar(language.grammar)?;
        Some(Self::new(language, syntax, text))
    }

    fn new(language: &'static FileLanguage, syntax: &'static SyntaxReference, text: &str) -> Self {
        let mut h = Self {
            language,
            syntax,
            text: text.to_string(),
            lines: LineIndex::new(text),
            states: Vec::new(),
            runs: Vec::new(),
        };
        h.reset_from(0);
        h
    }

    pub fn language(&self) -> &'static FileLanguage {
        self.language
    }

    fn reset_from(&mut self, line: usize) {
        if line == 0 || self.states.is_empty() {
            self.states = vec![(ParseState::new(self.syntax), ScopeStack::new())];
            self.runs.clear();
        } else {
            self.states.truncate(line + 1);
            self.runs.truncate(line);
        }
    }

    /// Replaces the text; `edits` (old-document offsets) say where it
    /// changed. Pass `&[]` to invalidate everything.
    pub fn update(&mut self, text: &str, edits: &[TextEdit]) {
        let first = if edits.is_empty() {
            0
        } else {
            crate::text::first_edited_line(&self.lines, edits).unwrap_or(0)
        };
        self.text = text.to_string();
        self.lines = LineIndex::new(text);
        self.reset_from(first.min(self.runs.len()));
    }

    /// Style runs (absolute byte offsets) for each line in `lines`.
    pub fn highlight(&mut self, lines: Range<usize>) -> Vec<Vec<StyleRun>> {
        let end = lines.end.min(self.lines.len());
        while self.runs.len() < end {
            let i = self.runs.len();
            let (mut state, mut stack) = self.states[i].clone();
            let lr = self.lines.line_range(i);
            let with_nl = if i + 1 < self.lines.len() {
                lr.start..lr.end + 1
            } else {
                lr.clone()
            };
            let mut spans = Vec::new();
            highlight_line(&self.text[with_nl], &mut state, &mut stack, &mut spans);
            let abs: Vec<_> = spans
                .into_iter()
                .map(|(r, id)| (r.start + lr.start..r.end + lr.start, id))
                .collect();
            self.runs.push(flatten_spans(&abs));
            self.states.push((state, stack));
        }
        (lines.start.min(end)..end)
            .map(|i| self.runs[i].clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages::LANGUAGES;

    #[test]
    fn all_mapped_grammars_exist() {
        let mut resolved = 0;
        for d in LANGUAGES {
            if let Some(g) = d.grammar {
                assert!(find_grammar(g).is_some(), "{} -> {g} missing", d.name);
                resolved += 1;
            }
        }
        assert_eq!(resolved, 143);
    }

    #[test]
    fn rust_tokens() {
        let s = highlight_snippet(
            "fn main() { let x = \"hi\"; // c\n}\n",
            find_grammar("Rust").unwrap(),
        );
        let get = |id| s.iter().any(|(_, i)| *i == id);
        assert!(get(StyleId::Keyword));
        assert!(get(StyleId::String));
        assert!(get(StyleId::Comment));
        assert!(get(StyleId::Function));
    }

    #[test]
    fn json_keys_are_properties() {
        let code = "{\"a\": 1}";
        let s = highlight_snippet(code, find_grammar("JSON").unwrap());
        let key = s.iter().find(|(r, _)| r.start <= 1 && r.end > 1).unwrap();
        assert_eq!(key.1, StyleId::Property);
        let num = s.iter().find(|(r, _)| &code[r.clone()] == "1").unwrap();
        assert_eq!(num.1, StyleId::Number);
    }

    #[test]
    fn file_highlighter_incremental() {
        let mut h = FileHighlighter::for_path("a.py", "x = 1\n# c\ny = 'z'\n").unwrap();
        let r = h.highlight(0..3);
        assert!(r[1].iter().any(|r| r.styles.contains(StyleId::Comment)));
        h.update("x = 1\n'''doc\ny = 'z'\n", &[TextEdit::new(6..9, "'''doc")]);
        let r = h.highlight(0..3);
        // Line 2 is now inside the docstring.
        assert!(
            r[2].iter()
                .all(|r| r.styles.contains(StyleId::String) || r.styles.contains(StyleId::Comment))
        );
        assert!(FileHighlighter::for_path("Makefile", "").is_none());
    }
}
