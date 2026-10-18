//! Display model of one source line: the text actually drawn (hidden
//! syntax removed, widgets replaced by their label) plus the mapping
//! between source bytes and display bytes. Pure (no gpui), so it is unit
//! tested directly.

use std::ops::Range;

use fuchico_markdown::style::LineMetrics;

use fuchico_markdown::{
    CalloutKind, ColorToken, FoldPlaceholder, LineDecoration, LineEndWidget, TextEdit as MdEdit,
    Widget,
};

/// What a display range stands for.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetKind {
    Checkbox { checked: bool, toggle: MdEdit },
    Link { url: String },
    Wikilink { target: String, resolved: Option<String> },
    Tag { tag: String },
    CalloutHeader { kind: CalloutKind },
    Image { alt: String, url: String },
    RegionStart { line_from: usize },
    RegionEnd,
    /// A fold placeholder; `range` is the folded source range.
    Fold { range: Range<usize>, region: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineWidget {
    pub disp: Range<usize>,
    pub src: Range<usize>,
    pub kind: WidgetKind,
}

/// Paint attributes of a display range.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpanStyle {
    pub color: Option<ColorToken>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub opacity: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub disp: Range<usize>,
    pub style: SpanStyle,
}

/// Source ↔ display mapping piece.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Absolute source byte range.
    pub src: Range<usize>,
    pub disp: Range<usize>,
    /// Text segments map byte-for-byte; others are atomic.
    pub text: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayLine {
    /// 0-based source line.
    pub line: usize,
    /// Source byte range of the line (no newline).
    pub src: Range<usize>,
    pub text: String,
    pub spans: Vec<Span>,
    pub segments: Vec<Segment>,
    pub widgets: Vec<InlineWidget>,
    pub line_end: Vec<LineEndWidget>,
    pub metrics: LineMetrics,
}

fn style_of(r: fuchico_markdown::ResolvedStyle) -> SpanStyle {
    SpanStyle {
        color: r.color,
        bold: r.bold,
        italic: r.italic,
        underline: r.underline,
        strikethrough: r.strikethrough,
        opacity: r.opacity,
    }
}

/// Label drawn for a widget, its style and kind (`None` = hidden).
fn widget_label(w: &Widget, doc_slice: &str) -> Option<(String, SpanStyle, WidgetKind)> {
    let plain = SpanStyle::default();
    Some(match w {
        // Two cells: the 14px box is painted over them.
        Widget::Checkbox { checked, toggle } => {
            ("  ".into(), plain, WidgetKind::Checkbox { checked: *checked, toggle: toggle.clone() })
        }
        Widget::Link { text, url } => (
            if text.is_empty() { url.clone() } else { text.clone() },
            SpanStyle { color: Some(ColorToken::SyntaxFunction), underline: true, ..plain },
            WidgetKind::Link { url: url.clone() },
        ),
        Widget::Wikilink { label, target, resolved, .. } => (
            label.clone(),
            SpanStyle {
                color: Some(if resolved.is_some() { ColorToken::SyntaxFunction } else { ColorToken::Muted }),
                ..plain
            },
            WidgetKind::Wikilink { target: target.clone(), resolved: resolved.clone() },
        ),
        Widget::Tag { tag, .. } => (
            format!("#{tag}"),
            SpanStyle { color: Some(ColorToken::SyntaxKeyword), ..plain },
            WidgetKind::Tag { tag: tag.clone() },
        ),
        Widget::CalloutHeader { kind, text } => (
            text.to_uppercase(),
            SpanStyle { color: Some(kind.accent()), bold: true, ..plain },
            WidgetKind::CalloutHeader { kind: *kind },
        ),
        Widget::Image { alt, url } => (
            format!("🖼 {}", if alt.is_empty() { url } else { alt }),
            SpanStyle { color: Some(ColorToken::Muted), ..plain },
            WidgetKind::Image { alt: alt.clone(), url: url.clone() },
        ),
        Widget::RegionStart { name, line_from } => (
            format!("▾ 📁 {name}"),
            SpanStyle { color: Some(ColorToken::SyntaxFunction), bold: true, ..plain },
            WidgetKind::RegionStart { line_from: *line_from },
        ),
        Widget::RegionEnd => ("      ".into(), plain, WidgetKind::RegionEnd),
        Widget::RegionMarkerHidden => return None,
        Widget::Fold(p) => {
            let _ = doc_slice;
            match p {
                FoldPlaceholder::Ellipsis => (
                    " … ".into(),
                    SpanStyle { color: Some(ColorToken::Muted), ..plain },
                    WidgetKind::Fold { range: 0..0, region: false },
                ),
                FoldPlaceholder::Region { name, .. } => (
                    format!(" ▸ 📁 {name}  {} ", p.count_label().unwrap_or_default()),
                    SpanStyle { color: Some(ColorToken::SyntaxFunction), bold: true, ..plain },
                    WidgetKind::Fold { range: 0..0, region: true },
                ),
            }
        }
    })
}

impl DisplayLine {
    /// Builds the display line of `dec` over `doc` (the whole document).
    pub fn build(doc: &str, dec: &LineDecoration) -> DisplayLine {
        let metrics = dec.height.metrics();
        let src = dec.range.clone();
        let mut text = String::with_capacity(src.len());
        let mut spans: Vec<Span> = Vec::new();
        let mut segments: Vec<Segment> = Vec::new();
        let mut widgets: Vec<InlineWidget> = Vec::new();

        // Text pieces: split src range by style runs.
        let push_text = |from: usize, to: usize, text: &mut String, spans: &mut Vec<Span>, segs: &mut Vec<Segment>| {
            if from >= to {
                return;
            }
            let d0 = text.len();
            text.push_str(&doc[from..to]);
            segs.push(Segment { src: from..to, disp: d0..text.len(), text: true });
            for run in &dec.styles {
                let s = run.range.start.max(from);
                let e = run.range.end.min(to);
                if s < e {
                    spans.push(Span { disp: d0 + (s - from)..d0 + (e - from), style: style_of(run.styles.resolve()) });
                }
            }
        };

        let mut pos = src.start;
        for rep in &dec.replacements {
            let rs = rep.range.start.max(pos);
            if rs > src.end {
                break;
            }
            push_text(pos, rs.min(src.end), &mut text, &mut spans, &mut segments);
            let label = rep.widget.as_ref().and_then(|w| widget_label(w, ""));
            let src_end = rep.range.end.min(src.end).max(rs);
            let d0 = text.len();
            if let Some((label, style, mut kind)) = label {
                if let WidgetKind::Fold { range, .. } = &mut kind {
                    *range = rep.range.clone();
                }
                text.push_str(&label);
                spans.push(Span { disp: d0..text.len(), style });
                widgets.push(InlineWidget { disp: d0..text.len(), src: rs..src_end, kind });
            }
            segments.push(Segment { src: rs..src_end, disp: d0..text.len(), text: false });
            pos = rep.range.end.max(rs);
        }
        if pos < src.end {
            push_text(pos, src.end, &mut text, &mut spans, &mut segments);
        }
        if segments.is_empty() {
            segments.push(Segment { src: src.start..src.start, disp: 0..0, text: true });
        }
        DisplayLine { line: dec.line, src, text, spans, segments, widgets, line_end: dec.line_end.clone(), metrics }
    }

    /// Display byte of a source byte. Inside an atomic piece: its start
    /// (or its end when `pos` is at the piece's end).
    pub fn disp_of(&self, pos: usize) -> usize {
        for seg in &self.segments {
            if pos < seg.src.start {
                return seg.disp.start;
            }
            if pos <= seg.src.end {
                if seg.text {
                    return seg.disp.start + (pos - seg.src.start);
                }
                if pos == seg.src.end && seg.src.end > seg.src.start {
                    return seg.disp.end;
                }
                return seg.disp.start;
            }
        }
        self.text.len()
    }

    /// Source byte of a display byte.
    pub fn src_of(&self, disp: usize) -> usize {
        for seg in &self.segments {
            if disp < seg.disp.end || (disp == seg.disp.end && seg.text) {
                if disp < seg.disp.start {
                    return seg.src.start;
                }
                if seg.text {
                    return seg.src.start + (disp - seg.disp.start);
                }
                // atomic: nearest edge
                return if disp - seg.disp.start <= (seg.disp.end - seg.disp.start) / 2 {
                    seg.src.start
                } else {
                    seg.src.end
                };
            }
        }
        self.src.end
    }

    pub fn widget_at(&self, disp: usize) -> Option<&InlineWidget> {
        self.widgets.iter().find(|w| w.disp.start <= disp && disp < w.disp.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuchico_markdown::{DecorationContext, FoldState, MarkdownModel, Settings};

    fn line(doc: &str, l: usize, cursor: Option<usize>) -> DisplayLine {
        let m = MarkdownModel::new(doc, Settings::default());
        let folds = FoldState::new();
        let ctx = DecorationContext::new(cursor, &folds);
        let decs = m.decorations(0..m.line_count(), &ctx);
        DisplayLine::build(doc, decs.get(l).unwrap())
    }

    #[test]
    fn heading_marker_hidden_off_cursor() {
        let d = line("# Title\n\nx", 0, Some(2));
        assert_eq!(d.text, "Title");
        assert_eq!(d.disp_of(2), 0);
        assert_eq!(d.src_of(0), 2);
        let d = line("# Title\n\nx", 0, Some(0));
        assert_eq!(d.text, "# Title");
    }

    #[test]
    fn bold_markers_hidden_and_mapping() {
        let doc = "a **b** c\n\nz";
        let d = line(doc, 0, Some(2));
        assert_eq!(d.text, "a b c");
        assert_eq!(d.disp_of(4), 2); // 'b'
        assert_eq!(d.src_of(2), 4);
        assert!(d.spans.iter().any(|s| s.style.bold));
    }

    #[test]
    fn checkbox_widget() {
        let doc = "- [ ] task\n\nz";
        let d = line(doc, 0, Some(2));
        let w = d.widgets.iter().find(|w| matches!(w.kind, WidgetKind::Checkbox { .. })).unwrap();
        assert_eq!(&d.text[w.disp.clone()], "  ");
    }

    #[test]
    fn wikilink_label() {
        let doc = "see [[Note|label]] ok\n\nz";
        let d = line(doc, 0, Some(2));
        assert_eq!(d.text, "see label ok", "{:?}", d.segments);
        let w = &d.widgets[0];
        assert!(matches!(&w.kind, WidgetKind::Wikilink { target, .. } if target == "Note"));
    }
}
