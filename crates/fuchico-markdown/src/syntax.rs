//! The parse pass: pulldown-cmark events turned into the per-line facts the
//! original CodeMirror plugins read from the Lezer tree (marks, line
//! classes, style spans, widgets candidates) plus a block tree for folding.
//!
//! Everything here is cursor-independent; [`crate::decorate`] applies the
//! cursor-dependent reveal rules on top.

use std::ops::Range;
use std::sync::LazyLock;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use regex::Regex;

use crate::languages::{LanguageDesc, language_for_info};
use crate::style::{CalloutKind, LineClass, LineHeight, StyleId};
use crate::text::LineIndex;

/// Kind of a block-tree node (the Lezer node types folding cares about).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    /// YAML frontmatter (kept out of the Markdown parse, see the README).
    Frontmatter,
    Heading {
        level: u8,
        setext: bool,
    },
    Paragraph,
    Blockquote,
    List {
        ordered: bool,
    },
    ListItem,
    FencedCode,
    IndentedCode,
    HtmlBlock,
    Table,
    Rule,
}

impl NodeKind {
    /// `foldNodeProp` of lang-markdown: every block except the document,
    /// headings and lists (headings fold through the heading service).
    /// Frontmatter is foldable too (our addition).
    pub fn is_foldable(self) -> bool {
        !matches!(
            self,
            NodeKind::Document | NodeKind::Heading { .. } | NodeKind::List { .. }
        )
    }
}

/// A node of the block tree. `range` ends at the last non-whitespace byte,
/// like Lezer's `node.to`.
#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub range: Range<usize>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// Leaf block kind of a line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BlockKind {
    #[default]
    Blank,
    Paragraph,
    /// ATX heading line, or the text lines of a setext heading.
    Heading {
        level: u8,
        setext: bool,
    },
    /// The `===` / `---` underline of a setext heading.
    SetextUnderline {
        level: u8,
    },
    /// First line of a list item. `task` is `Some(checked)` for `- [ ]`.
    ListItem {
        ordered: bool,
        task: Option<bool>,
    },
    /// Opening fence line. `language` is the first word of the info string
    /// (as typed), `resolved` the language-data name it maps to.
    FenceOpen {
        language: String,
        resolved: Option<&'static str>,
    },
    FenceBody,
    FenceClose,
    IndentedCode,
    /// A table line; `header` for the first row.
    Table {
        header: bool,
    },
    /// A frontmatter line; `fence` for the opening/closing `---`.
    Frontmatter {
        fence: bool,
    },
    HorizontalRule,
    Html,
}

/// Callout membership of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalloutLine {
    pub kind: CalloutKind,
    /// Text after `[!TYPE]` on the first line, trimmed (may be empty).
    pub title: String,
    /// True on the callout's first line.
    pub header: bool,
}

/// Cursor-dependent inline decoration candidates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InlineKind {
    /// Hidden unless the cursor is on `line` (HeaderMark, QuoteMark,
    /// EmphasisMark, CodeMark, StrikethroughMark).
    Hide,
    /// Always replaced by a checkbox.
    Checkbox { checked: bool },
    /// `[text](url)`, off-line.
    Link { text: String, url: String },
    /// `<url>`, off-line.
    Autolink { url: String },
    /// `[!TYPE]`, off-line.
    CalloutHeader(CalloutKind),
    /// `![alt](url)`, off-line, only with `Settings::inline_images`.
    Image { alt: String, url: String },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InlineItem {
    pub range: Range<usize>,
    /// Line of the node's `from`; the item is revealed when the cursor is on
    /// it (except checkboxes).
    pub line: usize,
    pub kind: InlineKind,
}

/// Static facts about one line.
#[derive(Debug, Clone, Default)]
pub struct LineInfo {
    pub kind: BlockKind,
    /// Number of enclosing block quotes.
    pub quote_depth: u8,
    pub callout: Option<CalloutLine>,
    /// Number of enclosing lists.
    pub list_depth: u8,
    pub classes: Vec<LineClass>,
    pub height: LineHeight,
    pub(crate) spans: Vec<(Range<usize>, StyleId)>,
    pub(crate) items: Vec<InlineItem>,
    pub(crate) fence: Option<u32>,
    pub(crate) table: Option<u32>,
}

/// A fenced code block.
#[derive(Debug, Clone)]
pub struct Fence {
    pub range: Range<usize>,
    pub open_line: usize,
    /// Last line of the node (the closing fence when `closed`).
    pub last_line: usize,
    pub closed: bool,
    /// Info string, trimmed (`CodeInfo`).
    pub info: String,
    pub info_range: Option<Range<usize>>,
    pub language: Option<&'static LanguageDesc>,
    /// Source pieces of the code content (container prefixes excluded).
    pub pieces: Vec<Range<usize>>,
}

impl Fence {
    pub fn is_mermaid(&self) -> bool {
        self.info.to_lowercase() == "mermaid"
    }
}

/// A table block (rendered by the table widget).
#[derive(Debug, Clone)]
pub struct TableBlock {
    pub range: Range<usize>,
    pub first_line: usize,
    pub last_line: usize,
    /// Inside a block quote (rows carry `>` prefixes).
    pub quoted: bool,
}

/// Frontmatter location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmatterBlock {
    /// 0-based closing fence line.
    pub close_line: usize,
    /// Byte offset where the Markdown body starts.
    pub body_start: usize,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub nodes: Vec<Node>,
    pub lines: Vec<LineInfo>,
    pub fences: Vec<Fence>,
    pub tables: Vec<TableBlock>,
    pub frontmatter: Option<FrontmatterBlock>,
    /// Sorted, non-overlapping code ranges (fenced, indented, inline).
    pub code_ranges: Vec<Range<usize>>,
    /// Ids of the nodes starting on each line, outermost first.
    pub line_nodes: Vec<Vec<u32>>,
}

static CALLOUT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(>\s*)(\[!(NOTE|WARNING|TIP|IMPORTANT|CAUTION)\])").unwrap()
});
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([^\]]*)\]\(([^)]*)\)$").unwrap());
static IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^!\[([^\]]*)\]\(([^)]*)\)$").unwrap());
static EMOJI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":[a-zA-Z_0-9]+:").unwrap());
static BARE_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:https?://|www\.)[^\s<]*[^\s<?!.,:*_~'`)\]]|[A-Za-z0-9.+_-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}")
        .unwrap()
});
static FM_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([ \t]*)([^:\s][^:]*)(:)").unwrap());

/// pulldown-cmark options matching the original Lezer `markdownLanguage`
/// (CommonMark + GFM tables, strikethrough, task lists, plus the
/// subscript/superscript extensions so `~x~` is not mistaken for anything).
pub fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
}

fn trim_range(text: &str, r: Range<usize>) -> Range<usize> {
    let b = text.as_bytes();
    let mut end = r.end.min(text.len());
    while end > r.start && matches!(b[end - 1], b' ' | b'\t' | b'\n' | b'\r') {
        end -= 1;
    }
    r.start..end
}

struct Builder<'a> {
    text: &'a str,
    idx: &'a LineIndex,
    inline_images: bool,
    out: Parsed,
}

impl Builder<'_> {
    fn span(&mut self, r: Range<usize>, id: StyleId) {
        if r.start >= r.end {
            return;
        }
        let (l0, l1) = (
            self.idx.line_of(r.start),
            self.idx.line_of(r.end.saturating_sub(1).max(r.start)),
        );
        for l in l0..=l1 {
            let lr = self.idx.line_range(l);
            let (a, b) = (r.start.max(lr.start), r.end.min(lr.end));
            if a < b {
                self.out.lines[l].spans.push((a..b, id));
            }
        }
    }

    fn item(&mut self, range: Range<usize>, line: usize, kind: InlineKind) {
        let l = self.idx.line_of(range.start);
        self.out.lines[l]
            .items
            .push(InlineItem { range, line, kind });
    }

    fn hide(&mut self, range: Range<usize>) {
        if range.start < range.end {
            let line = self.idx.line_of(range.start);
            self.item(range, line, InlineKind::Hide);
        }
    }

    /// Hides `range` plus one following space (HeaderMark / QuoteMark).
    fn hide_with_space(&mut self, range: Range<usize>) {
        let end = if self.text.as_bytes().get(range.end) == Some(&b' ') {
            range.end + 1
        } else {
            range.end
        };
        self.hide(range.start..end);
    }

    fn class(&mut self, line: usize, c: LineClass) {
        let cl = &mut self.out.lines[line].classes;
        if !cl.contains(&c) {
            cl.push(c);
        }
    }

    fn add_node(&mut self, kind: NodeKind, range: Range<usize>, parent: usize) -> usize {
        let id = self.out.nodes.len();
        self.out.nodes.push(Node {
            kind,
            range,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.out.nodes[parent].children.push(id);
        let l = self.idx.line_of(self.out.nodes[id].range.start);
        self.out.line_nodes[l].push(id as u32);
        id
    }

    fn lines_of(&self, r: &Range<usize>) -> Range<usize> {
        self.idx.line_of(r.start)..self.idx.line_of(r.end) + 1
    }
}

enum Frame {
    Block,
    Inline,
    CodeBlock,
    Table,
    Link,
}

/// The facts of a non-Markdown file: a bare document node, no blocks.
pub fn plain(text: &str, idx: &LineIndex) -> Parsed {
    Parsed {
        lines: vec![LineInfo::default(); idx.len()],
        line_nodes: vec![Vec::new(); idx.len()],
        nodes: vec![Node {
            kind: NodeKind::Document,
            range: 0..text.len(),
            parent: None,
            children: Vec::new(),
        }],
        ..Default::default()
    }
}

/// Parses `text` into the static decoration facts.
pub fn parse(text: &str, idx: &LineIndex, inline_images: bool) -> Parsed {
    let mut b = Builder {
        text,
        idx,
        inline_images,
        out: Parsed {
            lines: vec![LineInfo::default(); idx.len()],
            line_nodes: vec![Vec::new(); idx.len()],
            ..Default::default()
        },
    };
    b.out.nodes.push(Node {
        kind: NodeKind::Document,
        range: 0..text.len(),
        parent: None,
        children: Vec::new(),
    });

    // --- Frontmatter (not Markdown-parsed) ---
    let mut cut = text.len().min(20_000);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut body_start = 0;
    if let Some(fm) = fuchico_core::frontmatter::parse_frontmatter(&text[..cut]) {
        let close_line = fm.end_line - 1;
        let range = 0..idx.line_end(close_line);
        b.add_node(NodeKind::Frontmatter, range.clone(), 0);
        for l in 0..=close_line {
            let fence = l == 0 || l == close_line;
            b.out.lines[l].kind = BlockKind::Frontmatter { fence };
            b.class(l, LineClass::FrontmatterLine);
            if fence {
                b.class(l, LineClass::FrontmatterFence);
            } else {
                let lr = idx.line_range(l);
                if let Some(c) = FM_KEY_RE.captures(&text[lr.clone()]) {
                    let k = c.get(2).unwrap();
                    b.span(
                        lr.start + k.start()..lr.start + k.end(),
                        StyleId::FrontmatterKey,
                    );
                }
            }
        }
        body_start = fm.body_start;
        b.out.frontmatter = Some(FrontmatterBlock {
            close_line,
            body_start,
            range,
        });
    }

    parse_body(&mut b, body_start);
    b.out.code_ranges.sort_by_key(|r| r.start);
    b.out
}

fn parse_body(b: &mut Builder, off: usize) {
    let text = b.text;
    let body = &text[off..];
    let mut stack: Vec<Frame> = Vec::new();
    let mut parents: Vec<usize> = vec![0];
    let mut in_table = 0usize;
    let mut in_link = 0usize;
    let mut cur_fence: Option<usize> = None;
    let mut in_code = false;
    let mut text_run: Option<Range<usize>> = None;
    let mut list_ordered: Vec<bool> = Vec::new();
    let mut item_lines: Vec<usize> = Vec::new();

    let flush = |b: &mut Builder, run: &mut Option<Range<usize>>, in_link: usize| {
        if let Some(r) = run.take() {
            let src = &b.text[r.clone()];
            for m in EMOJI_RE.find_iter(src) {
                b.span(r.start + m.start()..r.start + m.end(), StyleId::String);
            }
            if in_link == 0 {
                for m in BARE_URL_RE.find_iter(src) {
                    b.span(r.start + m.start()..r.start + m.end(), StyleId::Url);
                }
            }
        }
    };

    for (ev, r) in Parser::new_ext(body, parser_options()).into_offset_iter() {
        let r = r.start + off..r.end + off;
        if !matches!(ev, Event::Text(_)) || in_code {
            flush(b, &mut text_run, in_link);
        }
        let parent = *parents.last().unwrap();
        match ev {
            Event::Start(tag) => {
                let tr = trim_range(text, r.clone());
                match tag {
                    Tag::Paragraph => {
                        let id = b.add_node(NodeKind::Paragraph, tr.clone(), parent);
                        for l in b.lines_of(&tr) {
                            if b.out.lines[l].kind == BlockKind::Blank {
                                b.out.lines[l].kind = BlockKind::Paragraph;
                            }
                        }
                        parents.push(id);
                        stack.push(Frame::Block);
                    }
                    Tag::Heading { level, .. } => {
                        let level = heading_level(level);
                        let lines = b.lines_of(&tr);
                        let setext = lines.len() > 1 || !is_atx_start(text, tr.start);
                        let id =
                            b.add_node(NodeKind::Heading { level, setext }, tr.clone(), parent);
                        heading(b, tr.clone(), level, setext);
                        parents.push(id);
                        stack.push(Frame::Block);
                    }
                    Tag::BlockQuote(_) => {
                        let id = b.add_node(NodeKind::Blockquote, tr.clone(), parent);
                        blockquote(b, tr.clone());
                        parents.push(id);
                        stack.push(Frame::Block);
                    }
                    Tag::List(start) => {
                        let id = b.add_node(
                            NodeKind::List {
                                ordered: start.is_some(),
                            },
                            tr.clone(),
                            parent,
                        );
                        for l in b.lines_of(&tr) {
                            b.out.lines[l].list_depth += 1;
                        }
                        list_ordered.push(start.is_some());
                        parents.push(id);
                        stack.push(Frame::Block);
                    }
                    Tag::Item => {
                        let id = b.add_node(NodeKind::ListItem, tr.clone(), parent);
                        let l = b.idx.line_of(tr.start);
                        let ordered = list_ordered.last().copied().unwrap_or(false);
                        b.out.lines[l].kind = BlockKind::ListItem {
                            ordered,
                            task: None,
                        };
                        item_lines.push(l);
                        parents.push(id);
                        stack.push(Frame::Block);
                    }
                    Tag::CodeBlock(kind) => {
                        in_code = true;
                        b.out.code_ranges.push(tr.clone());
                        match kind {
                            CodeBlockKind::Fenced(_) => {
                                b.add_node(NodeKind::FencedCode, tr.clone(), parent);
                                cur_fence = Some(fence_start(b, tr.clone()));
                                stack.push(Frame::CodeBlock);
                            }
                            CodeBlockKind::Indented => {
                                b.add_node(NodeKind::IndentedCode, tr.clone(), parent);
                                for l in b.lines_of(&tr) {
                                    b.out.lines[l].kind = BlockKind::IndentedCode;
                                }
                                b.span(tr.clone(), StyleId::Monospace);
                                stack.push(Frame::CodeBlock);
                            }
                        }
                    }
                    Tag::HtmlBlock => {
                        let id = b.add_node(NodeKind::HtmlBlock, tr.clone(), parent);
                        for l in b.lines_of(&tr) {
                            b.out.lines[l].kind = BlockKind::Html;
                        }
                        parents.push(id);
                        stack.push(Frame::Block);
                    }
                    Tag::Table(_) => {
                        b.add_node(NodeKind::Table, tr.clone(), parent);
                        let lines = b.lines_of(&tr);
                        let quoted = b.out.lines[lines.start].quote_depth > 0;
                        let ti = b.out.tables.len() as u32;
                        for l in lines.clone() {
                            b.out.lines[l].kind = BlockKind::Table {
                                header: l == lines.start,
                            };
                            b.out.lines[l].table = Some(ti);
                        }
                        b.out.tables.push(TableBlock {
                            range: tr.clone(),
                            first_line: lines.start,
                            last_line: lines.end - 1,
                            quoted,
                        });
                        in_table += 1;
                        stack.push(Frame::Table);
                    }
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough => {
                        if in_table == 0 {
                            let (id, n) = match tag {
                                Tag::Emphasis => (StyleId::Emphasis, 1),
                                Tag::Strong => (StyleId::Strong, 2),
                                _ => {
                                    let n = text[tr.clone()]
                                        .bytes()
                                        .take_while(|c| *c == b'~')
                                        .count()
                                        .min(2);
                                    (StyleId::Strikethrough, n)
                                }
                            };
                            b.span(tr.clone(), id);
                            if tr.len() >= 2 * n {
                                b.hide(tr.start..tr.start + n);
                                b.hide(tr.end - n..tr.end);
                            }
                        }
                        stack.push(Frame::Inline);
                    }
                    Tag::Link { link_type, .. } => {
                        if in_table == 0 {
                            link(b, tr.clone(), link_type, false);
                        }
                        in_link += 1;
                        stack.push(Frame::Link);
                    }
                    Tag::Image { link_type, .. } => {
                        if in_table == 0 {
                            link(b, tr.clone(), link_type, true);
                        }
                        in_link += 1;
                        stack.push(Frame::Link);
                    }
                    _ => stack.push(Frame::Inline),
                }
            }
            Event::End(end) => match stack.pop() {
                Some(Frame::Block) => {
                    parents.pop();
                    if matches!(end, TagEnd::List(_)) {
                        list_ordered.pop();
                    }
                    if matches!(end, TagEnd::Item) {
                        item_lines.pop();
                    }
                }
                Some(Frame::CodeBlock) => {
                    in_code = false;
                    if let Some(fi) = cur_fence.take() {
                        fence_end(b, fi);
                    }
                }
                Some(Frame::Table) => in_table -= 1,
                Some(Frame::Link) => in_link -= 1,
                _ => {}
            },
            Event::Text(t) => {
                if in_code {
                    if let Some(fi) = cur_fence {
                        b.out.fences[fi].pieces.push(r.clone());
                    }
                    continue;
                }
                if in_table > 0 {
                    continue;
                }
                let src = &text[r.clone()];
                if src.starts_with('&') && src.ends_with(';') && src != t.as_ref() {
                    b.span(r.clone(), StyleId::String);
                }
                text_run = match text_run.take() {
                    Some(run) if run.end == r.start => Some(run.start..r.end),
                    Some(run) => {
                        let mut old = Some(run);
                        flush(b, &mut old, in_link);
                        Some(r.clone())
                    }
                    None => Some(r.clone()),
                };
            }
            Event::Code(_) => {
                b.out.code_ranges.push(r.clone());
                if in_table == 0 {
                    b.span(r.clone(), StyleId::Monospace);
                    let src = text[r.clone()].as_bytes();
                    let n = src.iter().take_while(|c| **c == b'`').count();
                    let m = src.iter().rev().take_while(|c| **c == b'`').count().min(n);
                    if n > 0 && r.len() > n {
                        b.hide(r.start..r.start + n);
                        b.hide(r.end - m..r.end);
                    }
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                if in_table == 0 {
                    html_spans(b, r.clone());
                }
            }
            Event::TaskListMarker(checked) => {
                let l = b.idx.line_of(r.start);
                b.item(r.clone(), l, InlineKind::Checkbox { checked });
                if let Some(&il) = item_lines.last()
                    && let BlockKind::ListItem { task, .. } = &mut b.out.lines[il].kind
                {
                    *task = Some(checked);
                }
            }
            Event::Rule => {
                let tr = trim_range(text, r.clone());
                b.add_node(NodeKind::Rule, tr.clone(), parent);
                let l = b.idx.line_of(tr.start);
                b.out.lines[l].kind = BlockKind::HorizontalRule;
                b.class(l, LineClass::HorizontalRule);
            }
            _ => {}
        }
    }
    flush(b, &mut text_run, in_link);
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn is_atx_start(text: &str, pos: usize) -> bool {
    text[pos..].trim_start_matches(' ').starts_with('#')
}

fn heading(b: &mut Builder, tr: Range<usize>, level: u8, setext: bool) {
    let text = b.text;
    let bytes = text.as_bytes();
    b.span(tr.clone(), StyleId::Heading);
    let lines = b.lines_of(&tr);
    if !setext {
        let l = lines.start;
        b.out.lines[l].kind = BlockKind::Heading {
            level,
            setext: false,
        };
        b.class(l, LineClass::Heading(level));
        b.out.lines[l].height = LineHeight::for_heading(level);
        let mut p = tr.start;
        while p < tr.end && bytes[p] == b' ' {
            p += 1;
        }
        let open_end = p + bytes[p..tr.end].iter().take_while(|c| **c == b'#').count();
        b.hide_with_space(p..open_end);
        // Closing sequence: `#`s preceded by a space or tab.
        let mut q = tr.end;
        while q > open_end && bytes[q - 1] == b'#' {
            q -= 1;
        }
        if q < tr.end && q > open_end && matches!(bytes[q - 1], b' ' | b'\t') {
            b.hide_with_space(q..tr.end);
        }
    } else {
        let under = lines.end - 1;
        for l in lines.start..under {
            b.out.lines[l].kind = BlockKind::Heading {
                level,
                setext: true,
            };
        }
        b.out.lines[under].kind = BlockKind::SetextUnderline { level };
        let lr = b.idx.line_range(under);
        let s = &text[lr.clone()];
        let lead = s.len() - s.trim_start().len();
        let mark_end = lr.start + s.trim_end().len();
        b.hide_with_space(lr.start + lead..mark_end);
    }
}

fn blockquote(b: &mut Builder, tr: Range<usize>) {
    let text = b.text;
    let bytes = text.as_bytes();
    b.span(tr.clone(), StyleId::Quote);
    let lines = b.lines_of(&tr);
    let first = b.idx.line_range(lines.start);
    let callout = CALLOUT_RE.captures(&text[first.clone()]).map(|c| {
        let kind = CalloutKind::parse(&c[3]).unwrap_or(CalloutKind::Note);
        let m = c.get(2).unwrap();
        (kind, first.start + m.start()..first.start + m.end())
    });
    for l in lines.clone() {
        b.out.lines[l].quote_depth += 1;
        match &callout {
            Some((kind, marker)) => {
                b.class(l, LineClass::Callout(*kind));
                if b.out.lines[l].callout.is_none() {
                    let title = if l == lines.start {
                        text[marker.end..first.end].trim().to_string()
                    } else {
                        String::new()
                    };
                    b.out.lines[l].callout = Some(CalloutLine {
                        kind: *kind,
                        title,
                        header: l == lines.start,
                    });
                }
            }
            None => b.class(l, LineClass::Blockquote),
        }
        // QuoteMarks: the leading `>` run(s) of each line.
        let lr = b.idx.line_range(l);
        let mut p = if l == lines.start { tr.start } else { lr.start };
        loop {
            while p < lr.end && matches!(bytes[p], b' ' | b'\t') {
                p += 1;
            }
            if p < lr.end && bytes[p] == b'>' {
                let already = b.out.lines[l].items.iter().any(|i| i.range.start == p);
                if !already {
                    b.hide_with_space(p..p + 1);
                }
                p += 1;
            } else {
                break;
            }
        }
    }
    if let Some((kind, marker)) = callout {
        b.item(marker, lines.start, InlineKind::CalloutHeader(kind));
    }
}

fn fence_start(b: &mut Builder, tr: Range<usize>) -> usize {
    let text = b.text;
    let bytes = text.as_bytes();
    let open_line = b.idx.line_of(tr.start);
    let last_line = b.idx.line_of(tr.end);
    let mut p = tr.start;
    while p < tr.end && bytes[p] == b' ' {
        p += 1;
    }
    let ch = bytes.get(p).copied().unwrap_or(b'`');
    let n = bytes[p..tr.end].iter().take_while(|c| **c == ch).count();
    b.hide(p..p + n);
    let ol = b.idx.line_range(open_line);
    let info_raw = &text[p + n..ol.end];
    let info = info_raw.trim().to_string();
    let info_range = (!info.is_empty()).then(|| {
        let lead = info_raw.len() - info_raw.trim_start().len();
        let s = p + n + lead;
        s..s + info.len()
    });
    // Closing fence?
    let mut closed = false;
    if last_line > open_line {
        let lr = b.idx.line_range(last_line);
        let s = &text[lr.clone()];
        let t = s.trim_end();
        let run = t.bytes().rev().take_while(|c| *c == ch).count();
        let prefix = &t[..t.len() - run];
        if run >= n && prefix.bytes().all(|c| matches!(c, b' ' | b'\t' | b'>')) {
            closed = true;
            b.hide(lr.start + prefix.len()..lr.start + t.len());
        }
    }
    let language = language_for_info(&info);
    let word: String = info.chars().take_while(|c| !c.is_whitespace()).collect();
    let fi = b.out.fences.len();
    for l in open_line..=last_line {
        let (kind, class) = if l == open_line {
            (
                BlockKind::FenceOpen {
                    language: word.clone(),
                    resolved: language.map(|d| d.name),
                },
                LineClass::FenceHeader,
            )
        } else if l == last_line {
            (
                if closed {
                    BlockKind::FenceClose
                } else {
                    BlockKind::FenceBody
                },
                LineClass::FenceFooter,
            )
        } else {
            (BlockKind::FenceBody, LineClass::FenceBody)
        };
        b.out.lines[l].kind = kind;
        b.class(l, class);
        b.out.lines[l].fence = Some(fi as u32);
    }
    b.out.fences.push(Fence {
        range: tr,
        open_line,
        last_line,
        closed,
        info,
        info_range,
        language,
        pieces: Vec::new(),
    });
    fi
}

fn fence_end(b: &mut Builder, fi: usize) {
    let f = &b.out.fences[fi];
    if f.language.and_then(|l| l.grammar).is_none() {
        let pieces = f.pieces.clone();
        for p in pieces {
            b.span(p, StyleId::Monospace);
        }
    }
}

/// Finds the `](` that closes the link text of `raw` (which starts at `[`).
fn link_text_end(raw: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = start;
    while i < raw.len() {
        match raw[i] {
            b'\\' => i += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn link(b: &mut Builder, tr: Range<usize>, link_type: LinkType, image: bool) {
    let text = b.text;
    let raw = &text[tr.clone()];
    let line = b.idx.line_of(tr.start);
    let single_line = !raw.contains('\n');
    match link_type {
        LinkType::Autolink | LinkType::Email => {
            b.span(tr.clone(), StyleId::Url);
            if raw.starts_with('<') && raw.ends_with('>') && raw.len() >= 2 {
                let url = raw[1..raw.len() - 1].to_string();
                b.item(tr, line, InlineKind::Autolink { url });
            }
        }
        LinkType::Inline => {
            b.span(tr.clone(), StyleId::Link);
            let bytes = raw.as_bytes();
            let open = if image { 1 } else { 0 };
            if let Some(close) = link_text_end(bytes, open)
                && bytes.get(close + 1) == Some(&b'(')
                && raw.ends_with(')')
            {
                let inner_end = raw.len() - 1;
                let mut p = close + 2;
                while p < inner_end && bytes[p].is_ascii_whitespace() {
                    p += 1;
                }
                let us = p;
                if bytes.get(p) == Some(&b'<') {
                    while p < inner_end && bytes[p] != b'>' {
                        p += 1;
                    }
                    p = (p + 1).min(inner_end);
                } else {
                    let mut depth = 0;
                    while p < inner_end && !bytes[p].is_ascii_whitespace() {
                        match bytes[p] {
                            b'(' => depth += 1,
                            b')' if depth == 0 => break,
                            b')' => depth -= 1,
                            b'\\' => p += 1,
                            _ => {}
                        }
                        p += 1;
                    }
                }
                let p = p.min(inner_end);
                if p > us {
                    b.span(tr.start + us..tr.start + p, StyleId::Url);
                }
                let mut t = p;
                while t < inner_end && bytes[t].is_ascii_whitespace() {
                    t += 1;
                }
                if t < inner_end && matches!(bytes[t], b'"' | b'\'' | b'(') {
                    let mut te = inner_end;
                    while te > t && bytes[te - 1].is_ascii_whitespace() {
                        te -= 1;
                    }
                    b.span(tr.start + t..tr.start + te, StyleId::String);
                }
            }
            if single_line {
                if image {
                    if b.inline_images
                        && let Some(c) = IMAGE_RE.captures(raw)
                    {
                        let url = c[2].split_whitespace().next().unwrap_or("").to_string();
                        b.item(
                            tr,
                            line,
                            InlineKind::Image {
                                alt: c[1].to_string(),
                                url,
                            },
                        );
                    }
                } else if let Some(c) = LINK_RE.captures(raw) {
                    b.item(
                        tr,
                        line,
                        InlineKind::Link {
                            text: c[1].to_string(),
                            url: c[2].to_string(),
                        },
                    );
                }
            }
        }
        _ => b.span(tr, StyleId::Link),
    }
}

/// A tiny HTML tokenizer standing in for lang-html's highlighting of
/// `HTMLBlock` / `HTMLTag` nodes.
fn html_spans(b: &mut Builder, r: Range<usize>) {
    let text = b.text;
    let s = &text.as_bytes()[..r.end];
    let mut i = r.start;
    let name_char = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'-' | b':' | b'_' | b'.');
    while i < r.end {
        if text[i..r.end].starts_with("<!--") {
            let end = text[i + 4..r.end]
                .find("-->")
                .map(|e| i + 4 + e + 3)
                .unwrap_or(r.end);
            b.span(i..end, StyleId::Comment);
            i = end;
            continue;
        }
        if s[i] == b'<' && i + 1 < r.end && (s[i + 1].is_ascii_alphabetic() || s[i + 1] == b'/') {
            let mut j = i + 1;
            if s[j] == b'/' {
                j += 1;
            }
            b.span(i..j, StyleId::Punctuation);
            let ns = j;
            while j < r.end && name_char(s[j]) {
                j += 1;
            }
            b.span(ns..j, StyleId::Type);
            loop {
                while j < r.end && s[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= r.end {
                    break;
                }
                if s[j] == b'>' {
                    b.span(j..j + 1, StyleId::Punctuation);
                    j += 1;
                    break;
                }
                if s[j] == b'/' && s.get(j + 1) == Some(&b'>') {
                    b.span(j..j + 2, StyleId::Punctuation);
                    j += 2;
                    break;
                }
                let an = j;
                while j < r.end
                    && !s[j].is_ascii_whitespace()
                    && !matches!(s[j], b'=' | b'>' | b'/')
                {
                    j += 1;
                }
                if j == an {
                    j += 1;
                    continue;
                }
                b.span(an..j, StyleId::Property);
                let mut k = j;
                while k < r.end && s[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < r.end && s[k] == b'=' {
                    b.span(k..k + 1, StyleId::Keyword);
                    k += 1;
                    while k < r.end && s[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    let vs = k;
                    if k < r.end && matches!(s[k], b'"' | b'\'') {
                        let q = s[k];
                        k += 1;
                        while k < r.end && s[k] != q {
                            k += 1;
                        }
                        k = (k + 1).min(r.end);
                    } else {
                        while k < r.end && !s[k].is_ascii_whitespace() && s[k] != b'>' {
                            k += 1;
                        }
                    }
                    b.span(vs..k, StyleId::String);
                    j = k;
                }
            }
            i = j;
            continue;
        }
        if s[i] == b'&' {
            let mut j = i + 1;
            while j < r.end && (s[j].is_ascii_alphanumeric() || s[j] == b'#') {
                j += 1;
            }
            if j < r.end && s[j] == b';' && j > i + 1 {
                b.span(i..j + 1, StyleId::String);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

impl Parsed {
    /// True if `pos` is inside a code node (`from <= pos < to`).
    pub fn is_inside_code(&self, pos: usize) -> bool {
        let i = self.code_ranges.partition_point(|r| r.start <= pos);
        i > 0 && pos < self.code_ranges[i - 1].end
    }
}
