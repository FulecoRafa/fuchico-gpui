//! Excalidraw fonts, text measurement/wrapping and glyph outlines.
//!
//! Text is drawn as vector glyph outlines (via `ttf-parser`) so it rotates,
//! scales and z-orders like every other element. The bundled faces are the
//! ones Excalidraw ships (all SIL OFL 1.1, converted from Excalidraw's woff2
//! subsets): Excalifont, Virgil, Cascadia Code, Liberation Sans (Excalidraw's
//! Helvetica fallback), Nunito, Lilita One and Comic Shanns.

use std::sync::OnceLock;

use crate::shape::Op;

pub struct FontFace {
    pub face: ttf_parser::Face<'static>,
    pub upm: f32,
    /// Excalidraw's FONT_METRICS (used for vertical placement).
    pub ascender: f32,
    pub descender: f32,
    pub excalidraw_upm: f32,
    pub line_height: f64,
}

struct Fonts {
    excalifont: FontFace,
    virgil: FontFace,
    helvetica: FontFace,
    cascadia: FontFace,
    nunito: FontFace,
    lilita: FontFace,
    comic: FontFace,
}

fn load(bytes: &'static [u8], upm: f32, asc: f32, desc: f32, lh: f64) -> FontFace {
    let face = ttf_parser::Face::parse(bytes, 0).expect("bundled font parses");
    FontFace {
        upm: face.units_per_em() as f32,
        face,
        ascender: asc,
        descender: desc,
        excalidraw_upm: upm,
        line_height: lh,
    }
}

fn fonts() -> &'static Fonts {
    static F: OnceLock<Fonts> = OnceLock::new();
    F.get_or_init(|| Fonts {
        excalifont: load(
            include_bytes!("../assets/fonts/Excalifont-Regular.ttf"),
            1000.0,
            886.0,
            -374.0,
            1.25,
        ),
        virgil: load(
            include_bytes!("../assets/fonts/Virgil-Regular.ttf"),
            1000.0,
            886.0,
            -374.0,
            1.25,
        ),
        helvetica: load(
            include_bytes!("../assets/fonts/LiberationSans-Regular.ttf"),
            2048.0,
            1577.0,
            -471.0,
            1.15,
        ),
        cascadia: load(
            include_bytes!("../assets/fonts/CascadiaCode-Regular.ttf"),
            2048.0,
            1900.0,
            -480.0,
            1.2,
        ),
        nunito: load(
            include_bytes!("../assets/fonts/Nunito-Regular.ttf"),
            1000.0,
            1011.0,
            -353.0,
            1.35,
        ),
        lilita: load(
            include_bytes!("../assets/fonts/LilitaOne-Regular.ttf"),
            1000.0,
            923.0,
            -220.0,
            1.15,
        ),
        comic: load(
            include_bytes!("../assets/fonts/ComicShanns-Regular.ttf"),
            1000.0,
            750.0,
            -250.0,
            1.25,
        ),
    })
}

/// Excalidraw `FONT_FAMILY` ids: 1 Virgil, 2 Helvetica, 3 Cascadia,
/// 5 Excalifont, 6 Nunito, 7 Lilita One, 8 Comic Shanns.
pub fn face(family: u32) -> &'static FontFace {
    let f = fonts();
    match family {
        1 => &f.virgil,
        2 => &f.helvetica,
        3 => &f.cascadia,
        6 => &f.nunito,
        7 => &f.lilita,
        8 => &f.comic,
        _ => &f.excalifont,
    }
}

/// Fallback chain for glyphs the primary face lacks.
fn chain(family: u32) -> [&'static FontFace; 3] {
    let f = fonts();
    [face(family), &f.excalifont, &f.helvetica]
}

/// Default unitless line height Excalidraw uses for a family.
pub fn default_line_height(family: u32) -> f64 {
    face(family).line_height
}

fn glyph(family: u32, c: char) -> Option<(&'static FontFace, ttf_parser::GlyphId)> {
    for f in chain(family) {
        if let Some(g) = f.face.glyph_index(c) {
            return Some((f, g));
        }
    }
    None
}

fn advance(family: u32, c: char, size: f64) -> f64 {
    if c == '\t' {
        return advance(family, ' ', size) * 4.0;
    }
    match glyph(family, c) {
        Some((f, g)) => {
            f.face.glyph_hor_advance(g).unwrap_or(0) as f64 * size / f.upm as f64
        }
        None => size * 0.5,
    }
}

pub fn line_width(line: &str, family: u32, size: f64) -> f64 {
    line.chars().map(|c| advance(family, c, size)).sum()
}

/// Caret x positions for each char boundary of `line` (len = chars + 1).
pub fn caret_stops(line: &str, family: u32, size: f64) -> Vec<f64> {
    let mut v = vec![0.0];
    let mut x = 0.0;
    for c in line.chars() {
        x += advance(family, c, size);
        v.push(x);
    }
    v
}

/// Width/height of an unwrapped text block (Excalidraw's `measureText`).
pub fn measure(text: &str, family: u32, size: f64, line_height: f64) -> (f64, f64) {
    let lines: Vec<&str> = text.split('\n').collect();
    let w = lines
        .iter()
        .map(|l| line_width(l, family, size))
        .fold(0.0, f64::max);
    (w, lines.len() as f64 * size * line_height)
}

/// Excalidraw's `wrapText`: break on whitespace to fit `max_width`; words
/// longer than a line are broken by characters.
pub fn wrap(text: &str, family: u32, size: f64, max_width: f64) -> String {
    if max_width <= 0.0 {
        return text.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    for para in text.split('\n') {
        if line_width(para, family, size) <= max_width {
            out.push(para.to_string());
            continue;
        }
        let mut line = String::new();
        for word in para.split_inclusive(' ') {
            let candidate = format!("{line}{word}");
            if line_width(candidate.trim_end(), family, size) <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                out.push(line.trim_end().to_string());
                line.clear();
            }
            if line_width(word.trim_end(), family, size) <= max_width {
                line = word.to_string();
            } else {
                for c in word.chars() {
                    let cand = format!("{line}{c}");
                    if !line.is_empty() && line_width(cand.trim_end(), family, size) > max_width {
                        out.push(line.trim_end().to_string());
                        line = c.to_string();
                    } else {
                        line = cand;
                    }
                }
            }
        }
        out.push(line.trim_end().to_string());
    }
    out.join("\n")
}

/// Baseline offset of the first line inside its line box (Excalidraw's
/// `getVerticalOffset`).
pub fn baseline_offset(family: u32, size: f64, line_height_px: f64) -> f64 {
    let f = face(family);
    let em = size / f.excalidraw_upm as f64;
    let line_gap = line_height_px - em * f.ascender as f64 + em * f.descender as f64;
    em * f.ascender as f64 + line_gap / 2.0
}

struct OutlineSink<'a> {
    ops: &'a mut Vec<Op>,
    ox: f32,
    oy: f32,
    scale: f32,
}

impl OutlineSink<'_> {
    fn p(&self, x: f32, y: f32) -> [f32; 2] {
        [self.ox + x * self.scale, self.oy - y * self.scale]
    }
}

impl ttf_parser::OutlineBuilder for OutlineSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.ops.push(Op::Move(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.ops.push(Op::Line(p));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.p(x1, y1), self.p(x, y));
        self.ops.push(Op::Quad(c, p));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        self.ops.push(Op::Cubic(c1, c2, p));
    }
    fn close(&mut self) {
        self.ops.push(Op::Close);
    }
}

/// Glyph outlines of `line` with its baseline at (`x`, `y`), in element-local
/// coordinates.
pub fn outline_line(line: &str, family: u32, size: f64, x: f64, y: f64, ops: &mut Vec<Op>) {
    let mut pen = x;
    for c in line.chars() {
        if let Some((f, g)) = glyph(family, c) {
            let scale = (size / f.upm as f64) as f32;
            let mut sink = OutlineSink {
                ops,
                ox: pen as f32,
                oy: y as f32,
                scale,
            };
            f.face.outline_glyph(g, &mut sink);
        }
        pen += advance(family, c, size);
    }
}

/// Outlines for a whole text element laid out the way Excalidraw's canvas
/// renderer does (per-line alignment within `width`).
pub fn outline_block(
    text: &str,
    family: u32,
    size: f64,
    line_height: f64,
    align: &str,
    width: f64,
) -> Vec<Op> {
    let mut ops = Vec::new();
    let lh = size * line_height;
    let base = baseline_offset(family, size, lh);
    for (i, line) in text.split('\n').enumerate() {
        let w = line_width(line, family, size);
        let x = match align {
            "center" => (width - w) / 2.0,
            "right" => width - w,
            _ => 0.0,
        };
        outline_line(line, family, size, x, i as f64 * lh + base, &mut ops);
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_and_measure() {
        for fam in [1, 2, 3, 5, 6, 7, 8] {
            let w = line_width("Hello", fam, 20.0);
            assert!(w > 20.0 && w < 100.0, "family {fam} width {w}");
        }
        let (w, h) = measure("a\nbb", 5, 20.0, 1.25);
        assert!(w > 0.0);
        assert_eq!(h, 50.0);
    }

    #[test]
    fn wraps() {
        let s = wrap("hello world foo bar", 5, 20.0, 80.0);
        assert!(s.contains('\n'));
        for l in s.lines() {
            assert!(line_width(l, 5, 20.0) <= 80.0 + 0.01, "{l}");
        }
    }
}
