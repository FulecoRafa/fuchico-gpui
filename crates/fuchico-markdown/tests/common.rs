#![allow(dead_code)]
use fuchico_markdown::*;

pub fn model(doc: &str) -> MarkdownModel {
    MarkdownModel::new(doc, Settings::default())
}

pub fn widget_name(w: &Widget) -> String {
    match w {
        Widget::Checkbox { checked, .. } => format!("[cb:{}]", if *checked { "x" } else { " " }),
        Widget::Link { text, url } => format!("[link:{text}->{url}]"),
        Widget::Wikilink {
            label, resolved, ..
        } => {
            format!(
                "[wiki:{label}{}]",
                if resolved.is_some() { "" } else { "?" }
            )
        }
        Widget::Tag { tag, .. } => format!("[tag:{tag}]"),
        Widget::CalloutHeader { text, .. } => format!("[callout:{text}]"),
        Widget::Image { alt, url } => format!("[img:{alt}->{url}]"),
        Widget::RegionStart { name, .. } => format!("[region:{name}]"),
        Widget::RegionEnd => "[region-end]".into(),
        Widget::RegionMarkerHidden => "[region-hidden]".into(),
        Widget::Fold(FoldPlaceholder::Ellipsis) => "[…]".into(),
        Widget::Fold(FoldPlaceholder::Region { name, line_count }) => {
            format!("[fold:{name}:{line_count}]")
        }
    }
}

/// Displayed text of a line: hidden ranges removed, widgets as `[..]`.
pub fn display(m: &MarkdownModel, d: &LineDecoration) -> String {
    if d.hidden != Hidden::No {
        return format!("<hidden:{:?}>", d.hidden);
    }
    if let Some(BlockWidget::Table { model, .. }) = &d.block {
        return format!("<table {}x{}>", model.rows.len() + 1, model.header.len());
    }
    let text = m.text();
    let mut out = String::new();
    let mut pos = d.range.start;
    for r in &d.replacements {
        if r.range.start > pos {
            out.push_str(&text[pos..r.range.start.min(d.range.end)]);
        }
        if let Some(w) = &r.widget {
            out.push_str(&widget_name(w));
        }
        pos = pos.max(r.range.end);
    }
    if pos < d.range.end {
        out.push_str(&text[pos..d.range.end]);
    }
    out
}

pub fn render(doc: &str, cursor: Option<usize>) -> Vec<String> {
    let m = model(doc);
    render_model(&m, cursor, &FoldState::new())
}

pub fn render_model(m: &MarkdownModel, cursor: Option<usize>, folds: &FoldState) -> Vec<String> {
    let files = vec!["/v/Note A.md".to_string()];
    let ctx = DecorationContext {
        cursor_line: cursor,
        folds,
        vault_files: &files,
        table_edit: None,
    };
    let d = m.decorations(0..m.line_count(), &ctx);
    d.lines.iter().map(|l| display(m, l)).collect()
}

pub fn classes(doc: &str, line: usize) -> Vec<String> {
    let m = model(doc);
    let fs = FoldState::new();
    let d = m.decorations(line..line + 1, &DecorationContext::new(None, &fs));
    d.lines[0].classes.iter().map(|c| c.css()).collect()
}

/// Text covered by each style run of `line`, with its style names.
pub fn styles(doc: &str, line: usize) -> Vec<(String, String)> {
    let m = model(doc);
    let fs = FoldState::new();
    let d = m.decorations(line..line + 1, &DecorationContext::new(None, &fs));
    d.lines[0]
        .styles
        .iter()
        .map(|s| (doc[s.range.clone()].to_string(), s.styles.names()))
        .collect()
}
