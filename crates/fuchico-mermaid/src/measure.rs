//! Text measurement for pele's layout: the port of the canvas measurer the
//! original uses (`ctx.font = "[italic ][bold ]<size>px <family>"`,
//! `measureText(text).width`). Text is shaped with rustybuzz in the faces
//! [`crate::fonts`] resolves, so measured widths match what resvg paints.

use crate::fonts::{family_of, fontdb, split_families};
use resvg::usvg::fontdb::{Database, Family, ID, Query, Style, Weight};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// pele's `Style` bit flags.
pub const BOLD: u32 = 1;
pub const ITALIC: u32 = 2;
pub const MONO: u32 = 4;

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Drops every thread's cached faces/widths (fonts were added).
pub fn invalidate() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

const WIDTH_CACHE_MAX: usize = 20_000;

/// Fallback families tried, in order, for glyphs the requested face lacks
/// (what the browser's per-character font fallback does).
const FALLBACKS: &[&str] = &[
    "Helvetica Neue",
    "PingFang SC",
    "Hiragino Sans",
    "Apple SD Gothic Neo",
    "Apple Color Emoji",
    "Apple Symbols",
    "Noto Sans",
    "Noto Sans CJK SC",
    "Noto Color Emoji",
    "Segoe UI",
    "Segoe UI Emoji",
    "Arial Unicode MS",
    "DejaVu Sans",
];

#[derive(Default)]
struct Measurer {
    generation: u64,
    sans: Vec<String>,
    mono: Vec<String>,
    faces: HashMap<u32, Vec<ID>>,
    fallback: HashMap<(char, u32), Option<ID>>,
    widths: HashMap<(String, u32, u32), f64>,
}

thread_local! {
    static MEASURER: RefCell<Measurer> = RefCell::new(Measurer::default());
}

/// Sets the `fontFamily` / `fontFamilyMono` lists used by [`width`] on
/// this thread.
pub fn set_families(sans: &str, mono: &str) {
    MEASURER.with(|m| {
        let mut m = m.borrow_mut();
        let (s, mo) = (split_families(sans), split_families(mono));
        if m.sans != s || m.mono != mo {
            m.sans = s;
            m.mono = mo;
            m.faces.clear();
            m.widths.clear();
        }
    });
}

/// Width in px of `text` at `size` px with pele `style` flags.
pub fn width(text: &str, size: f64, style: u32) -> f64 {
    MEASURER.with(|m| m.borrow_mut().width(text, size, style))
}

fn query_style(style: u32) -> (Weight, Style) {
    (
        if style & BOLD != 0 {
            Weight::BOLD
        } else {
            Weight::NORMAL
        },
        if style & ITALIC != 0 {
            Style::Italic
        } else {
            Style::Normal
        },
    )
}

/// The face that draws `c` when the requested ones lack it: the first of
/// [`FALLBACKS`] that has the glyph, else any face but `LastResort` (which
/// "has" every code point and draws boxes). Shared by measurement and the
/// rasterizer's fallback selector so both pick the same face.
pub fn fallback_face(
    db: &Database,
    c: char,
    weight: Weight,
    style: Style,
    exclude: &[ID],
) -> Option<ID> {
    FALLBACKS
        .iter()
        .filter_map(|n| {
            db.query(&Query {
                families: &[Family::Name(n)],
                weight,
                style,
                ..Query::default()
            })
        })
        .filter(|id| !exclude.contains(id))
        .find(|id| has_glyph(db, *id, c))
        .or_else(|| {
            db.faces()
                .filter(|f| {
                    !exclude.contains(&f.id)
                        && !f.families.iter().any(|(n, _)| n.contains("LastResort"))
                })
                .map(|f| f.id)
                .find(|id| has_glyph(db, *id, c))
        })
}

fn has_glyph(db: &Database, id: ID, c: char) -> bool {
    db.with_face_data(id, |data, index| {
        rustybuzz::ttf_parser::Face::parse(data, index)
            .ok()
            .and_then(|f| f.glyph_index(c))
            .is_some()
    })
    .unwrap_or(false)
}

impl Measurer {
    fn sync(&mut self) {
        let g = GENERATION.load(Ordering::SeqCst);
        if g != self.generation {
            self.generation = g;
            self.faces.clear();
            self.fallback.clear();
            self.widths.clear();
        }
    }

    /// Faces of the requested family list, first match per family, in order.
    fn faces_for(&mut self, db: &Database, style: u32) -> Vec<ID> {
        let key = style & (BOLD | ITALIC | MONO);
        if let Some(f) = self.faces.get(&key) {
            return f.clone();
        }
        let names = if style & MONO != 0 {
            &self.mono
        } else {
            &self.sans
        };
        let (weight, fstyle) = query_style(style);
        let mut out = Vec::new();
        let generic = if style & MONO != 0 {
            Family::Monospace
        } else {
            Family::SansSerif
        };
        for fam in names
            .iter()
            .map(|n| family_of(n))
            .chain(std::iter::once(generic))
        {
            if let Some(id) = db.query(&Query {
                families: &[fam],
                weight,
                style: fstyle,
                ..Query::default()
            }) && !out.contains(&id)
            {
                out.push(id);
            }
        }
        self.faces.insert(key, out.clone());
        out
    }

    fn fallback_for(&mut self, db: &Database, c: char, style: u32) -> Option<ID> {
        let key = (c, style & (BOLD | ITALIC));
        if let Some(f) = self.fallback.get(&key) {
            return *f;
        }
        let (weight, fstyle) = query_style(style);
        let found = fallback_face(db, c, weight, fstyle, &[]);
        self.fallback.insert(key, found);
        found
    }

    fn width(&mut self, text: &str, size: f64, style: u32) -> f64 {
        self.sync();
        let key = (text.to_string(), (size * 100.0).round() as u32, style);
        if let Some(w) = self.widths.get(&key) {
            return *w;
        }
        let db = fontdb();
        let faces = self.faces_for(&db, style);
        // Split into runs of characters drawn by the same face.
        let mut runs: Vec<(Option<ID>, String)> = Vec::new();
        for c in text.chars() {
            let face = if c.is_control() {
                faces.first().copied()
            } else {
                faces
                    .iter()
                    .copied()
                    .find(|id| has_glyph(&db, *id, c))
                    .or_else(|| self.fallback_for(&db, c, style))
            };
            match runs.last_mut() {
                Some((f, s)) if *f == face => s.push(c),
                _ => runs.push((face, c.to_string())),
            }
        }
        let bold = style & BOLD != 0;
        let mut total = 0.0;
        for (face, run) in runs {
            total += match face {
                Some(id) => shape_width(&db, id, &run, size, bold),
                // No face at all: pele's own metrics fallback (~0.58 em).
                None => run.chars().count() as f64 * size * 0.58,
            };
        }
        if self.widths.len() >= WIDTH_CACHE_MAX {
            self.widths.clear();
        }
        self.widths.insert(key, total);
        total
    }
}

fn shape_width(db: &Database, id: ID, text: &str, size: f64, bold: bool) -> f64 {
    db.with_face_data(id, |data, index| {
        let mut face = rustybuzz::Face::from_slice(data, index)?;
        if bold
            && face
                .variation_axes()
                .into_iter()
                .any(|a| a.tag == rustybuzz::ttf_parser::Tag::from_bytes(b"wght"))
        {
            face.set_variation(rustybuzz::ttf_parser::Tag::from_bytes(b"wght"), 700.0);
        }
        let upem = face.units_per_em() as f64;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        let out = rustybuzz::shape(&face, &[], buf);
        let adv: i64 = out
            .glyph_positions()
            .iter()
            .map(|p| p.x_advance as i64)
            .sum();
        Some(adv as f64 * size / upem)
    })
    .flatten()
    .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_scale_and_bold_is_wider() {
        set_families("Helvetica Neue, sans-serif", "Menlo, monospace");
        let a = width("Hello world", 16.0, 0);
        let b = width("Hello world", 32.0, 0);
        assert!(a > 40.0 && a < 120.0, "{a}");
        assert!((b - 2.0 * a).abs() < 0.5, "{a} {b}");
        assert!(width("Hello world", 16.0, BOLD) > a);
        let m = width("iiii", 16.0, MONO);
        assert!((m - width("MMMM", 16.0, MONO)).abs() < 0.01, "monospace");
        assert_eq!(width("", 16.0, 0), 0.0);
        // CJK falls back to a face that has the glyphs: about 1em each.
        let cjk = width("日本語", 16.0, 0);
        assert!(cjk > 30.0 && cjk < 60.0, "{cjk}");
    }
}
