//! The cursor-dependent decoration pass: turns the static parse facts into
//! the per-line live-preview model (what every original ViewPlugin /
//! StateField produced), for a window of visible lines.

use std::collections::HashMap;
use std::ops::Range;

use fuchico_core::parse_tags::match_inline_tags_in_line;
use fuchico_core::wikilinks::{find_wikilinks, resolve_wikilink_target};

use crate::fold::{FoldPlaceholder, FoldState};
use crate::languages::toolbar_options;
use crate::style::{flatten_spans, CalloutKind, LineClass, LineHeight, StyleId, StyleRun};
use crate::syntax::{BlockKind, CalloutLine, InlineKind};
use crate::table::{TableEditState, TableModel};
use crate::text::TextEdit;
use crate::MarkdownModel;

/// Inputs that change without the text changing.
#[derive(Clone, Copy)]
pub struct DecorationContext<'a> {
    /// 0-based line of the main selection head (`None` = editor unfocused /
    /// no cursor: everything is rendered).
    pub cursor_line: Option<usize>,
    pub folds: &'a FoldState,
    /// Vault paths, for wikilink resolution.
    pub vault_files: &'a [String],
    /// Active table cell (the table widget's edit state), if any.
    pub table_edit: Option<TableEditState>,
}

impl<'a> DecorationContext<'a> {
    pub fn new(cursor_line: Option<usize>, folds: &'a FoldState) -> Self {
        Self { cursor_line, folds, vault_files: &[], table_edit: None }
    }
}

/// An inline widget that replaces a source range.
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    /// `cm-task-checkbox`; clicking applies `toggle`.
    Checkbox { checked: bool, toggle: TextEdit },
    /// `cm-md-link-widget` (`[text](url)` and `<url>` autolinks).
    Link { text: String, url: String },
    /// `cm-wikilink` (+ `cm-wikilink-unresolved` when `resolved` is None).
    Wikilink { label: String, target: String, resolved: Option<String>, title: String },
    /// `cm-tag` pill showing `#tag`.
    Tag { tag: String, title: String },
    /// `cm-callout-header`, text = `"<icon> <label>"`.
    CalloutHeader { kind: CalloutKind, text: String },
    /// Inline image preview (only with `Settings::inline_images`).
    Image { alt: String, url: String },
    /// `cm-region-bar-start`: ▾ chevron, 📁, name. Clicking the chevron
    /// folds `foldable(line of line_from)`.
    RegionStart { name: String, line_from: usize },
    /// `cm-region-bar-end`.
    RegionEnd,
    /// `cm-region-marker-hidden` (start marker of a folded region).
    RegionMarkerHidden,
    /// Fold placeholder: the `…` pill or the region pill. The replaced
    /// range spans lines (from the fold line's end into the last folded
    /// line).
    Fold(FoldPlaceholder),
}

/// A replaced source range; `widget: None` means the text is just hidden.
#[derive(Debug, Clone, PartialEq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub widget: Option<Widget>,
}

/// The code-block toolbar (language picker + copy button), drawn at the
/// end of the opening fence line.
#[derive(Debug, Clone, PartialEq)]
pub struct FenceToolbar {
    pub fence: usize,
    /// Current language text (the trimmed info string).
    pub language: String,
    /// Options for the language picker (current one inserted if unknown).
    pub options: Vec<String>,
    /// Range a picked language replaces.
    pub lang_range: Range<usize>,
    /// Text the copy button copies.
    pub copy_text: String,
}

impl FenceToolbar {
    /// The edit for picking `lang` in the language menu.
    pub fn set_language(&self, lang: &str) -> TextEdit {
        TextEdit::new(self.lang_range.clone(), lang)
    }
}

/// Widgets anchored at the end of a line (`side: 1`).
#[derive(Debug, Clone, PartialEq)]
pub enum LineEndWidget {
    FenceToolbar(FenceToolbar),
    /// `cm-mermaid-preview-btn` "▶ Preview diagram"; `anchor` (fence start)
    /// is the block key, `text` the diagram source.
    MermaidButton { anchor: usize, text: String },
}

/// Block widget replacing whole lines.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockWidget {
    /// The table widget (always shown, cursor or not).
    Table { index: usize, model: TableModel, edit: Option<TableEditState> },
}

/// Why a line is not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hidden {
    #[default]
    No,
    /// Covered by a block widget starting on an earlier line.
    Block,
    /// Inside a folded range.
    Folded,
}

/// Everything needed to draw one line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineDecoration {
    pub line: usize,
    pub range: Range<usize>,
    pub kind: BlockKind,
    pub quote_depth: u8,
    pub callout: Option<CalloutLine>,
    pub list_depth: u8,
    pub classes: Vec<LineClass>,
    pub height: LineHeight,
    /// Indent guide columns (in display columns).
    pub indent_guides: Vec<usize>,
    /// Non-overlapping style runs, sorted (absolute byte ranges).
    pub styles: Vec<StyleRun>,
    /// Non-overlapping replacements, sorted.
    pub replacements: Vec<Replacement>,
    pub line_end: Vec<LineEndWidget>,
    pub block: Option<BlockWidget>,
    pub hidden: Hidden,
}

/// Decorations for a window of lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineDecorations {
    pub first_line: usize,
    pub lines: Vec<LineDecoration>,
}

impl LineDecorations {
    pub fn get(&self, line: usize) -> Option<&LineDecoration> {
        line.checked_sub(self.first_line).and_then(|i| self.lines.get(i))
    }
}

/// The edit a checkbox click makes (markdownStyle's CheckboxWidget).
pub fn checkbox_toggle(doc: &str, pos: usize) -> TextEdit {
    let was = matches!(doc.get(pos..pos + 3), Some("[x]") | Some("[X]"));
    TextEdit::new(pos..(pos + 3).min(doc.len()), if was { "[ ]" } else { "[x]" })
}

#[derive(Default, Clone)]
struct RegionLine {
    classes: Vec<LineClass>,
    replacement: Option<Replacement>,
}

fn leading_width(s: &str, tab: usize) -> Option<usize> {
    if s.trim().is_empty() {
        return None;
    }
    let mut w = 0;
    for c in s.chars() {
        match c {
            ' ' => w += 1,
            '\t' => w += tab - w % tab,
            _ => break,
        }
    }
    Some(w)
}

const GUIDE_SCAN: usize = 200;

impl MarkdownModel {
    fn line_text(&self, l: usize) -> &str {
        &self.text[self.lines.line_range(l)]
    }

    /// Indent guide columns for `line`: one per `tab_size` step of leading
    /// whitespace; blank lines take the smaller indent of their nearest
    /// non-blank neighbours.
    pub fn indent_guides(&self, line: usize) -> Vec<usize> {
        let tab = self.settings.tab_size.max(1);
        let width = match leading_width(self.line_text(line), tab) {
            Some(w) => w,
            None => {
                let prev = (line.saturating_sub(GUIDE_SCAN)..line)
                    .rev()
                    .find_map(|l| leading_width(self.line_text(l), tab));
                let next = (line + 1..(line + GUIDE_SCAN).min(self.lines.len()))
                    .find_map(|l| leading_width(self.line_text(l), tab));
                match (prev, next) {
                    (Some(a), Some(b)) => a.min(b),
                    _ => 0,
                }
            }
        };
        (0..).map(|k| k * tab).take_while(|c| *c < width).collect()
    }

    fn region_lines(&self, ctx: &DecorationContext, visible: &Range<usize>) -> HashMap<usize, RegionLine> {
        let mut out = HashMap::new();
        let (start, end) = (&self.settings.fold_start_marker, &self.settings.fold_end_marker);
        if start.trim().is_empty() || end.trim().is_empty() {
            return out;
        }
        let mut depth = 0usize;
        let mut skip_until: Option<usize> = None;
        for n in 0..self.lines.len().min(visible.end) {
            if skip_until.is_some_and(|s| n <= s) {
                continue;
            }
            let lr = self.lines.line_range(n);
            let trimmed = self.text[lr.clone()].trim_start();
            let on_cursor = ctx.cursor_line == Some(n);
            let mut rl = RegionLine::default();
            if let Some(rest) = trimmed.strip_prefix(start.as_str()) {
                let name = rest.trim();
                let name = if name.is_empty() { "Region" } else { name };
                if let Some(f) = ctx.folds.folded_at(lr.end) {
                    skip_until = Some(self.lines.line_of(f.end));
                    if !on_cursor {
                        rl.replacement = Some(Replacement { range: lr.clone(), widget: Some(Widget::RegionMarkerHidden) });
                    }
                } else {
                    if !on_cursor {
                        rl.classes.push(LineClass::RegionStart);
                        rl.replacement = Some(Replacement {
                            range: lr.clone(),
                            widget: Some(Widget::RegionStart { name: name.to_string(), line_from: lr.start }),
                        });
                    }
                    depth += 1;
                }
            } else if trimmed.starts_with(end.as_str()) {
                depth = depth.saturating_sub(1);
                if !on_cursor {
                    rl.classes.push(LineClass::RegionEnd);
                    rl.replacement = Some(Replacement { range: lr.clone(), widget: Some(Widget::RegionEnd) });
                }
            } else if depth > 0 {
                rl.classes.push(LineClass::RegionBody);
            }
            if n >= visible.start && (rl.replacement.is_some() || !rl.classes.is_empty()) {
                out.insert(n, rl);
            }
        }
        out
    }

    /// Syntect spans of fence `fi`, absolute offsets.
    fn fence_spans(&self, fi: usize) -> Vec<(Range<usize>, StyleId)> {
        let f = &self.parsed.fences[fi];
        let Some(grammar) = f.language.and_then(|l| l.grammar) else { return Vec::new() };
        let mut code = String::new();
        let mut map: Vec<(usize, usize, usize)> = Vec::new(); // (code_off, abs, len)
        for p in &f.pieces {
            map.push((code.len(), p.start, p.len()));
            code.push_str(&self.text[p.clone()]);
        }
        let spans = self.snippets.get(grammar, &code);
        let mut out = Vec::with_capacity(spans.len());
        for (r, id) in spans.iter() {
            let mut i = map.partition_point(|m| m.0 + m.2 <= r.start);
            while i < map.len() && map[i].0 < r.end {
                let (co, abs, len) = map[i];
                let a = r.start.max(co);
                let b = r.end.min(co + len);
                if a < b {
                    out.push((abs + a - co..abs + b - co, *id));
                }
                i += 1;
            }
        }
        out
    }

    fn fence_widgets(&self, fi: usize) -> Vec<LineEndWidget> {
        let f = &self.parsed.fences[fi];
        let text = &self.text;
        let open = self.lines.line_range(f.open_line);
        let closing = self.lines.line_range(self.lines.line_of(f.range.end));
        let mut out = Vec::new();
        let lang_range = match &f.info_range {
            Some(r) => r.clone(),
            None => {
                let b = text.as_bytes();
                let ch = b.get(f.range.start).copied().unwrap_or(b'`');
                let run = b[f.range.start..open.end].iter().take_while(|c| **c == ch).count();
                let s = (f.range.start + run).min(open.end);
                s..open.end
            }
        };
        let body_from = open.end + 1;
        let copy_text = if body_from < closing.start {
            text[body_from..closing.start].trim_end().to_string()
        } else {
            String::new()
        };
        out.push(LineEndWidget::FenceToolbar(FenceToolbar {
            fence: fi,
            language: f.info.clone(),
            options: toolbar_options(&f.info),
            lang_range,
            copy_text,
        }));
        if f.is_mermaid() {
            let to = closing.start.max(body_from).min(text.len());
            let from = body_from.min(to);
            out.push(LineEndWidget::MermaidButton { anchor: f.range.start, text: text[from..to].to_string() });
        }
        out
    }

    /// The live-preview decorations of `visible` lines (0-based, clamped).
    pub fn decorations(&self, visible: Range<usize>, ctx: &DecorationContext) -> LineDecorations {
        let n = self.lines.len();
        let visible = visible.start.min(n)..visible.end.min(n);
        let p = &self.parsed;

        // Folded lines and placeholders.
        let mut folded: Vec<(usize, Range<usize>)> = Vec::new(); // (start line, range)
        let mut fold_hidden: Vec<Range<usize>> = Vec::new(); // hidden line ranges
        for r in ctx.folds.ranges() {
            if r.end > self.text.len() {
                continue;
            }
            let l0 = self.lines.line_of(r.start);
            if fold_hidden.iter().any(|h| h.contains(&l0)) {
                continue;
            }
            let l1 = self.lines.line_of(r.end);
            folded.push((l0, r.clone()));
            if l1 > l0 {
                fold_hidden.push(l0 + 1..l1 + 1);
            }
        }
        let is_folded = |l: usize| fold_hidden.iter().any(|h| h.contains(&l));

        let regions = self.region_lines(ctx, &visible);
        let mut fence_cache: HashMap<usize, Vec<(Range<usize>, StyleId)>> = HashMap::new();
        let mut out = LineDecorations { first_line: visible.start, lines: Vec::with_capacity(visible.len()) };

        for l in visible.clone() {
            let info = &p.lines[l];
            let lr = self.lines.line_range(l);
            let on_cursor = ctx.cursor_line == Some(l);
            let mut d = LineDecoration {
                line: l,
                range: lr.clone(),
                kind: info.kind.clone(),
                quote_depth: info.quote_depth,
                callout: info.callout.clone(),
                list_depth: info.list_depth,
                classes: info.classes.clone(),
                height: info.height,
                indent_guides: Vec::new(),
                styles: Vec::new(),
                replacements: Vec::new(),
                line_end: Vec::new(),
                block: None,
                hidden: Hidden::No,
            };
            if is_folded(l) {
                d.hidden = Hidden::Folded;
                out.lines.push(d);
                continue;
            }
            if let Some(ti) = info.table {
                let t = &p.tables[ti as usize];
                if l == t.first_line {
                    let model = TableModel::parse(&self.text, t.range.clone(), t.quoted);
                    let edit = ctx.table_edit.filter(|e| e.anchor == t.range.start);
                    d.block = Some(BlockWidget::Table { index: ti as usize, model, edit });
                } else {
                    d.hidden = Hidden::Block;
                }
                out.lines.push(d);
                continue;
            }
            d.indent_guides = self.indent_guides(l);

            // Styles.
            let mut spans = info.spans.clone();
            if let Some(fi) = info.fence {
                let fs = fence_cache.entry(fi as usize).or_insert_with(|| self.fence_spans(fi as usize));
                let a = fs.partition_point(|(r, _)| r.end <= lr.start);
                spans.extend(fs[a..].iter().take_while(|(r, _)| r.start < lr.end.max(lr.start + 1)).filter_map(|(r, id)| {
                    let (s, e) = (r.start.max(lr.start), r.end.min(lr.end));
                    (s < e).then_some((s..e, *id))
                }));
                if l == p.fences[fi as usize].open_line {
                    d.line_end = self.fence_widgets(fi as usize);
                }
            }
            d.styles = flatten_spans(&spans);

            // Replacements.
            let mut reps: Vec<Replacement> = Vec::new();
            for it in &info.items {
                let revealed = ctx.cursor_line == Some(it.line);
                let widget = match &it.kind {
                    InlineKind::Checkbox { checked } => Some(Widget::Checkbox {
                        checked: *checked,
                        toggle: checkbox_toggle(&self.text, it.range.start),
                    }),
                    _ if revealed => continue,
                    InlineKind::Hide => None,
                    InlineKind::Link { text, url } => Some(Widget::Link { text: text.clone(), url: url.clone() }),
                    InlineKind::Autolink { url } => Some(Widget::Link { text: url.clone(), url: url.clone() }),
                    InlineKind::CalloutHeader(k) => {
                        Some(Widget::CalloutHeader { kind: *k, text: format!("{} {}", k.icon(), k.label()) })
                    }
                    InlineKind::Image { alt, url } => Some(Widget::Image { alt: alt.clone(), url: url.clone() }),
                };
                reps.push(Replacement { range: it.range.clone(), widget });
            }
            if !on_cursor {
                let line_text = &self.text[lr.clone()];
                for w in find_wikilinks(line_text) {
                    let start = lr.start + w.start;
                    if p.is_inside_code(start) {
                        continue;
                    }
                    let resolved = resolve_wikilink_target(&w.target, ctx.vault_files);
                    let title = resolved.clone().unwrap_or_else(|| format!("No note named \"{}\"", w.label));
                    reps.push(Replacement {
                        range: start..lr.start + w.end,
                        widget: Some(Widget::Wikilink { label: w.label, target: w.target, resolved, title }),
                    });
                }
                for t in match_inline_tags_in_line(line_text) {
                    let start = lr.start + t.from;
                    if p.is_inside_code(start) {
                        continue;
                    }
                    let title = format!("Filter notes tagged #{}", t.tag);
                    reps.push(Replacement { range: start..lr.start + t.to, widget: Some(Widget::Tag { tag: t.tag, title }) });
                }
            }
            if let Some(rl) = regions.get(&l) {
                for c in &rl.classes {
                    if !d.classes.contains(c) {
                        d.classes.push(*c);
                    }
                }
                if let Some(r) = &rl.replacement {
                    reps.push(r.clone());
                }
            }
            for (fl, r) in &folded {
                if *fl == l {
                    reps.push(Replacement { range: r.clone(), widget: Some(Widget::Fold(self.fold_placeholder(r))) });
                }
            }
            // Resolve overlaps: earliest first, longer first, drop overlaps.
            reps.sort_by(|a, b| a.range.start.cmp(&b.range.start).then(b.range.len().cmp(&a.range.len())));
            let mut kept: Vec<Replacement> = Vec::with_capacity(reps.len());
            for r in reps {
                if let Some(last) = kept.last() {
                    let overlaps = r.range.start < last.range.end
                        || (r.range.is_empty() && r.range.start == last.range.start && last.range.is_empty());
                    if overlaps {
                        continue;
                    }
                }
                kept.push(r);
            }
            d.replacements = kept;
            out.lines.push(d);
        }
        out
    }
}
