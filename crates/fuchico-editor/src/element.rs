//! Layout + paint of the editor text. `layout_frame` (prepaint) decorates
//! and shapes **only the visible lines** and turns everything into a flat
//! list of paint primitives plus hit regions ([`Frame`]); `paint` just
//! replays the primitives. The IME bridge (`EntityInputHandler`) lives
//! here too because it needs the frame geometry.

use std::ops::Range;
use std::rc::Rc;

use fuchico_helix::{ModeType, Scroll, ScrollAlign};
use fuchico_markdown::{
    BlockWidget, DecorationContext, FenceToolbar, Hidden, LineClass, LineDecoration, LineEndWidget,
    TableModel, TextEdit as MdEdit,
};
use gpui_kit::{
    App, BorderStyle, Bounds, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, Font, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, PathBuilder,
    Pixels, Point, ShapedLine, SharedString, StrikethroughStyle, Style, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, fill, point, px, quad, relative, size,
};

use crate::display::{DisplayLine, SpanStyle, WidgetKind};
use crate::style::{EditorStyle, mix};
use crate::view::{EditorView, InsertCursor, LineNumbers, Overlay};

/// Something clickable in the last frame.
#[derive(Debug, Clone)]
pub(crate) enum Hit {
    Checkbox(MdEdit),
    Link(String),
    Wikilink {
        target: String,
        resolved: Option<String>,
    },
    Tag(String),
    /// Fold chevron of a 0-based line.
    FoldGutter(usize),
    /// Region start chevron (byte of the region start line).
    RegionChevron(usize),
    /// Fold placeholder (folded byte range).
    FoldPill(Range<usize>),
    Copy {
        fence: usize,
        text: String,
    },
    Lang(FenceToolbar),
    Mermaid {
        anchor: usize,
    },
    TableCell {
        anchor: usize,
        row: usize,
        col: usize,
    },
    TableAddRow {
        anchor: usize,
    },
    TableAddCol {
        anchor: usize,
    },
}

#[allow(clippy::large_enum_variant)] // a short-lived per-frame paint list
pub(crate) enum Prim {
    Quad(PaintQuad),
    Text {
        line: Rc<WrappedLine>,
        origin: Point<Pixels>,
        lh: Pixels,
    },
    Shaped {
        line: ShapedLine,
        origin: Point<Pixels>,
        lh: Pixels,
    },
    Path {
        points: Vec<Point<Pixels>>,
        width: Pixels,
        color: Hsla,
    },
    Underline {
        origin: Point<Pixels>,
        width: Pixels,
        style: UnderlineStyle,
    },
}

/// A laid-out visible line.
pub(crate) struct FrameLine {
    pub line: usize,
    pub top: Pixels,
    pub height: Pixels,
    pub row_h: Pixels,
    pub x: Pixels,
    pub disp: DisplayLine,
    pub layout: Option<Rc<WrappedLine>>,
    /// Display byte where each visual row starts (first = 0).
    pub row_starts: Vec<usize>,
    pub table: bool,
}

impl FrameLine {
    fn ux(&self, i: usize) -> Pixels {
        self.layout
            .as_ref()
            .map_or(px(0.), |l| l.unwrapped_layout.x_for_index(i))
    }
    fn row_of(&self, i: usize) -> usize {
        self.row_starts.iter().rposition(|&s| s <= i).unwrap_or(0)
    }
    /// Position (relative to the line origin) of display byte `i`.
    pub fn pos(&self, i: usize) -> Point<Pixels> {
        let r = self.row_of(i);
        point(
            self.ux(i) - self.ux(self.row_starts[r]),
            self.row_h * r as f32,
        )
    }
    fn len(&self) -> usize {
        self.disp.text.len()
    }
    fn row_end(&self, r: usize) -> usize {
        self.row_starts.get(r + 1).copied().unwrap_or(self.len())
    }
    /// Width of the char at display byte `i` (or `fallback` at the end).
    fn advance(&self, i: usize, fallback: Pixels) -> Pixels {
        let t = &self.disp.text;
        match t[i.min(t.len())..].chars().next() {
            Some(c) => {
                let w = self.ux(i + c.len_utf8()) - self.ux(i);
                if w > px(0.) { w } else { fallback }
            }
            None => fallback,
        }
    }
    /// Line-relative rects covering display bytes `a..b`.
    fn rects(&self, a: usize, b: usize) -> Vec<Bounds<Pixels>> {
        let mut out = Vec::new();
        for r in 0..self.row_starts.len() {
            let (rs, re) = (self.row_starts[r], self.row_end(r));
            let (s, e) = (a.max(rs), b.min(re));
            if s > e || (s == e && !(a == b && s == a)) {
                continue;
            }
            let x0 = self.ux(s) - self.ux(rs);
            let x1 = self.ux(e) - self.ux(rs);
            out.push(Bounds {
                origin: point(x0, self.row_h * r as f32),
                size: size(x1 - x0, self.row_h),
            });
        }
        out
    }
}

pub(crate) struct Frame {
    pub bounds: Bounds<Pixels>,
    pub char_w: Pixels,
    pub lines: Vec<FrameLine>,
    pub hits: Vec<(Bounds<Pixels>, Hit)>,
    pub prims: Vec<Prim>,
    /// Bottom-left of the main cursor (completion popup anchor).
    pub cursor_anchor: Option<Point<Pixels>>,
    /// Painted cursors: (bounds, primary).
    pub cursors: Vec<(Bounds<Pixels>, bool)>,
    /// Gutter labels: (0-based line, text).
    pub gutter: Vec<(usize, String)>,
    /// Window x of each painted ruler.
    pub rulers: Vec<Pixels>,
}

impl Frame {
    pub fn hit_at(&self, p: Point<Pixels>) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(b, _)| b.contains(&p))
            .map(|(_, h)| h.clone())
    }

    fn line_at_y(&self, y: Pixels) -> Option<&FrameLine> {
        let lines: Vec<&FrameLine> = self.lines.iter().filter(|l| l.height > px(0.)).collect();
        let first = *lines.first()?;
        if y < first.top {
            return Some(first);
        }
        lines
            .iter()
            .copied()
            .find(|l| y < l.top + l.height)
            .or(lines.last().copied())
    }

    /// Source byte under a window point (and its 0-based line).
    /// Source byte under `p`. `block`: the char containing the point
    /// (normal-mode block cursor) instead of the closest char boundary.
    pub fn byte_at(&self, p: Point<Pixels>, block: bool) -> Option<(usize, usize)> {
        let fl = self.line_at_y(p.y)?;
        if fl.table || fl.layout.is_none() {
            return Some((fl.disp.src.start, fl.line));
        }
        let rows = fl.row_starts.len();
        let r = (((p.y - fl.top) / fl.row_h).floor().max(0.) as usize).min(rows - 1);
        let rs = fl.row_starts[r];
        let layout = fl.layout.as_ref().unwrap();
        let x = fl.ux(rs) + (p.x - fl.x).max(px(0.));
        let mut i = layout
            .unwrapped_layout
            .closest_index_for_x(x)
            .clamp(rs, fl.len());
        if block && i > rs && fl.ux(i) > x {
            i -= 1;
            while i > rs && !fl.disp.text.is_char_boundary(i) {
                i -= 1;
            }
        }
        if r + 1 < rows && i >= fl.row_starts[r + 1] {
            i = fl.row_starts[r + 1];
            // last char of the row
            while i > rs {
                i -= 1;
                if fl.disp.text.is_char_boundary(i) {
                    break;
                }
            }
        }
        Some((fl.disp.src_of(i), fl.line))
    }

    pub fn char_bounds(&self, byte: usize, _text: &str) -> Option<Bounds<Pixels>> {
        let fl = self
            .lines
            .iter()
            .find(|l| l.height > px(0.) && l.disp.src.start <= byte && byte <= l.disp.src.end)?;
        let d = fl.disp.disp_of(byte);
        let p = fl.pos(d);
        let w = fl.advance(d, self.char_w);
        Some(Bounds {
            origin: point(fl.x + p.x, fl.top + p.y),
            size: size(w, fl.row_h),
        })
    }
}

// ---- helpers ----------------------------------------------------------------

fn font(family: &SharedString, bold: bool, italic: bool) -> Font {
    Font {
        family: family.clone(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

fn run(len: usize, font: Font, color: Hsla) -> TextRun {
    TextRun {
        len,
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

fn shape(
    window: &Window,
    text: &str,
    family: &SharedString,
    size: Pixels,
    color: Hsla,
    bold: bool,
) -> ShapedLine {
    let s: SharedString = text.to_string().into();
    window.text_system().shape_line(
        s,
        size,
        &[run(text.len(), font(family, bold, false), color)],
        None,
    )
}

fn with_alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a: c.a * a, ..c }
}

/// Runs for a display line: base metrics + overlapping spans (later wins).
fn runs_for(st: &EditorStyle, d: &DisplayLine) -> Vec<TextRun> {
    let m = d.metrics;
    let base_color = m.color.map_or(st.foreground, |t| st.token(t));
    let base_color = with_alpha(base_color, m.opacity);
    let len = d.text.len();
    let mut cuts: Vec<usize> = vec![0, len];
    for s in &d.spans {
        cuts.push(s.disp.start.min(len));
        cuts.push(s.disp.end.min(len));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut runs = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let mut sty = SpanStyle::default();
        for s in d
            .spans
            .iter()
            .filter(|s| s.disp.start <= a && b <= s.disp.end)
        {
            let o = s.style;
            sty.color = o.color.or(sty.color);
            sty.bold |= o.bold;
            sty.italic |= o.italic;
            sty.underline |= o.underline;
            sty.strikethrough |= o.strikethrough;
            sty.opacity = o.opacity.or(sty.opacity);
        }
        let mut color = sty.color.map_or(base_color, |t| st.token(t));
        if let Some(o) = sty.opacity {
            color = with_alpha(color, o);
        }
        let mut r = run(
            b - a,
            font(&st.mono_family, m.bold || sty.bold, sty.italic),
            color,
        );
        if sty.underline {
            r.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: Some(with_alpha(color, 0.6)),
                wavy: false,
            });
        }
        if sty.strikethrough {
            r.strikethrough = Some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            });
        }
        runs.push(r);
    }
    runs
}

fn chevron(x: Pixels, y: Pixels, open: bool) -> Vec<Point<Pixels>> {
    if open {
        vec![
            point(x, y - px(2.)),
            point(x + px(3.5), y + px(1.5)),
            point(x + px(7.), y - px(2.)),
        ]
    } else {
        vec![
            point(x + px(1.5), y - px(4.)),
            point(x + px(5.), y - px(0.5)),
            point(x + px(1.5), y + px(3.)),
        ]
    }
}

const TABLE_PAD: f32 = 6.;

fn table_cell_h(body: f32) -> f32 {
    body + 8.
}

fn table_height(rows_incl_header: usize, body: f32) -> f32 {
    rows_incl_header as f32 * table_cell_h(body) + 2. * TABLE_PAD + 18.
}

// ---- layout -------------------------------------------------------------------

impl EditorView {
    fn hidden_lines(&self) -> Vec<bool> {
        let n = self.model.line_count();
        let mut hidden = vec![false; n];
        let lines = self.model.lines();
        let len = self.model.text().len();
        let mut covered: Vec<Range<usize>> = Vec::new();
        for r in self.folds.ranges() {
            if r.end > len {
                continue;
            }
            let l0 = lines.line_of(r.start);
            if covered.iter().any(|h| h.contains(&l0)) {
                continue;
            }
            let l1 = lines.line_of(r.end);
            if l1 > l0 {
                covered.push(l0 + 1..l1 + 1);
                for h in hidden.iter_mut().take((l1 + 1).min(n)).skip(l0 + 1) {
                    *h = true;
                }
            }
        }
        for t in &self.model.parsed().tables {
            for h in hidden
                .iter_mut()
                .take((t.last_line + 1).min(n))
                .skip(t.first_line + 1)
            {
                *h = true;
            }
        }
        hidden
    }

    fn row_h_of(&self, line: usize) -> f32 {
        let m = self.model.line_info(line).map(|i| i.height.metrics());
        let fs = f32::from(self.style.font_size);
        match m {
            Some(m) => (fs * m.font_scale * m.line_height).round(),
            None => f32::from(self.style.line_height_px()),
        }
    }

    fn recompute_offsets(&mut self, char_w: f32) {
        let hidden = self.hidden_lines();
        let n = hidden.len();
        let body = f32::from(self.style.line_height_px());
        let mut table_first: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for t in &self.model.parsed().tables {
            table_first.insert(t.first_line, t.last_line - t.first_line);
        }
        let mut off = Vec::with_capacity(n + 1);
        let mut y = 0.;
        off.push(0.);
        for (l, &hid) in hidden.iter().enumerate() {
            let h = if hid {
                0.
            } else if let Some(rows) = table_first.get(&l) {
                table_height(*rows, body)
            } else {
                let rows = match (self.wrap_rows.get(&l), self.wrap_width) {
                    (Some(r), _) => *r,
                    (None, Some(w)) if w > 0. => {
                        let chars = self.model.line_range(l).len() as f32;
                        ((chars * char_w) / w).ceil().max(1.) as usize
                    }
                    _ => 1,
                };
                self.row_h_of(l) * rows as f32
            };
            y += h;
            off.push(y);
        }
        self.offsets = off;
        let mut s = self.shared.borrow_mut();
        s.hidden = hidden;
        self.layout_dirty = false;
    }

    fn apply_scroll(&mut self, vh: f32) {
        let total = *self.offsets.last().unwrap_or(&0.);
        if let Some(s) = self.pending_scroll.take() {
            match s {
                Scroll::To { pos, align } => {
                    let b = self.byte_of(pos);
                    let mut l = self.model.lines().line_of(b);
                    let hidden = &self.shared.borrow().hidden;
                    while l > 0 && hidden.get(l).copied().unwrap_or(false) {
                        l -= 1;
                    }
                    let top = self.offsets[l];
                    let bottom = self.offsets[l + 1].max(top + 1.);
                    // Row of the target inside a wrapped line.
                    let (top, bottom) = match self.wrap_rows.get(&l) {
                        Some(&rows) if rows > 1 => {
                            let rh = (bottom - top) / rows as f32;
                            let row = self.shared.borrow().rows.get(&(l + 1)).map_or(0, |starts| {
                                starts.iter().rposition(|&s| s <= pos).unwrap_or(0)
                            });
                            (top + rh * row as f32, top + rh * (row + 1) as f32)
                        }
                        _ => (top, bottom),
                    };
                    let margin = f32::from(self.style.line_height_px()).min(vh / 4.);
                    self.scroll_y = match align {
                        ScrollAlign::Nearest => {
                            if top - margin < self.scroll_y {
                                top - margin
                            } else if bottom + margin > self.scroll_y + vh {
                                bottom + margin - vh
                            } else {
                                self.scroll_y
                            }
                        }
                        ScrollAlign::Center => top - (vh - (bottom - top)) / 2.,
                        ScrollAlign::Top => top,
                        ScrollAlign::Bottom => bottom - vh,
                    };
                }
                Scroll::Lines(n) => {
                    self.scroll_y += n as f32 * f32::from(self.style.line_height_px());
                }
                Scroll::RestorePromptStart => self.scroll_y = self.prompt_scroll_start,
            }
        }
        let max = (total - vh * 0.5).max(0.).max(total - vh);
        let max = if total <= vh {
            0.
        } else {
            max.min(total - vh * 0.25)
        };
        self.scroll_y = self.scroll_y.clamp(0., max.max(0.));
    }

    pub(crate) fn layout_frame(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Rc<Frame> {
        let st = self.style.clone();
        let fs = st.font_size;
        let body = f32::from(st.line_height_px());
        let mono = st.mono_family.clone();
        let sans = st.sans_family.clone();
        let focused = self.focus.is_focused(window);
        let char_w = shape(window, "M", &mono, fs, st.foreground, false).width();
        let n = self.model.line_count().max(1);

        // Gutter: [pad][numbers][gap][chevron][pad]
        let digits = n.to_string().len().max(2);
        let num_w = if self.options.line_numbers == LineNumbers::Off {
            px(0.)
        } else {
            char_w * digits as f32
        };
        let gutter_w = px(10.) + num_w + px(6.) + px(10.) + px(6.);
        let text_x = bounds.origin.x + gutter_w + px(4.);
        let right = bounds.origin.x + bounds.size.width;

        let wrap = self
            .options
            .soft_wrap
            .then(|| f32::from(bounds.size.width - gutter_w) - 24.);
        if wrap != self.wrap_width {
            self.wrap_width = wrap;
            self.wrap_rows.clear();
            self.layout_dirty = true;
        }
        if self.layout_dirty || self.offsets.len() != self.model.line_count() + 1 {
            self.recompute_offsets(f32::from(char_w));
        }
        let vh = f32::from(bounds.size.height);
        self.viewport = (f32::from(bounds.size.width), vh);
        self.apply_scroll(vh);

        // Visible range.
        let lc = self.model.line_count();
        let mut first = self.offsets[1..]
            .partition_point(|&o| o <= self.scroll_y)
            .min(lc.saturating_sub(1));
        let mut last = first;
        while last + 1 < lc && self.offsets[last + 1] < self.scroll_y + vh {
            last += 1;
        }
        let cursor_byte = self.byte_of(self.engine.selection().main().head);
        let cursor_line = self.model.lines().line_of(cursor_byte);
        let table_edit = self.table.as_ref().map(|t| t.state);

        // Decorate + shape (twice at most, if wrapping changed heights).
        let mut lines: Vec<FrameLine> = Vec::new();
        for _pass in 0..2 {
            lines.clear();
            let ctx = DecorationContext {
                cursor_line: Some(cursor_line),
                folds: &self.folds,
                vault_files: &self.options.vault_files,
                table_edit,
            };
            let decs = self.model.decorations(first..last + 1, &ctx);
            let mut wrap_changed = false;
            for l in first..=last {
                let Some(dec) = decs.get(l) else { continue };
                let top = bounds.origin.y + px(self.offsets[l] - self.scroll_y);
                let height = px(self.offsets[l + 1] - self.offsets[l]);
                if dec.hidden != Hidden::No {
                    continue;
                }
                let disp = DisplayLine::build(self.model.text(), dec);
                let row_h = px(self.row_h_of(l));
                let x = text_x + px(14. * dec.quote_depth as f32);
                if matches!(dec.block, Some(BlockWidget::Table { .. })) {
                    lines.push(FrameLine {
                        line: l,
                        top,
                        height,
                        row_h,
                        x,
                        disp,
                        layout: None,
                        row_starts: vec![0],
                        table: true,
                    });
                    continue;
                }
                let line_fs = fs * disp.metrics.font_scale;
                let runs = runs_for(&st, &disp);
                let ww = wrap.map(|w| px(w) - (x - text_x));
                let layout = window
                    .text_system()
                    .shape_text(disp.text.clone().into(), line_fs, &runs, ww, None)
                    .ok()
                    .and_then(|mut v| {
                        if v.is_empty() {
                            None
                        } else {
                            Some(Rc::new(v.remove(0)))
                        }
                    });
                let mut row_starts = vec![0];
                if let Some(lay) = &layout {
                    for b in lay.wrap_boundaries() {
                        if let Some(g) = lay
                            .unwrapped_layout
                            .runs
                            .get(b.run_ix)
                            .and_then(|r| r.glyphs.get(b.glyph_ix))
                        {
                            row_starts.push(g.index);
                        }
                    }
                }
                if wrap.is_some() {
                    let rows = row_starts.len();
                    if self.wrap_rows.get(&l) != Some(&rows) {
                        self.wrap_rows.insert(l, rows);
                        wrap_changed = true;
                    }
                }
                lines.push(FrameLine {
                    line: l,
                    top,
                    height,
                    row_h,
                    x,
                    disp,
                    layout,
                    row_starts,
                    table: false,
                });
            }
            if !wrap_changed {
                break;
            }
            self.recompute_offsets(f32::from(char_w));
            self.apply_scroll(vh);
            first = self.offsets[1..]
                .partition_point(|&o| o <= self.scroll_y)
                .min(lc.saturating_sub(1));
            last = first;
            while last + 1 < lc && self.offsets[last + 1] < self.scroll_y + vh {
                last += 1;
            }
        }

        // Publish layout facts to the engine host.
        {
            let mut s = self.shared.borrow_mut();
            s.rows.clear();
            let doc = self.engine.doc();
            for fl in &lines {
                if fl.row_starts.len() > 1 {
                    let starts = fl
                        .row_starts
                        .iter()
                        .map(|&d| doc.byte_to_char(fl.disp.src_of(d).min(self.model.text().len())))
                        .collect();
                    s.rows.insert(fl.line + 1, starts);
                }
            }
            let vis_first = lines.first().map_or(first, |l| l.line) + 1;
            let vis_last = lines.last().map_or(last, |l| l.line) + 1;
            s.visible = Some((vis_first, vis_last));
            s.half_page = ((vh / body) / 2.).floor().max(1.) as usize;
        }

        let mut prims: Vec<Prim> = Vec::new();
        let mut cursors: Vec<(Bounds<Pixels>, bool)> = Vec::new();
        let mut gutter: Vec<(usize, String)> = Vec::new();
        let mut rulers: Vec<Pixels> = Vec::new();
        let mut hits: Vec<(Bounds<Pixels>, Hit)> = Vec::new();
        let text_right = right - px(16.);

        // ---- backgrounds --------------------------------------------------------
        let decs_ctx = DecorationContext {
            cursor_line: Some(cursor_line),
            folds: &self.folds,
            vault_files: &self.options.vault_files,
            table_edit,
        };
        let decs = self.model.decorations(first..last + 1, &decs_ctx);
        let dec_of = |l: usize| -> Option<&LineDecoration> { decs.get(l) };
        for fl in &lines {
            let Some(dec) = dec_of(fl.line) else { continue };
            let row = Bounds {
                origin: point(bounds.origin.x, fl.top),
                size: size(bounds.size.width, fl.height),
            };
            if fl.line == cursor_line && !fl.table {
                prims.push(Prim::Quad(fill(row, st.active_line())));
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: row.origin,
                        size: size(gutter_w, fl.height),
                    },
                    st.active_gutter(),
                )));
            }
            let has = |c: LineClass| dec.classes.contains(&c);
            let box_x = text_x - px(6.);
            let box_b = Bounds {
                origin: point(box_x, fl.top),
                size: size(text_right - box_x, fl.height),
            };
            let r = st.radius_sm;
            if has(LineClass::FenceHeader)
                || has(LineClass::FenceBody)
                || has(LineClass::FenceFooter)
            {
                let (tl, bl) = (has(LineClass::FenceHeader), has(LineClass::FenceFooter));
                let corners = gpui_kit::Corners {
                    top_left: if tl { r } else { px(0.) },
                    top_right: if tl { r } else { px(0.) },
                    bottom_left: if bl { r } else { px(0.) },
                    bottom_right: if bl { r } else { px(0.) },
                };
                let borders = gpui_kit::Edges {
                    top: if tl { px(1.) } else { px(0.) },
                    bottom: if bl { px(1.) } else { px(0.) },
                    left: px(1.),
                    right: px(1.),
                };
                prims.push(Prim::Quad(quad(
                    box_b,
                    corners,
                    mix(st.secondary, 0.6),
                    borders,
                    st.border,
                    BorderStyle::Solid,
                )));
            }
            if has(LineClass::FrontmatterLine) || has(LineClass::FrontmatterFence) {
                prims.push(Prim::Quad(fill(box_b, mix(st.foreground, 0.04))));
            }
            if let Some(c) = &dec.callout {
                let acc = st.callout_accent(c.kind);
                prims.push(Prim::Quad(fill(box_b, mix(acc, 0.08))));
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: box_b.origin,
                        size: size(px(3.), fl.height),
                    },
                    acc,
                )));
            } else if dec.quote_depth > 0 {
                for i in 0..dec.quote_depth {
                    prims.push(Prim::Quad(fill(
                        Bounds {
                            origin: point(text_x + px(14. * i as f32), fl.top),
                            size: size(px(3.), fl.height),
                        },
                        st.border,
                    )));
                }
            }
            if has(LineClass::RegionBody)
                || has(LineClass::RegionStart)
                || has(LineClass::RegionEnd)
            {
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: point(box_x, fl.top),
                        size: size(px(2.), fl.height),
                    },
                    mix(st.syntax_function, 0.4),
                )));
            }
            if has(LineClass::HorizontalRule) && fl.line != cursor_line {
                let y = fl.top + fl.height / 2.;
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: point(text_x, y),
                        size: size(text_right - text_x, px(1.)),
                    },
                    st.border,
                )));
            }
            for &col in &dec.indent_guides {
                let gx = fl.x + char_w * col as f32;
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: point(gx, fl.top),
                        size: size(px(1.), fl.height),
                    },
                    mix(st.border, 0.7),
                )));
            }
        }

        // ---- selections + find matches --------------------------------------
        let mode = self.mode();
        let sel = self.engine.selection().clone();
        let main_ix = sel.main_index();
        let text_len = self.model.text().len();
        let to_b = |c: usize| self.byte_of(c);
        for (i, r) in sel.ranges().iter().enumerate() {
            let w = r.to() - r.from();
            if w == 0 || (mode != ModeType::Insert && w <= 1) {
                continue;
            }
            let (a, b) = (to_b(r.from()), to_b(r.to()));
            let color = if i == main_ix {
                st.selection()
            } else {
                st.selection_secondary()
            };
            for fl in lines.iter().filter(|fl| !fl.table) {
                let (ls, le) = (fl.disp.src.start, fl.disp.src.end);
                if b < ls || a > le || (b == ls && a < ls) {
                    continue;
                }
                let da = fl.disp.disp_of(a.max(ls));
                let db = fl.disp.disp_of(b.min(le));
                let mut rects = fl.rects(da, db);
                if b > le
                    && let Some(last) = rects.last_mut()
                {
                    last.size.width += char_w * 0.6; // the newline
                }
                for rc in rects {
                    prims.push(Prim::Quad(fill(rc + point(fl.x, fl.top), color)));
                }
            }
        }
        if let Overlay::Find(f) = &self.overlay
            && let (Some(fl0), Some(fl1)) = (lines.first(), lines.last())
        {
            let (vs, ve) = (fl0.disp.src.start, fl1.disp.src.end);
            let start = f.matches.partition_point(|m| m.end < vs);
            for (k, m) in f.matches.iter().enumerate().skip(start) {
                if m.start > ve {
                    break;
                }
                let color = st.search_match(k == f.current);
                for fl in lines.iter().filter(|fl| {
                    !fl.table && fl.disp.src.start <= m.end && m.start <= fl.disp.src.end
                }) {
                    let da = fl.disp.disp_of(m.start.max(fl.disp.src.start));
                    let db = fl.disp.disp_of(m.end.min(fl.disp.src.end));
                    for rc in fl.rects(da, db) {
                        prims.push(Prim::Quad(fill(rc + point(fl.x, fl.top), color)));
                    }
                }
            }
        }

        // ---- widgets under text (pills) -----------------------------------------
        for fl in lines.iter().filter(|fl| !fl.table) {
            for w in &fl.disp.widgets {
                let rects = fl.rects(w.disp.start, w.disp.end);
                let abs: Vec<Bounds<Pixels>> =
                    rects.iter().map(|r| *r + point(fl.x, fl.top)).collect();
                match &w.kind {
                    WidgetKind::Tag { tag } => {
                        for b in &abs {
                            let b = b.dilate(px(1.));
                            prims.push(Prim::Quad(quad(
                                Bounds {
                                    origin: point(b.origin.x, b.origin.y + px(2.)),
                                    size: size(b.size.width, b.size.height - px(4.)),
                                },
                                px(4.),
                                mix(st.syntax_keyword, 0.12),
                                px(0.),
                                gpui_kit::transparent_black(),
                                BorderStyle::Solid,
                            )));
                            hits.push((b, Hit::Tag(tag.clone())));
                        }
                    }
                    WidgetKind::Fold { range, .. } => {
                        for b in &abs {
                            prims.push(Prim::Quad(quad(
                                Bounds {
                                    origin: point(b.origin.x, b.origin.y + px(2.)),
                                    size: size(b.size.width, b.size.height - px(4.)),
                                },
                                px(4.),
                                mix(st.muted_foreground, 0.15),
                                px(0.),
                                gpui_kit::transparent_black(),
                                BorderStyle::Solid,
                            )));
                            hits.push((*b, Hit::FoldPill(range.clone())));
                        }
                    }
                    WidgetKind::Link { url } => {
                        for b in &abs {
                            hits.push((*b, Hit::Link(url.clone())));
                        }
                    }
                    WidgetKind::Wikilink { target, resolved } => {
                        for b in &abs {
                            let color = if resolved.is_some() {
                                mix(st.syntax_function, 0.45)
                            } else {
                                mix(st.muted_foreground, 0.6)
                            };
                            let y = b.origin.y + b.size.height - px(4.);
                            prims.push(Prim::Underline {
                                origin: point(b.origin.x, y),
                                width: b.size.width,
                                style: UnderlineStyle {
                                    thickness: px(1.),
                                    color: Some(color),
                                    wavy: false,
                                },
                            });
                            hits.push((
                                *b,
                                Hit::Wikilink {
                                    target: target.clone(),
                                    resolved: resolved.clone(),
                                },
                            ));
                        }
                    }
                    WidgetKind::RegionStart { line_from } => {
                        if let Some(b) = abs.first() {
                            hits.push((
                                Bounds {
                                    origin: b.origin,
                                    size: size(char_w * 2., b.size.height),
                                },
                                Hit::RegionChevron(*line_from),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }

        // ---- text ---------------------------------------------------------------
        for fl in &lines {
            if let Some(l) = &fl.layout {
                prims.push(Prim::Text {
                    line: l.clone(),
                    origin: point(fl.x, fl.top),
                    lh: fl.row_h,
                });
            }
        }

        // ---- widgets over text --------------------------------------------------
        for fl in lines.iter().filter(|fl| !fl.table) {
            for w in &fl.disp.widgets {
                if let WidgetKind::Checkbox { checked, toggle } = &w.kind {
                    let p = fl.pos(w.disp.start);
                    let sz = px(14.);
                    let bx = fl.x + p.x + (char_w * 2. - sz) / 2.;
                    let by = fl.top + p.y + (fl.row_h - sz) / 2.;
                    let b = Bounds {
                        origin: point(bx, by),
                        size: size(sz, sz),
                    };
                    if *checked {
                        prims.push(Prim::Quad(quad(
                            b,
                            px(3.),
                            st.primary,
                            px(1.),
                            st.primary,
                            BorderStyle::Solid,
                        )));
                        prims.push(Prim::Path {
                            points: vec![
                                point(bx + px(3.), by + px(7.)),
                                point(bx + px(6.), by + px(10.)),
                                point(bx + px(11.), by + px(4.)),
                            ],
                            width: px(1.6),
                            color: st.primary_foreground,
                        });
                    } else {
                        prims.push(Prim::Quad(quad(
                            b,
                            px(3.),
                            gpui_kit::transparent_black(),
                            px(1.),
                            st.muted_foreground,
                            BorderStyle::Solid,
                        )));
                    }
                    hits.push((b.dilate(px(2.)), Hit::Checkbox(toggle.clone())));
                }
            }
            // Line-end widgets (fence toolbar, mermaid button).
            let mut ex = text_right - px(8.);
            for le in &fl.disp.line_end {
                match le {
                    LineEndWidget::FenceToolbar(tb) => {
                        let copied = self.copied_fence == Some(tb.fence);
                        let copy = shape(
                            window,
                            if copied { "Copied" } else { "Copy" },
                            &sans,
                            px(11.),
                            st.muted_foreground,
                            false,
                        );
                        let lang_label = if tb.language.is_empty() {
                            "plain".to_string()
                        } else {
                            tb.language.clone()
                        };
                        let lang = shape(
                            window,
                            &format!("{lang_label} ▾"),
                            &sans,
                            px(11.),
                            st.muted_foreground,
                            false,
                        );
                        let y = fl.top + (fl.row_h - px(16.)) / 2.;
                        let cw = copy.width() + px(12.);
                        let cb = Bounds {
                            origin: point(ex - cw, y),
                            size: size(cw, px(16.)),
                        };
                        prims.push(Prim::Quad(quad(
                            cb,
                            px(3.),
                            st.background,
                            px(1.),
                            st.border,
                            BorderStyle::Solid,
                        )));
                        prims.push(Prim::Shaped {
                            line: copy,
                            origin: point(cb.origin.x + px(6.), y),
                            lh: px(16.),
                        });
                        hits.push((
                            cb,
                            Hit::Copy {
                                fence: tb.fence,
                                text: tb.copy_text.clone(),
                            },
                        ));
                        ex = cb.origin.x - px(6.);
                        let lw = lang.width() + px(12.);
                        let lb = Bounds {
                            origin: point(ex - lw, y),
                            size: size(lw, px(16.)),
                        };
                        prims.push(Prim::Quad(quad(
                            lb,
                            px(3.),
                            st.background,
                            px(1.),
                            st.border,
                            BorderStyle::Solid,
                        )));
                        prims.push(Prim::Shaped {
                            line: lang,
                            origin: point(lb.origin.x + px(6.), y),
                            lh: px(16.),
                        });
                        hits.push((lb, Hit::Lang(tb.clone())));
                        ex = lb.origin.x - px(6.);
                    }
                    LineEndWidget::MermaidButton { anchor, .. } => {
                        let label = shape(
                            window,
                            "▶ Preview diagram",
                            &sans,
                            px(11.),
                            st.primary,
                            false,
                        );
                        let y = fl.top + (fl.row_h - px(16.)) / 2.;
                        let w = label.width() + px(12.);
                        let b = Bounds {
                            origin: point(ex - w, y),
                            size: size(w, px(16.)),
                        };
                        prims.push(Prim::Quad(quad(
                            b,
                            px(3.),
                            mix(st.primary, 0.1),
                            px(1.),
                            mix(st.primary, 0.4),
                            BorderStyle::Solid,
                        )));
                        prims.push(Prim::Shaped {
                            line: label,
                            origin: point(b.origin.x + px(6.), y),
                            lh: px(16.),
                        });
                        hits.push((b, Hit::Mermaid { anchor: *anchor }));
                        ex = b.origin.x - px(6.);
                    }
                }
            }
        }

        // ---- tables -------------------------------------------------------------
        for fl in lines.iter().filter(|fl| fl.table) {
            let Some(dec) = dec_of(fl.line) else { continue };
            let Some(BlockWidget::Table { model, edit, .. }) = &dec.block else {
                continue;
            };
            self.paint_table(window, &st, fl, model, *edit, char_w, &mut prims, &mut hits);
        }

        // ---- gutter -------------------------------------------------------------
        for fl in &lines {
            let y = fl.top;
            let lh = fl.row_h.min(fl.height);
            if self.options.line_numbers != LineNumbers::Off {
                let num = match self.options.line_numbers {
                    LineNumbers::Relative if fl.line != cursor_line => {
                        fl.line.abs_diff(cursor_line)
                    }
                    _ => fl.line + 1,
                };
                let color = if fl.line == cursor_line {
                    st.foreground
                } else {
                    mix(st.muted_foreground, 0.8)
                };
                gutter.push((fl.line, num.to_string()));
                let s = shape(
                    window,
                    &num.to_string(),
                    &mono,
                    fs.min(px(13.)),
                    color,
                    false,
                );
                let nx = bounds.origin.x + px(10.) + num_w - s.width();
                prims.push(Prim::Shaped {
                    line: s,
                    origin: point(nx, y),
                    lh,
                });
            }
            if !fl.table && self.model.foldable(fl.line).is_some() {
                let folded = self
                    .model
                    .foldable(fl.line)
                    .is_some_and(|r| self.folds.ranges().iter().any(|f| f.start == r.start));
                let cx0 = bounds.origin.x + px(10.) + num_w + px(6.);
                let cy = y + lh / 2.;
                prims.push(Prim::Path {
                    points: chevron(cx0, cy, !folded),
                    width: px(1.2),
                    color: st.muted_foreground,
                });
                hits.push((
                    Bounds {
                        origin: point(cx0 - px(3.), y),
                        size: size(px(14.), lh),
                    },
                    Hit::FoldGutter(fl.line),
                ));
            }
        }

        // ---- rulers ---------------------------------------------------------------
        for &col in self.model.rulers() {
            let x = text_x + char_w * col as f32;
            if x < right {
                rulers.push(x);
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: point(x, bounds.origin.y),
                        size: size(px(1.), bounds.size.height),
                    },
                    st.ruler(),
                )));
            }
        }

        // ---- IME marked text ------------------------------------------------------
        if let Some(m) = &self.marked {
            let (a, b) = (to_b(m.start), to_b(m.end));
            for fl in lines
                .iter()
                .filter(|fl| !fl.table && fl.disp.src.start <= a && b <= fl.disp.src.end)
            {
                for rc in fl.rects(fl.disp.disp_of(a), fl.disp.disp_of(b)) {
                    let rc = rc + point(fl.x, fl.top);
                    prims.push(Prim::Underline {
                        origin: point(rc.origin.x, rc.origin.y + rc.size.height - px(3.)),
                        width: rc.size.width,
                        style: UnderlineStyle {
                            thickness: px(1.),
                            color: Some(st.foreground),
                            wavy: false,
                        },
                    });
                }
            }
        }

        // ---- cursors -------------------------------------------------------------
        let heads = self.engine.block_cursors();
        let block = mode != ModeType::Insert || self.options.insert_cursor == InsertCursor::Block;
        let mut cursor_anchor = None;
        if self.table.is_none() {
            for (i, &h) in heads.iter().enumerate() {
                let hb = to_b(h).min(text_len);
                let Some(fl) = lines
                    .iter()
                    .find(|fl| !fl.table && fl.disp.src.start <= hb && hb <= fl.disp.src.end)
                else {
                    continue;
                };
                let d = fl.disp.disp_of(hb);
                let p = fl.pos(d);
                let origin = point(fl.x + p.x, fl.top + p.y);
                let primary = i == main_ix;
                if block {
                    let w = fl.advance(d, char_w);
                    let b = Bounds {
                        origin,
                        size: size(w, fl.row_h),
                    };
                    cursors.push((b, primary));
                    if primary {
                        prims.push(Prim::Quad(quad(
                            b,
                            px(1.),
                            if focused {
                                st.block_cursor()
                            } else {
                                gpui_kit::transparent_black()
                            },
                            px(1.),
                            st.block_cursor_outline(focused),
                            BorderStyle::Solid,
                        )));
                    } else {
                        prims.push(Prim::Quad(fill(b, st.block_cursor_secondary())));
                    }
                } else {
                    let color = if primary {
                        st.foreground
                    } else {
                        mix(st.foreground, 0.5)
                    };
                    cursors.push((
                        Bounds {
                            origin,
                            size: size(px(2.), fl.row_h),
                        },
                        primary,
                    ));
                    prims.push(Prim::Quad(fill(
                        Bounds {
                            origin,
                            size: size(px(2.), fl.row_h),
                        },
                        color,
                    )));
                }
                if primary {
                    cursor_anchor = Some(point(origin.x, origin.y + fl.row_h));
                }
            }
        }
        let _ = (cx, &sans);

        Rc::new(Frame {
            bounds,
            char_w,
            lines,
            hits,
            prims,
            cursor_anchor,
            cursors,
            gutter,
            rulers,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_table(
        &self,
        window: &Window,
        st: &EditorStyle,
        fl: &FrameLine,
        model: &TableModel,
        edit: Option<fuchico_markdown::TableEditState>,
        char_w: Pixels,
        prims: &mut Vec<Prim>,
        hits: &mut Vec<(Bounds<Pixels>, Hit)>,
    ) {
        let body = f32::from(st.line_height_px());
        let cell_h = px(table_cell_h(body));
        let anchor = model.range.start;
        // Live data while a cell is being edited.
        let data = match (&self.table, edit) {
            (Some(t), Some(_)) if t.state.anchor == anchor => t.data.clone(),
            _ => model.data(),
        };
        let cols = data.col_count();
        let rows = data.row_count();
        let widths: Vec<Pixels> = (0..cols)
            .map(|c| char_w * (model.widths.get(c).copied().unwrap_or(3).max(3) as f32) + px(20.))
            .collect();
        let x0 = fl.x;
        let y0 = fl.top + px(TABLE_PAD);
        let total_w: Pixels = widths.iter().fold(px(0.), |a, &w| a + w);
        let grid = Bounds {
            origin: point(x0, y0),
            size: size(total_w, cell_h * rows as f32),
        };
        prims.push(Prim::Quad(quad(
            grid,
            px(4.),
            st.background,
            px(1.),
            st.border,
            BorderStyle::Solid,
        )));
        prims.push(Prim::Quad(quad(
            Bounds {
                origin: grid.origin,
                size: size(total_w, cell_h),
            },
            gpui_kit::Corners {
                top_left: px(4.),
                top_right: px(4.),
                bottom_left: px(0.),
                bottom_right: px(0.),
            },
            mix(st.secondary, 0.8),
            px(0.),
            gpui_kit::transparent_black(),
            BorderStyle::Solid,
        )));
        let sans = st.sans_family.clone();
        let fsz = st.font_size;
        for r in 0..rows {
            let y = y0 + cell_h * r as f32;
            if r > 0 {
                prims.push(Prim::Quad(fill(
                    Bounds {
                        origin: point(x0, y),
                        size: size(total_w, px(1.)),
                    },
                    st.border,
                )));
            }
            let mut x = x0;
            for (c, &w) in widths.iter().enumerate() {
                if c > 0 && r == 0 {
                    prims.push(Prim::Quad(fill(
                        Bounds {
                            origin: point(x, y0),
                            size: size(px(1.), cell_h * rows as f32),
                        },
                        st.border,
                    )));
                }
                let cb = Bounds {
                    origin: point(x, y),
                    size: size(w, cell_h),
                };
                let text = data.cell(r, c);
                let line = shape(window, text, &sans, fsz, st.foreground, r == 0);
                let tw = line.width();
                let tx = match data.align.get(c).copied().flatten() {
                    Some(fuchico_markdown::Align::Right) => x + w - px(10.) - tw,
                    Some(fuchico_markdown::Align::Center) => x + (w - tw) / 2.,
                    _ => x + px(10.),
                };
                prims.push(Prim::Shaped {
                    line,
                    origin: point(tx, y + px(4.)),
                    lh: cell_h - px(8.),
                });
                if let Some(e) = edit
                    && e.row == r
                    && e.col == c
                {
                    prims.push(Prim::Quad(quad(
                        cb.dilate(px(-1.)),
                        px(2.),
                        gpui_kit::transparent_black(),
                        px(2.),
                        st.ring,
                        BorderStyle::Solid,
                    )));
                    if e.editing {
                        prims.push(Prim::Quad(fill(
                            Bounds {
                                origin: point(tx + tw + px(1.), y + px(5.)),
                                size: size(px(1.5), cell_h - px(10.)),
                            },
                            st.foreground,
                        )));
                    }
                }
                hits.push((
                    cb,
                    Hit::TableCell {
                        anchor,
                        row: r,
                        col: c,
                    },
                ));
                x += w;
            }
        }
        // "+" affordances: add row (below) / add column (right).
        let plus_row = Bounds {
            origin: point(x0, y0 + cell_h * rows as f32 + px(2.)),
            size: size(total_w, px(14.)),
        };
        let plus_col = Bounds {
            origin: point(x0 + total_w + px(2.), y0),
            size: size(px(14.), cell_h * rows as f32),
        };
        for (b, h) in [
            (plus_row, Hit::TableAddRow { anchor }),
            (plus_col, Hit::TableAddCol { anchor }),
        ] {
            let l = shape(window, "+", &sans, px(11.), st.muted_foreground, false);
            let o = point(
                b.origin.x + (b.size.width - l.width()) / 2.,
                b.origin.y + (b.size.height - px(14.)) / 2.,
            );
            prims.push(Prim::Shaped {
                line: l,
                origin: o,
                lh: px(14.),
            });
            hits.push((b, h));
        }
    }
}

// ---- the element ------------------------------------------------------------------

pub(crate) struct EditorElement {
    pub view: Entity<EditorView>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = (Rc<Frame>, Hitbox);

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> (Rc<Frame>, Hitbox) {
        let frame = self.view.update(cx, |v, cx| {
            let f = v.layout_frame(bounds, window, cx);
            v.frame = Some(f.clone());
            f
        });
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        (frame, hitbox)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        state: &mut (Rc<Frame>, Hitbox),
        window: &mut Window,
        cx: &mut App,
    ) {
        let (frame, hitbox) = state;
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        let over_hit = frame.hit_at(window.mouse_position()).is_some();
        window.set_cursor_style(
            if over_hit {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            },
            hitbox,
        );
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            for p in &frame.prims {
                match p {
                    Prim::Quad(q) => window.paint_quad(q.clone()),
                    Prim::Text { line, origin, lh } => {
                        let _ = line.paint(*origin, *lh, TextAlign::Left, None, window, cx);
                    }
                    Prim::Shaped { line, origin, lh } => {
                        let _ = line.paint(*origin, *lh, TextAlign::Left, None, window, cx);
                    }
                    Prim::Path {
                        points,
                        width,
                        color,
                    } => {
                        let mut b = PathBuilder::stroke(*width);
                        if let Some((f, rest)) = points.split_first() {
                            b.move_to(*f);
                            for p in rest {
                                b.line_to(*p);
                            }
                        }
                        if let Ok(path) = b.build() {
                            window.paint_path(path, *color);
                        }
                    }
                    Prim::Underline {
                        origin,
                        width,
                        style,
                    } => window.paint_underline(*origin, *width, style),
                }
            }
        });
    }
}

// ---- IME ---------------------------------------------------------------------------

impl EditorView {
    fn utf16_to_char(&self, u: usize) -> usize {
        let rope = self.engine.doc().rope();
        rope.utf16_cu_to_char(u.min(rope.len_utf16_cu()))
    }
    fn char_to_utf16(&self, c: usize) -> usize {
        let rope = self.engine.doc().rope();
        rope.char_to_utf16_cu(c.min(rope.len_chars()))
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (a, b) = (
            self.utf16_to_char(range.start),
            self.utf16_to_char(range.end),
        );
        *adjusted = Some(self.char_to_utf16(a)..self.char_to_utf16(b));
        Some(self.engine.doc().slice(a, b))
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let r = self.engine.selection().main();
        let (a, h) = if self.mode() == ModeType::Insert {
            (r.anchor, r.head)
        } else {
            (r.head, r.head)
        };
        let (s, e) = (a.min(h), a.max(h));
        Some(UTF16Selection {
            range: self.char_to_utf16(s)..self.char_to_utf16(e),
            reversed: h < a,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|m| self.char_to_utf16(m.start)..self.char_to_utf16(m.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_insert(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_mark(new_text, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let c = self.utf16_to_char(range_utf16.start);
        self.bounds_for_char(c)
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let c = self.position_for_point(p)?;
        Some(self.char_to_utf16(c))
    }
}
