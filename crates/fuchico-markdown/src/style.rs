//! Semantic style ids, line classes and their resolved look.
//!
//! The model never emits colours. It emits semantic ids that mirror the
//! original app's CSS classes (`cm-md-h1`, `cm-callout-note`, ...) and its
//! `HighlightStyle` tags, and [`StyleSet::resolve`] turns a set of ids into
//! a host-neutral description (colour *token*, weight, italic, ...), using the
//! same precedence as the CSS cascade did.

use std::ops::Range;

/// Theme colour tokens, named after the original CSS variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorToken {
    /// `--foreground`
    Foreground,
    /// `--muted-foreground`
    Muted,
    /// `--destructive`
    Destructive,
    /// `--primary`
    Primary,
    /// `--syntax-comment`
    SyntaxComment,
    /// `--syntax-string`
    SyntaxString,
    /// `--syntax-number`
    SyntaxNumber,
    /// `--syntax-keyword`
    SyntaxKeyword,
    /// `--syntax-function`
    SyntaxFunction,
    /// `--syntax-type`
    SyntaxType,
}

/// One semantic style. The discriminant order is the **precedence order**
/// used by [`StyleSet::resolve`] for colour: a later id wins, exactly like
/// the later rule of the original `HighlightStyle.define([...])` won in the
/// CSS cascade. `FrontmatterKey` is last because it came from a theme class
/// with higher specificity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum StyleId {
    /// `t.comment`: comment colour, italic.
    Comment = 0,
    /// `t.string` and sub-tags (`character`, `attributeValue`, `LinkTitle`,
    /// Markdown entities and `:emoji:`).
    String,
    /// `t.number`, `t.bool`, `t.null`, `t.atom`.
    Number,
    /// `t.keyword`, control/operator keywords and `t.operator`.
    Keyword,
    /// `t.function(variableName | propertyName)`.
    Function,
    /// `t.className`, `t.typeName`, `t.tagName`.
    Type,
    /// `t.definition(variableName)`, `t.propertyName`, `t.attributeName`
    /// (plain foreground).
    Property,
    /// `t.punctuation` (muted).
    Punctuation,
    /// `t.heading` (bold). The per-level colour comes from the line class.
    Heading,
    /// `t.link`: function colour + underline.
    Link,
    /// `t.url`: function colour.
    Url,
    /// `t.emphasis`: italic.
    Emphasis,
    /// `t.strong`: bold.
    Strong,
    /// `t.strikethrough`: line-through.
    Strikethrough,
    /// `t.quote`: muted.
    Quote,
    /// `t.monospace`: string colour + mono font (inline code, unknown-language
    /// and indented code blocks).
    Monospace,
    /// `t.invalid`: destructive colour.
    Invalid,
    /// `.cm-frontmatter-key`: keyword colour at 0.85 opacity.
    FrontmatterKey,
}

impl StyleId {
    pub const ALL: [StyleId; 18] = [
        StyleId::Comment,
        StyleId::String,
        StyleId::Number,
        StyleId::Keyword,
        StyleId::Function,
        StyleId::Type,
        StyleId::Property,
        StyleId::Punctuation,
        StyleId::Heading,
        StyleId::Link,
        StyleId::Url,
        StyleId::Emphasis,
        StyleId::Strong,
        StyleId::Strikethrough,
        StyleId::Quote,
        StyleId::Monospace,
        StyleId::Invalid,
        StyleId::FrontmatterKey,
    ];

    /// Stable short name (used in golden tests and debugging).
    pub fn name(self) -> &'static str {
        match self {
            StyleId::Comment => "comment",
            StyleId::String => "string",
            StyleId::Number => "number",
            StyleId::Keyword => "keyword",
            StyleId::Function => "function",
            StyleId::Type => "type",
            StyleId::Property => "property",
            StyleId::Punctuation => "punctuation",
            StyleId::Heading => "heading",
            StyleId::Link => "link",
            StyleId::Url => "url",
            StyleId::Emphasis => "em",
            StyleId::Strong => "strong",
            StyleId::Strikethrough => "strike",
            StyleId::Quote => "quote",
            StyleId::Monospace => "mono",
            StyleId::Invalid => "invalid",
            StyleId::FrontmatterKey => "fm-key",
        }
    }

    fn color(self) -> Option<ColorToken> {
        Some(match self {
            StyleId::Comment => ColorToken::SyntaxComment,
            StyleId::String | StyleId::Monospace => ColorToken::SyntaxString,
            StyleId::Number => ColorToken::SyntaxNumber,
            StyleId::Keyword | StyleId::FrontmatterKey => ColorToken::SyntaxKeyword,
            StyleId::Function | StyleId::Link | StyleId::Url => ColorToken::SyntaxFunction,
            StyleId::Type => ColorToken::SyntaxType,
            StyleId::Property => ColorToken::Foreground,
            StyleId::Punctuation | StyleId::Quote => ColorToken::Muted,
            StyleId::Invalid => ColorToken::Destructive,
            StyleId::Heading | StyleId::Emphasis | StyleId::Strong | StyleId::Strikethrough => {
                return None;
            }
        })
    }
}

/// A set of [`StyleId`]s (bitset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StyleSet(pub u32);

impl StyleSet {
    pub const EMPTY: StyleSet = StyleSet(0);

    pub fn single(id: StyleId) -> Self {
        StyleSet(1 << id as u8)
    }

    pub fn with(self, id: StyleId) -> Self {
        StyleSet(self.0 | 1 << id as u8)
    }

    pub fn contains(self, id: StyleId) -> bool {
        self.0 & (1 << id as u8) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn union(self, other: StyleSet) -> Self {
        StyleSet(self.0 | other.0)
    }

    pub fn iter(self) -> impl Iterator<Item = StyleId> {
        StyleId::ALL
            .into_iter()
            .filter(move |id| self.contains(*id))
    }

    /// Space-separated names, for tests and debugging.
    pub fn names(self) -> String {
        self.iter().map(StyleId::name).collect::<Vec<_>>().join(" ")
    }

    /// Resolves the set into a concrete text style (colour precedence =
    /// [`StyleId`] order, later wins).
    pub fn resolve(self) -> ResolvedStyle {
        let mut r = ResolvedStyle::default();
        for id in self.iter() {
            if let Some(c) = id.color() {
                r.color = Some(c);
            }
            match id {
                StyleId::Comment | StyleId::Emphasis => r.italic = true,
                StyleId::Heading | StyleId::Strong => r.bold = true,
                StyleId::Link => r.underline = true,
                StyleId::Strikethrough => r.strikethrough = true,
                StyleId::Monospace => r.monospace = true,
                StyleId::FrontmatterKey => r.opacity = Some(0.85),
                _ => {}
            }
        }
        r
    }
}

/// Host-neutral text style produced by [`StyleSet::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResolvedStyle {
    /// `None` = inherit (line colour, e.g. a heading level colour).
    pub color: Option<ColorToken>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub monospace: bool,
    pub opacity: Option<f32>,
}

/// A styled byte range (non-overlapping, sorted, within one line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRun {
    pub range: Range<usize>,
    pub styles: StyleSet,
}

/// The five callout types (`> [!TYPE]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalloutKind {
    Note,
    Tip,
    Warning,
    Important,
    Caution,
}

impl CalloutKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "NOTE" => CalloutKind::Note,
            "TIP" => CalloutKind::Tip,
            "WARNING" => CalloutKind::Warning,
            "IMPORTANT" => CalloutKind::Important,
            "CAUTION" => CalloutKind::Caution,
            _ => return None,
        })
    }

    /// `CALLOUT_META` icon.
    pub fn icon(self) -> &'static str {
        match self {
            CalloutKind::Note => "ℹ",
            CalloutKind::Tip => "💡",
            CalloutKind::Warning => "⚠",
            CalloutKind::Important => "📌",
            CalloutKind::Caution => "🔥",
        }
    }

    /// `CALLOUT_META` label.
    pub fn label(self) -> &'static str {
        match self {
            CalloutKind::Note => "Note",
            CalloutKind::Tip => "Tip",
            CalloutKind::Warning => "Warning",
            CalloutKind::Important => "Important",
            CalloutKind::Caution => "Caution",
        }
    }

    /// Lower-case type, as in the `cm-callout-<type>` class.
    pub fn slug(self) -> &'static str {
        match self {
            CalloutKind::Note => "note",
            CalloutKind::Tip => "tip",
            CalloutKind::Warning => "warning",
            CalloutKind::Important => "important",
            CalloutKind::Caution => "caution",
        }
    }

    /// Accent colour: background `color-mix(accent 8%|10%)` and the 3px left
    /// border; the header text uses it too.
    pub fn accent(self) -> ColorToken {
        match self {
            CalloutKind::Note => ColorToken::SyntaxFunction,
            CalloutKind::Tip => ColorToken::SyntaxString,
            CalloutKind::Warning => ColorToken::SyntaxNumber,
            CalloutKind::Important => ColorToken::SyntaxKeyword,
            CalloutKind::Caution => ColorToken::Destructive,
        }
    }

    /// Background mix percentage (`8` or `10`).
    pub fn background_percent(self) -> u8 {
        match self {
            CalloutKind::Warning | CalloutKind::Caution => 10,
            _ => 8,
        }
    }
}

/// Line-level classes, one per original `Decoration.line({class})`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineClass {
    /// `cm-md-h1`..`cm-md-h6` (ATX headings only, cursor on or off).
    Heading(u8),
    /// `cm-md-blockquote`.
    Blockquote,
    /// `cm-callout cm-callout-<type>` (replaces `Blockquote`).
    Callout(CalloutKind),
    /// `cm-md-hr`.
    HorizontalRule,
    /// `cm-fence-header` / `-body` / `-footer`.
    FenceHeader,
    FenceBody,
    FenceFooter,
    /// `cm-frontmatter-line`.
    FrontmatterLine,
    /// `cm-frontmatter-fence` (first and closing line).
    FrontmatterFence,
    /// `cm-region-start-line` / `-end-line` / `-body-line`.
    RegionStart,
    RegionEnd,
    RegionBody,
}

impl LineClass {
    /// The original CSS class string.
    pub fn css(self) -> String {
        match self {
            LineClass::Heading(n) => format!("cm-md-h{n}"),
            LineClass::Blockquote => "cm-md-blockquote".into(),
            LineClass::Callout(k) => format!("cm-callout cm-callout-{}", k.slug()),
            LineClass::HorizontalRule => "cm-md-hr".into(),
            LineClass::FenceHeader => "cm-fence-header".into(),
            LineClass::FenceBody => "cm-fence-body".into(),
            LineClass::FenceFooter => "cm-fence-footer".into(),
            LineClass::FrontmatterLine => "cm-frontmatter-line".into(),
            LineClass::FrontmatterFence => "cm-frontmatter-fence".into(),
            LineClass::RegionStart => "cm-region-start-line".into(),
            LineClass::RegionEnd => "cm-region-end-line".into(),
            LineClass::RegionBody => "cm-region-body-line".into(),
        }
    }
}

/// Line height / font-size class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineHeight {
    #[default]
    Body,
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
}

/// Concrete metrics of a [`LineHeight`] class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    /// Font size relative to the editor font (`em`).
    pub font_scale: f32,
    /// Line height relative to this line's font size.
    pub line_height: f32,
    pub bold: bool,
    /// Line text colour (`None` = editor foreground).
    pub color: Option<ColorToken>,
    pub opacity: f32,
}

impl LineHeight {
    pub fn for_heading(level: u8) -> Self {
        match level {
            1 => LineHeight::H1,
            2 => LineHeight::H2,
            3 => LineHeight::H3,
            4 => LineHeight::H4,
            5 => LineHeight::H5,
            _ => LineHeight::H6,
        }
    }

    /// Metrics from the original `chromeTheme` (`.cm-md-hN`, body 1.55).
    pub fn metrics(self) -> LineMetrics {
        let m = |font_scale, line_height, color, opacity| LineMetrics {
            font_scale,
            line_height,
            bold: !matches!(self, LineHeight::Body),
            color,
            opacity,
        };
        match self {
            LineHeight::Body => m(1.0, 1.55, None, 1.0),
            LineHeight::H1 => m(1.6, 1.4, Some(ColorToken::Destructive), 1.0),
            LineHeight::H2 => m(1.4, 1.4, Some(ColorToken::SyntaxNumber), 1.0),
            LineHeight::H3 => m(1.2, 1.4, Some(ColorToken::SyntaxKeyword), 1.0),
            LineHeight::H4 => m(1.08, 1.55, Some(ColorToken::SyntaxString), 1.0),
            LineHeight::H5 => m(1.0, 1.55, Some(ColorToken::SyntaxString), 0.85),
            LineHeight::H6 => m(1.0, 1.55, Some(ColorToken::SyntaxString), 0.65),
        }
    }
}

/// Flattens possibly-overlapping `(range, id)` spans into sorted,
/// non-overlapping runs whose style is the union of the covering spans.
pub fn flatten_spans(spans: &[(Range<usize>, StyleId)]) -> Vec<StyleRun> {
    if spans.is_empty() {
        return Vec::new();
    }
    let mut points: Vec<usize> = Vec::with_capacity(spans.len() * 2);
    for (r, _) in spans {
        if r.start < r.end {
            points.push(r.start);
            points.push(r.end);
        }
    }
    points.sort_unstable();
    points.dedup();
    let mut runs: Vec<StyleRun> = Vec::new();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut set = StyleSet::EMPTY;
        for (r, id) in spans {
            if r.start <= a && r.end >= b {
                set = set.with(*id);
            }
        }
        if set.is_empty() {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.range.end == a && last.styles == set => last.range.end = b,
            _ => runs.push(StyleRun {
                range: a..b,
                styles: set,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_follows_highlight_style_order() {
        // Link inside quote: quote (muted) comes later than link -> muted.
        let s = StyleSet::single(StyleId::Link).with(StyleId::Quote);
        let r = s.resolve();
        assert_eq!(r.color, Some(ColorToken::Muted));
        assert!(r.underline);
        // String vs link (LinkTitle inside Link): link wins.
        let s = StyleSet::single(StyleId::String).with(StyleId::Link);
        assert_eq!(s.resolve().color, Some(ColorToken::SyntaxFunction));
        let fm = StyleSet::single(StyleId::FrontmatterKey).resolve();
        assert_eq!(fm.opacity, Some(0.85));
    }

    #[test]
    fn flatten_merges_overlaps() {
        let runs = flatten_spans(&[(0..10, StyleId::Strong), (2..4, StyleId::Emphasis)]);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].range, 2..4);
        assert!(
            runs[1].styles.contains(StyleId::Strong) && runs[1].styles.contains(StyleId::Emphasis)
        );
    }
}
