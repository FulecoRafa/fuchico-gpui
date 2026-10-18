//! Painting: element shapes → GPUI paths (with Excalidraw's dark-mode canvas
//! filter), images, frames, and the interactive overlay primitives.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{
    fill, point, px, Bounds, ContentMask, Corners, PathBuilder, Pixels, Point,
    RenderImage, Window,
};

use crate::color::{self, Rgba};
use crate::geom::{self, P};
use crate::model::Element;
use crate::scene::Scene;
use crate::shape::{self, Ink, Op, Paint, Shape};
use crate::text;

/// Excalidraw's viewport: `screen = origin + (world + scroll) * zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { scroll_x: 0.0, scroll_y: 0.0, zoom: 1.0 }
    }
}

impl Camera {
    pub fn to_screen(&self, origin: Point<Pixels>, w: P) -> Point<Pixels> {
        point(
            origin.x + px(((w[0] + self.scroll_x) * self.zoom) as f32),
            origin.y + px(((w[1] + self.scroll_y) * self.zoom) as f32),
        )
    }

    pub fn to_world(&self, origin: Point<Pixels>, s: Point<Pixels>) -> P {
        [
            f64::from(f32::from(s.x - origin.x)) / self.zoom - self.scroll_x,
            f64::from(f32::from(s.y - origin.y)) / self.zoom - self.scroll_y,
        ]
    }

    /// Zooms to `zoom`, keeping the world point under `anchor` (screen,
    /// relative to the canvas origin) fixed.
    pub fn zoom_at(&mut self, zoom: f64, anchor: [f64; 2]) {
        let zoom = zoom.clamp(0.1, 30.0);
        let world = [anchor[0] / self.zoom - self.scroll_x, anchor[1] / self.zoom - self.scroll_y];
        self.zoom = zoom;
        self.scroll_x = anchor[0] / zoom - world[0];
        self.scroll_y = anchor[1] / zoom - world[1];
    }
}

/// Colour resolution for the current theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// The canvas background as drawn (already filtered in dark mode).
    pub canvas: Rgba,
}

impl Palette {
    pub fn resolve(&self, css: &str) -> Rgba {
        let c = color::parse(css).unwrap_or(Rgba::new(0.0, 0.0, 0.0, 1.0));
        if self.dark { c.dark_filter() } else { c }
    }
}

/// Cached shapes and decoded images, shared between frames.
#[derive(Default)]
pub struct Caches {
    shapes: HashMap<String, (u64, Rc<Shape>)>,
    images: HashMap<String, Option<Decoded>>,
    transformed: HashMap<(String, u64), Option<Arc<RenderImage>>>,
}

#[derive(Clone)]
struct Decoded {
    rgba: Arc<image::RgbaImage>,
    plain: Arc<RenderImage>,
}

pub type SharedCaches = Rc<RefCell<Caches>>;

impl Caches {
    pub fn shape(&mut self, el: &Element) -> Rc<Shape> {
        let key = shape::shape_key(el);
        if let Some((k, s)) = self.shapes.get(&el.id)
            && *k == key {
                return s.clone();
            }
        let s = Rc::new(shape::generate(el));
        self.shapes.insert(el.id.clone(), (key, s.clone()));
        s
    }

    pub fn retain(&mut self, scene: &Scene) {
        if self.shapes.len() > scene.items.len() * 2 + 64 {
            let ids: std::collections::HashSet<&str> = scene.elements().map(|e| e.id.as_str()).collect();
            self.shapes.retain(|k, _| ids.contains(k.as_str()));
        }
        if self.transformed.len() > 64 {
            self.transformed.clear();
        }
    }

    fn decoded(&mut self, scene: &Scene, file_id: &str) -> Option<Decoded> {
        if let Some(d) = self.images.get(file_id) {
            return d.clone();
        }
        let d = scene.file_data_url(file_id).and_then(decode_data_url).map(|rgba| {
            let plain = Arc::new(to_render_image(&rgba));
            Decoded { rgba: Arc::new(rgba), plain }
        });
        self.images.insert(file_id.to_string(), d.clone());
        d
    }

    /// Natural size of an image file, if decodable.
    pub fn image_size(&mut self, scene: &Scene, file_id: &str) -> Option<(u32, u32)> {
        self.decoded(scene, file_id).map(|d| (d.rgba.width(), d.rgba.height()))
    }
}

pub fn decode_data_url(url: &str) -> Option<image::RgbaImage> {
    use base64::Engine;
    let (_, data) = url.split_once(',')?;
    let bytes = if url[..url.find(',')?].contains(";base64") {
        base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()?
    } else {
        data.as_bytes().to_vec()
    };
    if url[..url.find(',')?].contains("image/svg") || looks_like_svg(&bytes) {
        return rasterize_svg(&bytes, 2048);
    }
    image::load_from_memory(&bytes).ok().map(|i| i.to_rgba8())
}

/// Whether `bytes` is SVG markup.
pub fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    head.contains("<svg")
}

/// Rasterises SVG at its intrinsic size (longest side capped at `max`, at
/// least 2x for crispness up to that cap), unpremultiplied RGBA.
pub fn rasterize_svg(bytes: &[u8], max: u32) -> Option<image::RgbaImage> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let sz = tree.size();
    let (w, h) = (sz.width().max(1.0), sz.height().max(1.0));
    let scale = (2.0f32).min(max as f32 / w.max(h));
    let (pw, ph) = ((w * scale).ceil().max(1.0) as u32, (h * scale).ceil().max(1.0) as u32);
    let mut pm = resvg::tiny_skia::Pixmap::new(pw, ph)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
    let mut out = image::RgbaImage::new(pw, ph);
    for (o, p) in out.pixels_mut().zip(pm.pixels()) {
        let c = p.demultiply();
        o.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    Some(out)
}

/// Intrinsic (CSS px) size of SVG markup.
pub fn svg_size(bytes: &[u8]) -> Option<(f64, f64)> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    Some((tree.size().width() as f64, tree.size().height() as f64))
}

fn to_render_image(rgba: &image::RgbaImage) -> RenderImage {
    let mut bgra = rgba.clone();
    for p in bgra.pixels_mut() {
        p.0.swap(0, 2);
    }
    RenderImage::new(vec![image::Frame::new(bgra)])
}

/// Maps element-local points to screen pixels.
pub struct Xf {
    origin: Point<Pixels>,
    cam: Camera,
    ox: f64,
    oy: f64,
    c: P,
    sin: f64,
    cos: f64,
}

impl Xf {
    pub fn new(el: &Element, cam: Camera, origin: Point<Pixels>) -> Self {
        let (sin, cos) = el.angle().sin_cos();
        Self { origin, cam, ox: el.x(), oy: el.y(), c: geom::center(el), sin, cos }
    }

    pub fn world(&self, p: [f32; 2]) -> P {
        let (wx, wy) = (self.ox + p[0] as f64, self.oy + p[1] as f64);
        let (dx, dy) = (wx - self.c[0], wy - self.c[1]);
        [self.c[0] + dx * self.cos - dy * self.sin, self.c[1] + dx * self.sin + dy * self.cos]
    }

    pub fn screen(&self, p: [f32; 2]) -> Point<Pixels> {
        self.cam.to_screen(self.origin, self.world(p))
    }
}

pub fn build_path(ops: &[Op], xf: &Xf, paint: &Paint, zoom: f64) -> Option<gpui_kit::Path<Pixels>> {
    let mut b = match paint {
        Paint::Fill => PathBuilder::fill(),
        Paint::Stroke { width, dash } => {
            let w = px((*width as f64 * zoom).max(0.5) as f32);
            if let Some([a, g]) = dash {
                // GPUI's dash_array (lyon's measure) can panic on some
                // multi-subpath inputs; dash the flattened path ourselves.
                let polys = flatten(ops, |p| xf.screen(p));
                return dashed(&polys, w, (*a as f64 * zoom) as f32, (*g as f64 * zoom) as f32);
            }
            PathBuilder::stroke(w)
        }
    };
    let mut started = false;
    for op in ops {
        match *op {
            Op::Move(p) => {
                b.move_to(xf.screen(p));
                started = true;
            }
            Op::Line(p) => {
                if !started {
                    b.move_to(xf.screen(p));
                    started = true;
                } else {
                    b.line_to(xf.screen(p));
                }
            }
            Op::Quad(c, p) => b.curve_to(xf.screen(p), xf.screen(c)),
            Op::Cubic(c1, c2, p) => b.cubic_bezier_to(xf.screen(p), xf.screen(c1), xf.screen(c2)),
            Op::Close => b.close(),
        }
    }
    if !started {
        return None;
    }
    b.build().ok()
}

/// Flattens path ops into screen-space polylines (one per subpath).
fn flatten(ops: &[Op], map: impl Fn([f32; 2]) -> Point<Pixels>) -> Vec<Vec<Point<Pixels>>> {
    const N: usize = 12;
    let mut out: Vec<Vec<Point<Pixels>>> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    let mut start = [0.0f32; 2];
    let flush = |cur: &mut Vec<[f32; 2]>, out: &mut Vec<Vec<Point<Pixels>>>| {
        if cur.len() > 1 {
            out.push(cur.iter().map(|q| map(*q)).collect());
        }
        cur.clear();
    };
    for op in ops {
        let last = cur.last().copied().unwrap_or(start);
        match *op {
            Op::Move(p) => {
                flush(&mut cur, &mut out);
                start = p;
                cur.push(p);
            }
            Op::Line(p) => {
                if cur.is_empty() {
                    cur.push(last);
                }
                cur.push(p)
            }
            Op::Quad(c, p) => {
                if cur.is_empty() {
                    cur.push(last);
                }
                for k in 1..=N {
                    let t = k as f32 / N as f32;
                    let u = 1.0 - t;
                    cur.push([
                        u * u * last[0] + 2.0 * u * t * c[0] + t * t * p[0],
                        u * u * last[1] + 2.0 * u * t * c[1] + t * t * p[1],
                    ]);
                }
            }
            Op::Cubic(c1, c2, p) => {
                if cur.is_empty() {
                    cur.push(last);
                }
                for k in 1..=N {
                    let t = k as f32 / N as f32;
                    let u = 1.0 - t;
                    let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
                    cur.push([f(last[0], c1[0], c2[0], p[0]), f(last[1], c1[1], c2[1], p[1])]);
                }
            }
            Op::Close => {
                cur.push(start);
                flush(&mut cur, &mut out);
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// One stroke path made of the "on" pieces of `polys` dashed by `on`/`off`.
fn dashed(polys: &[Vec<Point<Pixels>>], width: Pixels, on: f32, off: f32) -> Option<gpui_kit::Path<Pixels>> {
    let (on, off) = (on.max(0.5), off.max(0.5));
    let mut b = PathBuilder::stroke(width);
    let mut any = false;
    for poly in polys {
        let mut drawing = true;
        let mut left = on;
        let mut pen_down = false;
        for w in poly.windows(2) {
            let (a, c) = (w[0], w[1]);
            let (dx, dy) = (f32::from(c.x - a.x), f32::from(c.y - a.y));
            let len = (dx * dx + dy * dy).sqrt();
            let mut pos = 0.0f32;
            if drawing && !pen_down {
                b.move_to(a);
                pen_down = true;
            }
            while len - pos > left {
                pos += left;
                let q = point(a.x + px(dx * pos / len), a.y + px(dy * pos / len));
                if drawing {
                    b.line_to(q);
                    any = true;
                    pen_down = false;
                } else {
                    b.move_to(q);
                    pen_down = true;
                }
                drawing = !drawing;
                left = if drawing { on } else { off };
            }
            left -= len - pos;
            if drawing {
                b.line_to(c);
                any = true;
            }
        }
    }
    if !any {
        return None;
    }
    b.build().ok()
}

/// Paints one element (shape layers, images, placeholders).
#[allow(clippy::too_many_arguments)]
pub fn paint_element(
    el: &Element,
    scene: &Scene,
    caches: &mut Caches,
    cam: Camera,
    origin: Point<Pixels>,
    pal: Palette,
    alpha: f32,
    window: &mut Window,
) {
    let opacity = (el.opacity() / 100.0).clamp(0.0, 1.0) as f32 * alpha;
    match el.kind.as_str() {
        "image" => {
            paint_image(el, scene, caches, cam, origin, opacity, window);
            return;
        }
        "frame" | "magicframe" => return, // painted in a later pass
        "rectangle" | "diamond" | "ellipse" | "line" | "arrow" | "freedraw" | "text" => {}
        _ => {
            paint_placeholder(el, cam, origin, pal, window);
            return;
        }
    }
    let shape = caches.shape(el);
    let xf = Xf::new(el, cam, origin);
    let stroke = pal.resolve(el.stroke_color());
    let bg = pal.resolve(el.background_color());
    for layer in &shape.layers {
        let c = match layer.ink {
            Ink::Stroke => stroke,
            Ink::Background => bg,
            Ink::Canvas => pal.canvas,
        };
        if c.a == 0.0 {
            continue;
        }
        if let Some(path) = build_path(&layer.ops, &xf, &layer.paint, cam.zoom) {
            window.paint_path(path, c.with_alpha(opacity).to_hsla());
        }
    }
}

fn paint_placeholder(el: &Element, cam: Camera, origin: Point<Pixels>, pal: Palette, window: &mut Window) {
    let b = geom::local_bounds(el);
    let xf = Xf::new(el, cam, origin);
    let ops = [
        Op::Move([b[0] as f32, b[1] as f32]),
        Op::Line([b[2] as f32, b[1] as f32]),
        Op::Line([b[2] as f32, b[3] as f32]),
        Op::Line([b[0] as f32, b[3] as f32]),
        Op::Close,
    ];
    let paint = Paint::Stroke { width: 1.0, dash: Some([6.0, 4.0]) };
    if let Some(p) = build_path(&ops, &xf, &paint, cam.zoom) {
        window.paint_path(p, pal.resolve("#868e96").to_hsla());
    }
}

fn paint_image(
    el: &Element,
    scene: &Scene,
    caches: &mut Caches,
    cam: Camera,
    origin: Point<Pixels>,
    opacity: f32,
    window: &mut Window,
) {
    let file_id = el.f.file_id.get().cloned().unwrap_or_default();
    let Some(dec) = caches.decoded(scene, &file_id) else {
        // Excalidraw shows an image placeholder while (or if) the file is missing.
        let b = geom::aabb(el);
        let tl = cam.to_screen(origin, [b[0], b[1]]);
        let br = cam.to_screen(origin, [b[2], b[3]]);
        window.paint_quad(fill(Bounds::from_corners(tl, br), Rgba::new(0.5, 0.5, 0.5, 0.15 * opacity).to_hsla()));
        return;
    };
    let _ = opacity; // GPUI's image sprites have no alpha multiplier.
    let scale = el
        .f
        .scale
        .get()
        .map(|s| (s[0].0 < 0.0, s[1].0 < 0.0))
        .unwrap_or((false, false));
    let (w, h) = (el.width().abs(), el.height().abs());
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let img = if el.angle() == 0.0 && !scale.0 && !scale.1 {
        Some(dec.plain.clone())
    } else {
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            el.angle().to_bits().hash(&mut hs);
            w.to_bits().hash(&mut hs);
            h.to_bits().hash(&mut hs);
            scale.hash(&mut hs);
            hs.finish()
        };
        caches
            .transformed
            .entry((file_id.clone(), key))
            .or_insert_with(|| Some(Arc::new(transform_image(&dec.rgba, w, h, el.angle(), scale))))
            .clone()
    };
    let Some(img) = img else { return };
    let b = geom::aabb(el);
    let tl = cam.to_screen(origin, [b[0], b[1]]);
    let br = cam.to_screen(origin, [b[2], b[3]]);
    let bounds = Bounds::from_corners(tl, br);
    let _ = window.paint_image(bounds, bounds, Corners::default(), img, 0, false);
}

/// Resamples `src` stretched to `w`×`h`, flipped and rotated by `angle`, into
/// its axis-aligned bounding box.
fn transform_image(src: &image::RgbaImage, w: f64, h: f64, angle: f64, flip: (bool, bool)) -> RenderImage {
    let k = (src.width() as f64 / w).min(src.height() as f64 / h).min(2048.0 / w.max(h)).max(0.05);
    let (tw, th) = (w * k, h * k);
    let (s, c) = angle.sin_cos();
    let bw = (tw * c.abs() + th * s.abs()).ceil().max(1.0) as u32;
    let bh = (tw * s.abs() + th * c.abs()).ceil().max(1.0) as u32;
    let mut out = image::RgbaImage::new(bw, bh);
    let (cx, cy) = (bw as f64 / 2.0, bh as f64 / 2.0);
    for y in 0..bh {
        for x in 0..bw {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            // inverse rotation
            let lx = dx * c + dy * s + tw / 2.0;
            let ly = -dx * s + dy * c + th / 2.0;
            if lx < 0.0 || ly < 0.0 || lx >= tw || ly >= th {
                continue;
            }
            let mut u = lx / tw;
            let mut v = ly / th;
            if flip.0 {
                u = 1.0 - u;
            }
            if flip.1 {
                v = 1.0 - v;
            }
            let sx = ((u * src.width() as f64) as u32).min(src.width() - 1);
            let sy = ((v * src.height() as f64) as u32).min(src.height() - 1);
            out.put_pixel(x, y, *src.get_pixel(sx, sy));
        }
    }
    to_render_image(&out)
}

/// Frame border + name (drawn after the elements).
pub fn paint_frame(el: &Element, cam: Camera, origin: Point<Pixels>, pal: Palette, window: &mut Window) {
    let (w, h) = (el.width() as f32, el.height() as f32);
    let r = 8.0f32.min(w.abs() / 2.0).min(h.abs() / 2.0);
    let ops = [
        Op::Move([r, 0.0]),
        Op::Line([w - r, 0.0]),
        Op::Quad([w, 0.0], [w, r]),
        Op::Line([w, h - r]),
        Op::Quad([w, h], [w - r, h]),
        Op::Line([r, h]),
        Op::Quad([0.0, h], [0.0, h - r]),
        Op::Line([0.0, r]),
        Op::Quad([0.0, 0.0], [r, 0.0]),
        Op::Close,
    ];
    let xf = Xf::new(el, cam, origin);
    let paint = Paint::Stroke { width: (2.0 / cam.zoom) as f32, dash: None };
    let color = pal.resolve("#bbbbbb");
    if let Some(p) = build_path(&ops, &xf, &paint, cam.zoom) {
        window.paint_path(p, color.to_hsla());
    }
    // Name label in screen-constant 14px above the top-left corner.
    let name = el.f.name.get().cloned().unwrap_or_else(|| "Frame".into());
    let tl = cam.to_screen(origin, geom::to_world(el, [0.0, 0.0]));
    let size = 14.0;
    let mut ops = Vec::new();
    text::outline_line(&name, 2, size, 0.0, -6.0, &mut ops);
    paint_screen_ops(&ops, tl, pal.resolve(if pal.dark { "#7a7a7a" } else { "#999999" }), window);
}

/// Fills `ops` given in screen-pixel offsets from `at`.
pub fn paint_screen_ops(ops: &[Op], at: Point<Pixels>, color: Rgba, window: &mut Window) {
    let mut b = PathBuilder::fill();
    let p = |q: [f32; 2]| point(at.x + px(q[0]), at.y + px(q[1]));
    let mut any = false;
    for op in ops {
        match *op {
            Op::Move(q) => {
                b.move_to(p(q));
                any = true;
            }
            Op::Line(q) => b.line_to(p(q)),
            Op::Quad(c, q) => b.curve_to(p(q), p(c)),
            Op::Cubic(c1, c2, q) => b.cubic_bezier_to(p(q), p(c1), p(c2)),
            Op::Close => b.close(),
        }
    }
    if any
        && let Ok(path) = b.build() {
            window.paint_path(path, color.to_hsla());
        }
}

/// Content mask for a frame's children (frames are never rotated in
/// Excalidraw, so an axis-aligned clip is exact).
pub fn frame_mask(el: &Element, cam: Camera, origin: Point<Pixels>) -> ContentMask<Pixels> {
    let b = geom::aabb(el);
    let tl = cam.to_screen(origin, [b[0], b[1]]);
    let br = cam.to_screen(origin, [b[2], b[3]]);
    ContentMask { bounds: Bounds::from_corners(tl, br) }
}

/// A polyline/polygon in screen space.
pub fn stroke_screen(points: &[Point<Pixels>], closed: bool, width: f32, dash: Option<[f32; 2]>, color: Rgba, window: &mut Window) {
    if points.len() < 2 {
        return;
    }
    let path = if let Some([a, g]) = dash {
        let mut poly = points.to_vec();
        if closed {
            poly.push(points[0]);
        }
        dashed(&[poly], px(width), a, g)
    } else {
        let mut b = PathBuilder::stroke(px(width));
        b.move_to(points[0]);
        for p in &points[1..] {
            b.line_to(*p);
        }
        if closed {
            b.close();
        }
        b.build().ok()
    };
    if let Some(p) = path {
        window.paint_path(p, color.to_hsla());
    }
}

/// A selection handle: filled square/circle with an outline.
pub fn handle(at: Point<Pixels>, half: f32, circle: bool, fill_c: Rgba, stroke: Rgba, window: &mut Window) {
    let mut f = PathBuilder::fill();
    let mut s = PathBuilder::stroke(px(1.0));
    let pts: Vec<Point<Pixels>> = if circle {
        (0..16)
            .map(|k| {
                let a = k as f32 / 16.0 * std::f32::consts::TAU;
                point(at.x + px(half * a.cos()), at.y + px(half * a.sin()))
            })
            .collect()
    } else {
        vec![
            point(at.x - px(half), at.y - px(half)),
            point(at.x + px(half), at.y - px(half)),
            point(at.x + px(half), at.y + px(half)),
            point(at.x - px(half), at.y + px(half)),
        ]
    };
    for b in [&mut f, &mut s] {
        b.move_to(pts[0]);
        for p in &pts[1..] {
            b.line_to(*p);
        }
        b.close();
    }
    if let Ok(p) = f.build() {
        window.paint_path(p, fill_c.to_hsla());
    }
    if let Ok(p) = s.build() {
        window.paint_path(p, stroke.to_hsla());
    }
}

/// Canvas background quad.
pub fn paint_background(bounds: Bounds<Pixels>, c: Rgba, window: &mut Window) {
    window.paint_quad(fill(bounds, c.to_hsla()));
}

