//! A Lezer-like Markdown syntax tree (block + inline nodes with their
//! marks) for the structural selection commands: CodeMirror
//! `selectParentSyntax` (Alt-o / Alt-i through the engine's history) and
//! cm-helix `moveToSibling` (Alt-n / Alt-p).
//!
//! Built from pulldown-cmark offset events. Node names follow
//! `@lezer/markdown` (`Paragraph`, `ATXHeading`, `HeaderMark`, `ListItem`,
//! `ListMark`, `Emphasis`, `EmphasisMark`, `Link`, `LinkMark`, `URL`,
//! `InlineCode`, `CodeMark`, `FencedCode`, `CodeInfo`, `CodeText`, `Table`,
//! `TableCell`, `TableDelimiter`, ...). Deviations: tight list items get a
//! synthetic `Paragraph` (Lezer always has one), a task item keeps its
//! `TaskMarker` inside that paragraph instead of a `Task` node, escapes and
//! entities are not nodes, and fenced code has no nested language tree.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};

use crate::syntax::parser_options;

/// One node; `range` is in bytes. Node 0 is the `Document`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub name: &'static str,
    pub range: Range<usize>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct SyntaxTree {
    pub nodes: Vec<TreeNode>,
}

fn trim_end(text: &str, r: Range<usize>) -> Range<usize> {
    let b = text.as_bytes();
    let mut end = r.end.min(text.len());
    while end > r.start && matches!(b[end - 1], b' ' | b'\t' | b'\n' | b'\r') {
        end -= 1;
    }
    r.start..end
}

fn run_len(text: &str, at: usize, ch: u8, max: usize) -> usize {
    text.as_bytes()[at.min(text.len())..].iter().take(max).take_while(|c| **c == ch).count()
}

fn line_ranges(text: &str, r: Range<usize>) -> Vec<Range<usize>> {
    let end = r.end.min(text.len());
    let mut out = Vec::new();
    let mut s = r.start.min(end);
    loop {
        let e = text[s..end].find('\n').map_or(end, |i| s + i);
        out.push(s..e);
        if e >= end {
            break;
        }
        s = e + 1;
    }
    out
}

struct Spans<'a> {
    text: &'a str,
    spans: Vec<(&'static str, Range<usize>)>,
}

impl Spans<'_> {
    fn add(&mut self, name: &'static str, r: Range<usize>) {
        if r.start < r.end && r.end <= self.text.len() {
            self.spans.push((name, r));
        }
    }
}

enum Open {
    Block(&'static str, Range<usize>),
    Inline(&'static str, Range<usize>, usize),
    Code { range: Range<usize>, body: Option<Range<usize>> },
    Item,
}

impl SyntaxTree {
    /// Parses `text` (frontmatter becomes one `Frontmatter` node).
    pub fn parse(text: &str) -> Self {
        let mut s = Spans { text, spans: Vec::new() };
        let mut cut = text.len().min(20_000);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut off = 0;
        if let Some(fm) = fuchico_core::frontmatter::parse_frontmatter(&text[..cut]) {
            s.add("Frontmatter", trim_end(text, 0..fm.body_start));
            off = fm.body_start;
        }
        body(&mut s, off);
        Self::build(text.len(), s.spans)
    }

    fn build(len: usize, mut spans: Vec<(&'static str, Range<usize>)>) -> Self {
        // Stable: equal ranges keep their (outer-first) emission order.
        spans.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(b.1.end.cmp(&a.1.end)));
        let mut nodes =
            vec![TreeNode { name: "Document", range: 0..len, parent: None, children: Vec::new() }];
        let mut stack = vec![0usize];
        for (name, r) in spans {
            while stack.len() > 1 {
                let top = &nodes[*stack.last().unwrap()].range;
                if top.start <= r.start && r.end <= top.end {
                    break;
                }
                stack.pop();
            }
            let parent = *stack.last().unwrap();
            let id = nodes.len();
            nodes.push(TreeNode { name, range: r, parent: Some(parent), children: Vec::new() });
            nodes[parent].children.push(id);
            stack.push(id);
        }
        Self { nodes }
    }

    /// Lezer `resolveStack(pos, side)`: the nodes covering `pos`, innermost
    /// first, ending with the document. `side > 0` needs `from <= pos < to`,
    /// `side < 0` needs `from < pos <= to`, `0` either.
    pub fn resolve_stack(&self, pos: usize, side: i8) -> Vec<usize> {
        let covers = |r: &Range<usize>| match side.signum() {
            1 => r.start <= pos && pos < r.end,
            -1 => r.start < pos && pos <= r.end,
            _ => r.start <= pos && pos <= r.end,
        };
        let mut out = vec![0];
        let mut cur = 0;
        'down: loop {
            for &c in &self.nodes[cur].children {
                if covers(&self.nodes[c].range) {
                    out.push(c);
                    cur = c;
                    continue 'down;
                }
            }
            break;
        }
        out.reverse();
        out
    }

    /// CodeMirror `selectParentSyntax` for one range: the innermost
    /// non-top node strictly enclosing `from..to`.
    pub fn select_parent(&self, from: usize, to: usize) -> Option<Range<usize>> {
        let mut stack = self.resolve_stack(from, 1);
        if from == to {
            let before = self.resolve_stack(from, -1);
            let (b, a) = (&self.nodes[before[0]].range, &self.nodes[stack[0]].range);
            if b.start >= a.start && b.end <= a.end {
                stack = before;
            }
        }
        for (i, &id) in stack.iter().enumerate() {
            let r = &self.nodes[id].range;
            let encloses = (r.start < from && r.end >= to) || (r.end > to && r.start <= from);
            if encloses && i + 1 < stack.len() {
                return Some(r.clone());
            }
        }
        None
    }

    /// cm-helix `moveToSibling`: walking out from the innermost node at
    /// `from`, the first covering node with a next/previous sibling.
    pub fn sibling(&self, from: usize, to: usize, forward: bool) -> Option<Range<usize>> {
        for id in self.resolve_stack(from, 1) {
            let n = &self.nodes[id];
            if n.range.start > from || n.range.end < to {
                continue;
            }
            let Some(p) = n.parent else { continue };
            let sibs = &self.nodes[p].children;
            let i = sibs.iter().position(|c| *c == id)?;
            let j = if forward { sibs.get(i + 1) } else { i.checked_sub(1).and_then(|k| sibs.get(k)) };
            if let Some(&j) = j {
                return Some(self.nodes[j].range.clone());
            }
        }
        None
    }
}

fn inline_marks(s: &mut Spans, name: &'static str, r: &Range<usize>, children_end: usize) {
    let t = s.text;
    let b = t.as_bytes();
    let mark = |s: &mut Spans, n: &'static str, len: usize| {
        if len > 0 && r.end - r.start >= 2 * len {
            s.add(n, r.start..r.start + len);
            s.add(n, r.end - len..r.end);
        }
    };
    match name {
        "Emphasis" => mark(s, "EmphasisMark", 1),
        "StrongEmphasis" => mark(s, "EmphasisMark", 2),
        "Strikethrough" => mark(s, "StrikethroughMark", run_len(t, r.start, b'~', 2)),
        "Superscript" => mark(s, "SuperscriptMark", 1),
        "Subscript" => mark(s, "SubscriptMark", 1),
        "Link" | "Image" => {
            let open = if name == "Image" { 2 } else { 1 };
            if b.get(r.start) == Some(&b'<') {
                // Autolink.
                mark(s, "LinkMark", 1);
                s.add("URL", r.start + 1..r.end - 1);
                return;
            }
            s.add("LinkMark", r.start..r.start + open);
            let label_end = children_end.max(r.start + open);
            if b.get(label_end) == Some(&b']') {
                s.add("LinkMark", label_end..label_end + 1);
                if b.get(label_end + 1) == Some(&b'(') && b.get(r.end - 1) == Some(&b')') {
                    s.add("LinkMark", label_end + 1..label_end + 2);
                    let mut u = label_end + 2;
                    while u < r.end && matches!(b[u], b' ' | b'\t' | b'\n') {
                        u += 1;
                    }
                    let mut e = u;
                    while e < r.end - 1 && !matches!(b[e], b' ' | b'\t' | b'\n') {
                        e += 1;
                    }
                    s.add("URL", u..e);
                    s.add("LinkMark", r.end - 1..r.end);
                }
            }
        }
        _ => {}
    }
}

fn body(s: &mut Spans, off: usize) {
    let text = s.text;
    let mut open: Vec<Open> = Vec::new();
    // Synthetic paragraph of a tight list item.
    let mut tight: Option<Range<usize>> = None;
    let mut quote_depth = 0usize;

    let inline_depth = |open: &[Open]| open.iter().filter(|o| matches!(o, Open::Inline(..))).count();
    let close_tight = |s: &mut Spans, tight: &mut Option<Range<usize>>| {
        if let Some(r) = tight.take() {
            s.add("Paragraph", trim_end(s.text, r));
        }
    };

    for (ev, r) in Parser::new_ext(&text[off..], parser_options()).into_offset_iter() {
        let r = r.start + off..r.end + off;
        let is_inline = match &ev {
            Event::Start(t) => matches!(
                t,
                Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::Superscript
                    | Tag::Subscript
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            ),
            Event::End(_) | Event::TaskListMarker(_) | Event::Rule => false,
            _ => true,
        };
        // Inline content directly in a list item opens a synthetic paragraph.
        if is_inline && matches!(open.last(), Some(Open::Item)) {
            let t = tight.get_or_insert(r.start..r.end);
            t.end = t.end.max(r.end);
        } else if let Some(t) = &mut tight
            && (inline_depth(&open) > 0 || is_inline)
        {
            t.end = t.end.max(r.end);
        }
        // Inline children extend the enclosing link's label end.
        if let Some(Open::Inline(_, _, end)) = open.last_mut()
            && !matches!(ev, Event::End(_))
        {
            *end = (*end).max(r.end);
        }
        match ev {
            Event::Start(tag) => {
                let tr = trim_end(text, r.clone());
                if !is_inline && inline_depth(&open) == 0 {
                    close_tight(s, &mut tight);
                }
                match tag {
                    Tag::Paragraph => open.push(Open::Block("Paragraph", tr)),
                    Tag::Heading { .. } => {
                        let atx = text.as_bytes().get(tr.start) == Some(&b'#');
                        if atx {
                            let n = run_len(text, tr.start, b'#', 6);
                            s.add("HeaderMark", tr.start..tr.start + n);
                            let line = &text[tr.clone()];
                            let tail = line.len() - line.trim_end_matches('#').len();
                            if tail > 0 && tr.end - tail > tr.start + n {
                                let before = text.as_bytes()[tr.end - tail - 1];
                                if before == b' ' || before == b'\t' {
                                    s.add("HeaderMark", tr.end - tail..tr.end);
                                }
                            }
                            open.push(Open::Block("ATXHeading", tr));
                        } else {
                            if let Some(last) = line_ranges(text, tr.clone()).last() {
                                let lt = text[last.clone()].trim_start();
                                s.add("HeaderMark", last.end - lt.len()..last.end);
                            }
                            open.push(Open::Block("SetextHeading", tr));
                        }
                    }
                    Tag::BlockQuote(_) => {
                        quote_depth += 1;
                        for lr in line_ranges(text, tr.clone()) {
                            let mut seen = 0;
                            for (i, c) in text[lr.clone()].bytes().enumerate() {
                                match c {
                                    b'>' => {
                                        seen += 1;
                                        if seen == quote_depth {
                                            s.add("QuoteMark", lr.start + i..lr.start + i + 1);
                                            break;
                                        }
                                    }
                                    b' ' | b'\t' => {}
                                    _ => break,
                                }
                            }
                        }
                        open.push(Open::Block("Blockquote", tr));
                    }
                    Tag::List(start) => {
                        open.push(Open::Block(if start.is_some() { "OrderedList" } else { "BulletList" }, tr))
                    }
                    Tag::Item => {
                        let n = text[tr.clone()].bytes().take_while(|c| !c.is_ascii_whitespace()).count();
                        s.add("ListMark", tr.start..tr.start + n);
                        s.add("ListItem", tr);
                        open.push(Open::Item);
                    }
                    Tag::CodeBlock(kind) => {
                        if let CodeBlockKind::Fenced(_) = kind {
                            let lines = line_ranges(text, tr.clone());
                            let first = lines[0].clone();
                            let lead = text[first.clone()].len() - text[first.clone()].trim_start().len();
                            let at = first.start + lead;
                            let ch = text.as_bytes().get(at).copied().unwrap_or(b'`');
                            let n = run_len(text, at, ch, usize::MAX);
                            s.add("CodeMark", at..at + n);
                            let info = text[at + n..first.end].trim();
                            if !info.is_empty() {
                                let i = at + n + text[at + n..first.end].find(info).unwrap_or(0);
                                s.add("CodeInfo", i..i + info.len());
                            }
                            if lines.len() > 1 {
                                let last = lines.last().unwrap().clone();
                                let lt = text[last.clone()].trim();
                                if lt.len() >= n && lt.bytes().all(|c| c == ch) {
                                    let i = last.start + text[last.clone()].find(lt).unwrap_or(0);
                                    s.add("CodeMark", i..i + lt.len());
                                }
                            }
                            open.push(Open::Code { range: tr, body: None });
                        } else {
                            open.push(Open::Code { range: tr, body: None });
                        }
                    }
                    Tag::HtmlBlock => open.push(Open::Block("HTMLBlock", tr)),
                    Tag::Table(_) => open.push(Open::Block("Table", tr)),
                    Tag::TableHead => {
                        let first = line_ranges(text, tr.clone())[0].clone();
                        if first.end < text.len()
                            && let Some(d) = line_ranges(text, first.end + 1..text.len()).first()
                        {
                            let dt = text[d.clone()].trim();
                            let i = d.start + text[d.clone()].find(dt).unwrap_or(0);
                            s.add("TableDelimiter", i..i + dt.len());
                        }
                        open.push(Open::Block("TableHeader", trim_end(text, first)))
                    }
                    Tag::TableRow => open.push(Open::Block("TableRow", tr)),
                    Tag::TableCell => {
                        let c = &text[tr.clone()];
                        let lead = c.len() - c.trim_start().len();
                        open.push(Open::Block("TableCell", tr.start + lead..tr.end))
                    }
                    Tag::Emphasis => open.push(Open::Inline("Emphasis", tr, 0)),
                    Tag::Strong => open.push(Open::Inline("StrongEmphasis", tr, 0)),
                    Tag::Strikethrough => open.push(Open::Inline("Strikethrough", tr, 0)),
                    Tag::Superscript => open.push(Open::Inline("Superscript", tr, 0)),
                    Tag::Subscript => open.push(Open::Inline("Subscript", tr, 0)),
                    Tag::Link { link_type, .. } => {
                        let name = if matches!(link_type, LinkType::Autolink | LinkType::Email) {
                            "Autolink"
                        } else {
                            "Link"
                        };
                        open.push(Open::Inline(name, tr, 0))
                    }
                    Tag::Image { .. } => open.push(Open::Inline("Image", tr, 0)),
                    _ => open.push(Open::Block("Block", tr)),
                }
            }
            Event::End(end) => {
                match open.pop() {
                    Some(Open::Block(name, r)) => {
                        if matches!(end, TagEnd::BlockQuote(_)) {
                            quote_depth -= 1;
                        }
                        if name == "TableHeader" || name == "TableRow" {
                            // Pipes outside the cells.
                            let cells: Vec<Range<usize>> = s
                                .spans
                                .iter()
                                .filter(|(n, c)| *n == "TableCell" && c.start >= r.start && c.end <= r.end)
                                .map(|(_, c)| c.clone())
                                .collect();
                            for (i, c) in text[r.clone()].bytes().enumerate() {
                                let p = r.start + i;
                                if c == b'|' && !cells.iter().any(|x| x.start <= p && p < x.end) {
                                    s.add("TableDelimiter", p..p + 1);
                                }
                            }
                        }
                        if name != "Block" {
                            s.add(name, r);
                        }
                    }
                    Some(Open::Inline(name, r, children_end)) => {
                        inline_marks(s, if name == "Autolink" { "Link" } else { name }, &r, children_end);
                        s.add(name, r);
                    }
                    Some(Open::Code { range, body }) => {
                        if let Some(b) = body {
                            s.add("CodeText", trim_end(text, b));
                        }
                        let fenced = s.spans.iter().any(|(n, c)| *n == "CodeMark" && c.start == range.start)
                            || text[range.clone()].trim_start().starts_with(['`', '~']);
                        s.add(if fenced { "FencedCode" } else { "CodeBlock" }, range);
                    }
                    Some(Open::Item) => close_tight(s, &mut tight),
                    None => {}
                }
            }
            Event::Text(_) => {
                if let Some(Open::Code { body, .. }) = open.last_mut() {
                    let b = body.get_or_insert(r.clone());
                    b.end = r.end;
                }
            }
            Event::Code(_) => {
                let tr = trim_end(text, r);
                let n = run_len(text, tr.start, b'`', usize::MAX);
                if tr.len() >= 2 * n {
                    s.add("CodeMark", tr.start..tr.start + n);
                    s.add("CodeMark", tr.end - n..tr.end);
                }
                s.add("InlineCode", tr);
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                if !matches!(open.last(), Some(Open::Block("HTMLBlock", _))) {
                    s.add("HTMLTag", trim_end(text, r));
                }
            }
            Event::HardBreak => s.add("HardBreak", r),
            Event::Rule => s.add("HorizontalRule", trim_end(text, r)),
            Event::TaskListMarker(_) => s.add("TaskMarker", trim_end(text, r)),
            Event::FootnoteReference(_) => s.add("FootnoteReference", r),
            _ => {}
        }
    }
    close_tight(s, &mut tight);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(t: &SyntaxTree, src: &str, pos: usize) -> Vec<String> {
        t.resolve_stack(pos, 1)
            .iter()
            .map(|&i| format!("{}:{}", t.nodes[i].name, &src[t.nodes[i].range.clone()]))
            .collect()
    }

    #[test]
    fn inline_and_block_nodes() {
        let src = "# Title *x*\n\n- a **b** [l](u)\n- c\n";
        let t = SyntaxTree::parse(src);
        let at = |s: &str| src.find(s).unwrap();
        assert_eq!(names(&t, src, at("x")), ["Emphasis:*x*", "ATXHeading:# Title *x*", &("Document:".to_string() + src)]);
        assert_eq!(names(&t, src, 0)[0], "HeaderMark:#");
        let b = names(&t, src, at("b"));
        assert_eq!(&b[..4], ["StrongEmphasis:**b**", "Paragraph:a **b** [l](u)", "ListItem:- a **b** [l](u)", "BulletList:- a **b** [l](u)\n- c"]);
        assert_eq!(names(&t, src, at("u)"))[0], "URL:u");
        assert_eq!(names(&t, src, at("- c"))[0], "ListMark:-");
    }

    #[test]
    fn parent_and_sibling() {
        let src = "para one\n\n- a *b* c\n- d\n";
        let t = SyntaxTree::parse(src);
        let b = src.find('b').unwrap();
        let r = t.select_parent(b, b + 1).unwrap();
        assert_eq!(&src[r.clone()], "*b*");
        let r = t.select_parent(r.start, r.end).unwrap();
        assert_eq!(&src[r.clone()], "a *b* c");
        let r = t.select_parent(r.start, r.end).unwrap();
        assert_eq!(&src[r.clone()], "- a *b* c");
        let r = t.select_parent(r.start, r.end).unwrap();
        assert_eq!(&src[r.clone()], "- a *b* c\n- d");
        // The document is the top node: never selected.
        assert_eq!(t.select_parent(r.start, r.end), None);

        let p = t.sibling(0, 0, true).unwrap();
        assert_eq!(&src[p], "- a *b* c\n- d");
        assert_eq!(t.sibling(0, 0, false), None);
        let item = src.find("- d").unwrap() + 2;
        assert_eq!(&src[t.sibling(item, item, false).unwrap()], "-");
    }

    #[test]
    fn fenced_code_and_tables() {
        let src = "```rust\nfn x() {}\n```\n\n| a | b |\n| - | - |\n| 1 | 2 |\n";
        let t = SyntaxTree::parse(src);
        let at = |s: &str| src.find(s).unwrap();
        assert_eq!(&names(&t, src, at("fn"))[..2], ["CodeText:fn x() {}", "FencedCode:```rust\nfn x() {}\n```"]);
        assert_eq!(names(&t, src, at("rust"))[0], "CodeInfo:rust");
        assert_eq!(names(&t, src, at("2"))[..2], ["TableCell:2".to_string(), "TableRow:| 1 | 2 |".into()]);
        assert_eq!(names(&t, src, at("| a"))[0], "TableDelimiter:|");
    }
}
