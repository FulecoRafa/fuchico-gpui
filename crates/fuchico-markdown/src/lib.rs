//! UI-agnostic Markdown live-preview model for the native Fuchico editor.
//!
//! [`MarkdownModel`] owns the text, re-parses it on every update and answers
//! decoration, folding, table and completion queries. It replaces every
//! CodeMirror ViewPlugin / StateField of the original app (markdownStyle,
//! codeBlockStyle, mermaidPreview, tableStyle, wikilinks, tags,
//! frontmatter, region folding/decorations, rulers, completion). See the
//! crate README for the API tour and documented deviations.

pub mod completion;
pub mod decorate;
pub mod fold;
pub mod highlight;
pub mod languages;
pub mod rulers;
pub mod style;
pub mod syntax;
pub mod table;
pub mod text;

use std::ops::Range;

pub use completion::{CompletionContext, CompletionOption, CompletionSource, CompletionType};
pub use decorate::{
    checkbox_toggle, BlockWidget, DecorationContext, FenceToolbar, Hidden, LineDecoration, LineDecorations,
    LineEndWidget, Replacement, Widget,
};
pub use fold::{code_foldable, CodeFoldStyle, FoldPlaceholder, FoldState};
pub use highlight::FileHighlighter;
pub use style::{CalloutKind, ColorToken, LineClass, LineHeight, ResolvedStyle, StyleId, StyleRun, StyleSet};
pub use syntax::{BlockKind, Parsed};
pub use table::{Align, TableData, TableEditState, TableKey, TableModel, TableOutcome};
pub use text::{apply_edits, LineIndex, TextEdit};

use fuchico_core::outline::{extract_outline, OutlineHeader};
use highlight::SnippetCache;

/// Editor settings that affect the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Region start marker (`:::fold`); blank disables region folding.
    pub fold_start_marker: String,
    /// Region end marker (`:::endfold`).
    pub fold_end_marker: String,
    /// Indent unit for indent guides.
    pub tab_size: usize,
    /// Ruler columns (see [`rulers::parse_rulers`]).
    pub rulers: Vec<u32>,
    /// Replace `![alt](url)` with an image widget off-cursor.
    pub inline_images: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fold_start_marker: ":::fold".into(),
            fold_end_marker: ":::endfold".into(),
            tab_size: 2,
            rulers: Vec::new(),
            inline_images: false,
        }
    }
}

/// The live-preview model of one Markdown document.
pub struct MarkdownModel {
    text: String,
    lines: LineIndex,
    settings: Settings,
    parsed: Parsed,
    snippets: SnippetCache,
}

impl MarkdownModel {
    pub fn new(text: &str, settings: Settings) -> Self {
        let lines = LineIndex::new(text);
        let parsed = syntax::parse(text, &lines, settings.inline_images);
        Self { text: text.to_string(), lines, settings, parsed, snippets: SnippetCache::default() }
    }

    /// Replaces the text. `edits` (old-document offsets) describe the change
    /// and are accepted for API symmetry with incremental parsers; the parse
    /// itself is a full re-parse (fast enough, see the README), while fenced
    /// code highlighting is reused through a content-keyed cache.
    pub fn update(&mut self, text: &str, edits: &[TextEdit]) {
        let _ = edits;
        self.text.clear();
        self.text.push_str(text);
        self.reparse();
    }

    /// Applies `edits` (old-document offsets) to the current text.
    pub fn apply(&mut self, edits: &[TextEdit]) {
        self.text = apply_edits(&self.text, edits);
        self.reparse();
    }

    /// [`Self::update`] from a rope.
    pub fn update_rope(&mut self, rope: &ropey::Rope, edits: &[TextEdit]) {
        let s = text::rope_to_string(rope);
        self.update(&s, edits);
    }

    fn reparse(&mut self) {
        self.lines = LineIndex::new(&self.text);
        self.parsed = syntax::parse(&self.text, &self.lines, self.settings.inline_images);
    }

    pub fn set_settings(&mut self, settings: Settings) {
        let reparse = settings.inline_images != self.settings.inline_images;
        self.settings = settings;
        if reparse {
            self.reparse();
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn lines(&self) -> &LineIndex {
        &self.lines
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Raw parse facts (block tree, per-line kinds, fences, tables).
    pub fn parsed(&self) -> &Parsed {
        &self.parsed
    }

    /// Static facts of a line.
    pub fn line_info(&self, line: usize) -> Option<&syntax::LineInfo> {
        self.parsed.lines.get(line)
    }

    /// True inside fenced/indented code or an inline code span.
    pub fn is_inside_code(&self, pos: usize) -> bool {
        self.parsed.is_inside_code(pos)
    }

    pub fn table_count(&self) -> usize {
        self.parsed.tables.len()
    }

    /// Table model `i` (cells with source ranges, alignment, widths).
    pub fn table(&self, i: usize) -> Option<TableModel> {
        let t = self.parsed.tables.get(i)?;
        Some(TableModel::parse(&self.text, t.range.clone(), t.quoted))
    }

    /// Table containing `pos`, with its index.
    pub fn table_at(&self, pos: usize) -> Option<(usize, TableModel)> {
        let i = self.parsed.tables.iter().position(|t| t.range.start <= pos && pos <= t.range.end)?;
        Some((i, self.table(i)?))
    }

    /// Document outline (fuchico-core `extract_outline`).
    pub fn outline(&self) -> Vec<OutlineHeader> {
        extract_outline(&self.text)
    }

    pub fn rulers(&self) -> &[u32] {
        &self.settings.rulers
    }

    /// Byte range of 0-based `line` (without the newline).
    pub fn line_range(&self, line: usize) -> Range<usize> {
        self.lines.line_range(line)
    }
}
