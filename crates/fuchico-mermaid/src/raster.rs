//! SVG -> GPUI image, with a small cache keyed by (source hash, theme, scale).

use crate::render::{MermaidError, render_svg};
use crate::theme::MermaidTheme;
use gpui_kit::RenderImage;
use image::{Frame, RgbaImage};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

/// Largest raster side in device pixels (keeps GPU atlas allocations sane).
pub const MAX_RASTER_SIDE: f32 = 8192.0;
const CACHE_CAPACITY: usize = 16;

/// A rasterized diagram. `width`/`height` are the SVG's logical size; the
/// image holds `width * scale` x `height * scale` device pixels.
#[derive(Clone)]
pub struct Raster {
    pub image: Arc<RenderImage>,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

fn options() -> usvg::Options<'static> {
    usvg::Options {
        fontdb: crate::fonts::fontdb(),
        font_family: crate::fonts::default_sans(),
        font_resolver: usvg::FontResolver {
            select_font: usvg::FontResolver::default_font_selector(),
            select_fallback: Box::new(|c, exclude, db| {
                let base = db.face(*exclude.first()?)?;
                crate::measure::fallback_face(db, c, base.weight, base.style, exclude)
            }),
        },
        ..Default::default()
    }
}

/// Quantize a wanted render scale so zooming does not re-rasterize on every
/// wheel tick: round up to the next multiple of 0.5 (min 1.0).
pub fn quantize_scale(wanted: f32) -> f32 {
    ((wanted.max(0.1) * 2.0).ceil() / 2.0).max(1.0)
}

/// Rasterize `svg` at `scale` (device pixels per logical px). Transparent
/// background, premultiplied BGRA as GPUI expects.
pub fn rasterize(svg: &str, scale: f32) -> Result<Raster, MermaidError> {
    let tree = usvg::Tree::from_str(svg, &options()).map_err(|e| MermaidError {
        message: format!("Invalid SVG: {e}"),
        line: None,
    })?;
    let size = tree.size();
    let (w, h) = (size.width().max(1.0), size.height().max(1.0));
    let scale = scale.min(MAX_RASTER_SIDE / w.max(h)).max(0.01);
    let (pw, ph) = ((w * scale).ceil() as u32, (h * scale).ceil() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(pw.max(1), ph.max(1)).ok_or_else(|| MermaidError {
        message: "Diagram too large to rasterize".into(),
        line: None,
    })?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut data = pixmap.take();
    for px in data.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let buf = RgbaImage::from_raw(pw.max(1), ph.max(1), data).expect("pixmap size matches");
    Ok(Raster {
        image: Arc::new(RenderImage::new(vec![Frame::new(buf)])),
        width: w,
        height: h,
        scale,
    })
}

fn hash_of<T: Hash + ?Sized>(v: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RasterKey {
    pub source: u64,
    pub theme: u64,
    /// Scale in 1/100 steps.
    pub scale: u32,
}

impl RasterKey {
    pub fn new(source: &str, theme: &MermaidTheme, scale: f32) -> Self {
        Self {
            source: hash_of(source),
            theme: hash_of(theme),
            scale: (scale * 100.0).round() as u32,
        }
    }
}

#[derive(Default)]
struct Cache {
    map: HashMap<RasterKey, Raster>,
    order: Vec<RasterKey>,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Cached lookup only.
pub fn cached(source: &str, theme: &MermaidTheme, scale: f32) -> Option<Raster> {
    let key = RasterKey::new(source, theme, scale);
    let mut c = cache().lock().ok()?;
    let hit = c.map.get(&key).cloned()?;
    c.order.retain(|k| *k != key);
    c.order.push(key);
    Some(hit)
}

/// Render `source` and rasterize at `scale`, going through the cache.
/// `svg` may be supplied when the caller already has it for this source/theme.
pub fn rasterize_cached(
    source: &str,
    theme: &MermaidTheme,
    svg: Option<&str>,
    scale: f32,
) -> Result<Raster, MermaidError> {
    if let Some(r) = cached(source, theme, scale) {
        return Ok(r);
    }
    let r = match svg {
        Some(s) => rasterize(s, scale)?,
        None => rasterize(&render_svg(source, theme)?, scale)?,
    };
    let key = RasterKey::new(source, theme, scale);
    if let Ok(mut c) = cache().lock() {
        c.map.insert(key, r.clone());
        c.order.retain(|k| *k != key);
        c.order.push(key);
        while c.order.len() > CACHE_CAPACITY {
            let old = c.order.remove(0);
            c.map.remove(&old);
        }
    }
    Ok(r)
}

/// Number of cached rasters (for tests/diagnostics).
pub fn cache_len() -> usize {
    cache().lock().map(|c| c.map.len()).unwrap_or(0)
}

/// Tint a monochrome lucide icon (`currentColor`) and rasterize it.
pub fn rasterize_icon(svg: &str, color_hex: &str, px: f32, scale: f32) -> Option<Arc<RenderImage>> {
    let tinted = svg
        .replace("currentColor", color_hex)
        .replace("width=\"24\"", &format!("width=\"{px}\""))
        .replace("height=\"24\"", &format!("height=\"{px}\""));
    rasterize(&tinted, scale).ok().map(|r| r.image)
}
