//! Export to SVG and PNG, optionally embedding the scene the way Excalidraw
//! does so the file can be opened again as a drawing:
//!
//! * SVG: `<!-- svg-source:excalidraw -->` plus a `<metadata>` payload
//!   (base64 of `{"version":"1","encoding":"bstring","compressed":false,"encoded":…}`).
//! * PNG: a `tEXt` chunk with keyword `application/vnd.excalidraw+json`
//!   holding the same JSON wrapper.
//!
//! Shapes are written from the same geometry the canvas paints (rough.js
//! ops, glyph outlines for text), so exports match the editor exactly.

use std::collections::HashSet;
use std::fmt::Write as _;

use base64::Engine as _;
use serde_json::{json, Map, Value};

use crate::color::{self, Rgba};
use crate::geom;
use crate::model::Element;
use crate::render::Palette;
use crate::scene::Scene;
use crate::shape::{self, Ink, Op, Paint};

pub const EXCALIDRAW_MIME: &str = "application/vnd.excalidraw+json";

/// What and how to export.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions {
    /// Paint the canvas background colour (otherwise transparent).
    pub background: bool,
    /// Dark theme (Excalidraw's `exportWithDarkMode`).
    pub dark: bool,
    /// Embed the scene so the export can be re-opened as a drawing.
    pub embed_scene: bool,
    /// PNG scale factor (1, 2, 3 in Excalidraw's dialog).
    pub scale: f64,
    /// Export only the given element ids (`None`: the whole scene).
    pub only: Option<HashSet<String>>,
    /// Padding around the content (Excalidraw: 10).
    pub padding: f64,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { background: true, dark: false, embed_scene: false, scale: 1.0, only: None, padding: 10.0 }
    }
}

/// The elements an export covers (selection plus bound text and frame children).
pub fn export_elements<'a>(scene: &'a Scene, only: Option<&HashSet<String>>) -> Vec<&'a Element> {
    let Some(ids) = only else { return scene.elements().collect() };
    let mut set = ids.clone();
    for el in scene.elements() {
        let contained = el.container_id().map(|c| ids.contains(c)).unwrap_or(false);
        let framed = el.f.frame_id.get().map(|f| ids.contains(f)).unwrap_or(false);
        if contained || framed {
            set.insert(el.id.clone());
        }
    }
    scene.elements().filter(|e| set.contains(&e.id)).collect()
}

/// The scene (or a subset) as `.excalidraw` JSON, for embedding.
pub fn scene_json(scene: &Scene, only: Option<&HashSet<String>>) -> String {
    let els = export_elements(scene, only);
    let ids: HashSet<&str> = els.iter().filter_map(|e| e.f.file_id.get().map(|s| s.as_str())).collect();
    let files: Map<String, Value> = scene
        .files
        .iter()
        .filter(|(k, _)| ids.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut app = Map::new();
    let st = scene.app_state();
    app.insert("gridSize".into(), st.and_then(|a| a.get("gridSize")).cloned().unwrap_or(Value::from(20)));
    app.insert("viewBackgroundColor".into(), Value::from(scene.view_background()));
    let v = json!({
        "type": "excalidraw",
        "version": 2,
        "source": "fuchico",
        "elements": els.iter().filter_map(|e| serde_json::to_value(e).ok()).collect::<Vec<_>>(),
        "appState": app,
        "files": files,
    });
    serde_json::to_string(&v).unwrap_or_default()
}

/// UTF-8 → "byte string" (one char per byte), as `TextEncoder` + `String.fromCharCode`.
fn to_bstring(s: &str) -> String {
    s.as_bytes().iter().map(|b| *b as char).collect()
}

fn from_bstring(s: &str) -> Option<String> {
    let bytes: Option<Vec<u8>> = s.chars().map(|c| u8::try_from(c as u32).ok()).collect();
    String::from_utf8(bytes?).ok()
}

/// Excalidraw's `encode({ text, compress: false })` wrapper, as a byte string
/// (every char ≤ U+00FF, so it can be written as Latin-1).
fn encode_payload(scene_json: &str) -> String {
    let wrapper = json!({
        "version": "1",
        "encoding": "bstring",
        "compressed": false,
        "encoded": to_bstring(scene_json),
    });
    // serde escapes nothing above ASCII, so the wrapper is itself a byte string.
    serde_json::to_string(&wrapper).unwrap_or_default()
}

fn decode_payload(wrapper: &str) -> Option<String> {
    let v: Value = serde_json::from_str(wrapper).ok()?;
    let enc = v.get("encoded")?.as_str()?;
    if v.get("compressed").and_then(|c| c.as_bool()).unwrap_or(false) {
        return None; // deflated payloads (Excalidraw web) aren't supported
    }
    match v.get("encoding").and_then(|e| e.as_str()) {
        Some("bstring") | None => from_bstring(enc),
        _ => Some(enc.to_string()),
    }
}

fn latin1(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

/// The scene JSON embedded in an SVG exported with `embed_scene`.
pub fn decode_embedded_svg(svg: &str) -> Option<String> {
    if !svg.contains("svg-source:excalidraw") {
        return None;
    }
    let start = svg.find("<!-- payload-start -->")? + "<!-- payload-start -->".len();
    let end = svg[start..].find("<!-- payload-end -->")? + start;
    let raw = base64::engine::general_purpose::STANDARD.decode(svg[start..end].trim()).ok()?;
    let wrapper: String = raw.iter().map(|b| *b as char).collect();
    decode_payload(&wrapper)
}

/// The scene JSON embedded in a PNG exported with `embed_scene`.
pub fn decode_embedded_png(png: &[u8]) -> Option<String> {
    let mut i = 8;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let ty = &png[i + 4..i + 8];
        let data = png.get(i + 8..i + 8 + len)?;
        if ty == b"tEXt"
            && let Some(nul) = data.iter().position(|b| *b == 0)
                && &data[..nul] == EXCALIDRAW_MIME.as_bytes() {
                    let text: String = data[nul + 1..].iter().map(|b| *b as char).collect();
                    return decode_payload(&text);
                }
        i += 12 + len;
    }
    None
}

// ---------------------------------------------------------------- SVG

fn fmt(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() { format!("{}", r as i64) } else { format!("{r}") }
}

fn path_d(ops: &[Op]) -> String {
    let mut d = String::new();
    let f = |v: f32| fmt(v as f64);
    for op in ops {
        match *op {
            Op::Move(p) => write!(d, "M{} {} ", f(p[0]), f(p[1])),
            Op::Line(p) => write!(d, "L{} {} ", f(p[0]), f(p[1])),
            Op::Quad(c, p) => write!(d, "Q{} {} {} {} ", f(c[0]), f(c[1]), f(p[0]), f(p[1])),
            Op::Cubic(a, b, p) => write!(d, "C{} {} {} {} {} {} ", f(a[0]), f(a[1]), f(b[0]), f(b[1]), f(p[0]), f(p[1])),
            Op::Close => write!(d, "Z "),
        }
        .ok();
    }
    d.trim_end().to_string()
}

fn css(c: Rgba) -> (String, Option<f32>) {
    (c.to_hex(), if c.a < 1.0 { Some(c.a) } else { None })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Content bounds of an export (world coordinates, before padding).
pub fn export_bounds(els: &[&Element]) -> Option<[f64; 4]> {
    els.iter().map(|e| {
        let b = geom::aabb(e);
        let sw = e.stroke_width() / 2.0;
        [b[0] - sw, b[1] - sw, b[2] + sw, b[3] + sw]
    }).reduce(geom::union)
}

/// Data URLs that resvg can't decode (no raster-image features) are
/// re-encoded as PNG for rasterising.
fn png_data_url(url: &str) -> Option<String> {
    if url.starts_with("data:image/png") || url.starts_with("data:image/svg") {
        return Some(url.to_string());
    }
    let img = crate::render::decode_data_url(url)?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

fn write_image(out: &mut String, el: &Element, scene: &Scene, for_raster: bool) {
    let Some(fid) = el.f.file_id.get() else { return };
    let Some(url) = scene.file_data_url(fid) else { return };
    let url = if for_raster { png_data_url(url).unwrap_or_default() } else { url.to_string() };
    let (w, h) = (el.width().abs(), el.height().abs());
    let (fx, fy) = el.f.scale.get().map(|s| (s[0].0 < 0.0, s[1].0 < 0.0)).unwrap_or((false, false));
    let flip = if fx || fy {
        format!(
            " transform=\"translate({} {}) scale({} {})\"",
            if fx { fmt(w) } else { "0".into() },
            if fy { fmt(h) } else { "0".into() },
            if fx { -1 } else { 1 },
            if fy { -1 } else { 1 }
        )
    } else {
        String::new()
    };
    match crop_of(el) {
        Some(c) => {
            let _ = write!(
                out,
                "<svg width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" preserveAspectRatio=\"none\" overflow=\"hidden\"{flip}><image href=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\"/></svg>",
                fmt(w), fmt(h), fmt(c.x), fmt(c.y), fmt(c.width), fmt(c.height), esc(&url), fmt(c.natural_width), fmt(c.natural_height)
            );
        }
        None => {
            let _ = write!(
                out,
                "<image href=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\"{flip}/>",
                esc(&url), fmt(w), fmt(h)
            );
        }
    }
}

/// An image element's crop rectangle (natural-pixel coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub natural_width: f64,
    pub natural_height: f64,
}

pub fn crop_of(el: &Element) -> Option<Crop> {
    let c = el.f.crop.get()?.as_object()?;
    let g = |k: &str| c.get(k).and_then(|v| v.as_f64());
    let crop = Crop {
        x: g("x")?,
        y: g("y")?,
        width: g("width")?,
        height: g("height")?,
        natural_width: g("naturalWidth")?,
        natural_height: g("naturalHeight")?,
    };
    (crop.width > 0.0 && crop.height > 0.0).then_some(crop)
}

fn write_element(out: &mut String, el: &Element, scene: &Scene, pal: Palette, for_raster: bool) {
    let lc = geom::local_center(el);
    let opacity = (el.opacity() / 100.0).clamp(0.0, 1.0);
    let _ = write!(out, "<g transform=\"translate({} {})", fmt(el.x()), fmt(el.y()));
    if el.angle() != 0.0 {
        let _ = write!(out, " rotate({} {} {})", fmt(el.angle().to_degrees()), fmt(lc[0]), fmt(lc[1]));
    }
    out.push('"');
    if opacity < 1.0 {
        let _ = write!(out, " opacity=\"{}\"", fmt(opacity));
    }
    out.push('>');
    if is_arrow_label(el, scene) {
        let _ = write!(
            out,
            "<rect x=\"-4\" y=\"-4\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            fmt(el.width() + 8.0), fmt(el.height() + 8.0), pal.canvas.to_hex()
        );
    }
    match el.kind.as_str() {
        "image" => write_image(out, el, scene, for_raster),
        "frame" | "magicframe" => {
            let (w, h) = (el.width(), el.height());
            let _ = write!(
                out,
                "<rect width=\"{}\" height=\"{}\" rx=\"8\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\"/>",
                fmt(w), fmt(h), pal.resolve("#bbbbbb").to_hex()
            );
        }
        "rectangle" | "diamond" | "ellipse" | "line" | "arrow" | "freedraw" | "text" => {
            let s = shape::generate(el);
            let stroke = pal.resolve(el.stroke_color());
            let bg = pal.resolve(el.background_color());
            for layer in &s.layers {
                let c = match layer.ink {
                    Ink::Stroke => stroke,
                    Ink::Background => bg,
                    Ink::Canvas => pal.canvas,
                };
                if c.a == 0.0 || layer.ops.is_empty() {
                    continue;
                }
                let (hex, a) = css(c);
                let d = path_d(&layer.ops);
                match &layer.paint {
                    Paint::Fill => {
                        let _ = write!(out, "<path d=\"{d}\" fill=\"{hex}\"");
                        if let Some(a) = a {
                            let _ = write!(out, " fill-opacity=\"{}\"", fmt(a as f64));
                        }
                        out.push_str("/>");
                    }
                    Paint::Stroke { width, dash } => {
                        let _ = write!(
                            out,
                            "<path d=\"{d}\" fill=\"none\" stroke=\"{hex}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
                            fmt(*width as f64)
                        );
                        if let Some([on, off]) = dash {
                            let _ = write!(out, " stroke-dasharray=\"{} {}\"", fmt(*on as f64), fmt(*off as f64));
                        }
                        if let Some(a) = a {
                            let _ = write!(out, " stroke-opacity=\"{}\"", fmt(a as f64));
                        }
                        out.push_str("/>");
                    }
                }
            }
        }
        _ => {}
    }
    out.push_str("</g>");
}

fn build_svg(scene: &Scene, opts: &ExportOptions, for_raster: bool) -> String {
    let els = export_elements(scene, opts.only.as_ref());
    let bg = color::parse(&scene.view_background()).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
    let pal = Palette { dark: opts.dark, canvas: if opts.dark { bg.dark_filter() } else { bg } };
    let b = export_bounds(&els).unwrap_or([0.0, 0.0, 0.0, 0.0]);
    let pad = opts.padding;
    let (w, h) = (b[2] - b[0] + pad * 2.0, b[3] - b[1] + pad * 2.0);
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\">",
        fmt(w), fmt(h), fmt(w), fmt(h)
    );
    if opts.embed_scene && !for_raster {
        let payload = encode_payload(&scene_json(scene, opts.only.as_ref()));
        let b64 = base64::engine::general_purpose::STANDARD.encode(latin1(&payload));
        let _ = write!(
            out,
            "<!-- svg-source:excalidraw --><metadata><!-- payload-type:{EXCALIDRAW_MIME} --><!-- payload-version:2 --><!-- payload-start -->{b64}<!-- payload-end --></metadata>"
        );
    }
    // Frame clip paths.
    let frames: Vec<&&Element> = els.iter().filter(|e| e.kind == "frame" || e.kind == "magicframe").collect();
    if !frames.is_empty() {
        out.push_str("<defs>");
        for f in &frames {
            let fb = geom::aabb(f);
            let _ = write!(
                out,
                "<clipPath id=\"clip-{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\"/></clipPath>",
                esc(&f.id), fmt(fb[0]), fmt(fb[1]), fmt(fb[2] - fb[0]), fmt(fb[3] - fb[1])
            );
        }
        out.push_str("</defs>");
    }
    if opts.background {
        let _ = write!(out, "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>", fmt(w), fmt(h), pal.canvas.to_hex());
    }
    let frame_ids: HashSet<&str> = frames.iter().map(|f| f.id.as_str()).collect();
    let _ = write!(out, "<g transform=\"translate({} {})\">", fmt(pad - b[0]), fmt(pad - b[1]));
    for el in &els {
        if el.kind == "frame" || el.kind == "magicframe" {
            continue;
        }
        match el.f.frame_id.get().filter(|f| frame_ids.contains(f.as_str())) {
            Some(f) => {
                let _ = write!(out, "<g clip-path=\"url(#clip-{})\">", esc(f));
                write_element(&mut out, el, scene, pal, for_raster);
                out.push_str("</g>");
            }
            None => write_element(&mut out, el, scene, pal, for_raster),
        }
    }
    for f in frames {
        write_element(&mut out, f, scene, pal, for_raster);
    }
    out.push_str("</g></svg>");
    out
}

/// Text bound to an arrow (drawn over a canvas-coloured backing).
pub fn is_arrow_label(el: &Element, scene: &Scene) -> bool {
    el.kind == "text" && el.container_id().and_then(|c| scene.get(c)).map(|c| c.is_linear()).unwrap_or(false)
}

/// Colour of the rendered scene at a world point (the eyedropper), as
/// `#rrggbb`; the canvas colour where nothing is drawn.
pub fn sample_color(scene: &Scene, p: [f64; 2], dark: bool) -> Option<String> {
    let opts = ExportOptions { background: true, dark, embed_scene: false, scale: 1.0, only: None, padding: 10.0 };
    let els = export_elements(scene, None);
    let b = export_bounds(&els)?;
    let (x, y) = (p[0] - b[0] + opts.padding, p[1] - b[1] + opts.padding);
    let svg = build_svg(scene, &opts, true);
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    if x < 0.0 || y < 0.0 || x >= size.width() as f64 || y >= size.height() as f64 {
        let bg = color::parse(&scene.view_background())?;
        return Some(if dark { bg.dark_filter() } else { bg }.to_hex());
    }
    let mut pm = resvg::tiny_skia::Pixmap::new(1, 1)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_translate(-x as f32, -y as f32), &mut pm.as_mut());
    let px = pm.pixel(0, 0)?.demultiply();
    Some(format!("#{:02x}{:02x}{:02x}", px.red(), px.green(), px.blue()))
}

/// The scene as an SVG document.
pub fn to_svg(scene: &Scene, opts: &ExportOptions) -> String {
    build_svg(scene, opts, false)
}

// ---------------------------------------------------------------- PNG

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn text_chunk(keyword: &str, text: &[u8]) -> Vec<u8> {
    let mut data = keyword.as_bytes().to_vec();
    data.push(0);
    data.extend_from_slice(text);
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    let mut body = b"tEXt".to_vec();
    body.extend_from_slice(&data);
    chunk.extend_from_slice(&body);
    chunk.extend_from_slice(&crc32(&body).to_be_bytes());
    chunk
}

/// The scene rasterised to PNG bytes.
pub fn to_png(scene: &Scene, opts: &ExportOptions) -> Result<Vec<u8>, String> {
    let svg = build_svg(scene, opts, true);
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).map_err(|e| e.to_string())?;
    let scale = opts.scale.clamp(0.1, 8.0) as f32;
    let size = tree.size();
    let (w, h) = (((size.width() * scale).ceil() as u32).max(1), ((size.height() * scale).ceil() as u32).max(1));
    if (w as u64) * (h as u64) > 16384 * 16384 {
        return Err("Canvas too big".into());
    }
    let mut pm = resvg::tiny_skia::Pixmap::new(w, h).ok_or("Canvas too big")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
    let mut png = pm.encode_png().map_err(|e| e.to_string())?;
    if opts.embed_scene {
        let payload = encode_payload(&scene_json(scene, opts.only.as_ref()));
        // After the signature (8) and IHDR (25).
        let chunk = text_chunk(EXCALIDRAW_MIME, &latin1(&payload));
        png.splice(33..33, chunk);
    }
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::{decode_embedded_png, decode_embedded_svg, to_png, to_svg, ExportOptions};
    use crate::scene::Scene;

    const DOC: &str = r##"{"type":"excalidraw","version":2,"source":"t","elements":[
      {"id":"r","type":"rectangle","x":10,"y":20,"width":100,"height":60,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"#a5d8ff","fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"groupIds":[],"frameId":null,"roundness":null,"seed":1,"version":1,"versionNonce":1,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false},
      {"id":"t","type":"text","x":20,"y":100,"width":80,"height":25,"angle":0.3,"strokeColor":"#e03131","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":60,"groupIds":[],"frameId":null,"roundness":null,"seed":2,"version":1,"versionNonce":1,"isDeleted":false,"boundElements":null,"updated":1,"link":null,"locked":false,"text":"Olá ✓","fontSize":20,"fontFamily":5,"textAlign":"left","verticalAlign":"top","containerId":null,"originalText":"Olá ✓","autoResize":true,"lineHeight":1.25}
    ],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##;

    #[test]
    fn svg_embeds_and_decodes_scene() {
        let s = Scene::parse(DOC).unwrap();
        let svg = to_svg(&s, &ExportOptions { embed_scene: true, ..Default::default() });
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<path"));
        assert!(svg.contains("opacity=\"0.6\""));
        let json = decode_embedded_svg(&svg).expect("payload");
        let back = Scene::parse(&json).unwrap();
        assert_eq!(back.elements().count(), 2);
        assert_eq!(back.get("t").unwrap().text(), "Olá ✓");
        // usvg accepts it.
        resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap();
    }

    #[test]
    fn png_renders_and_embeds_scene() {
        let s = Scene::parse(DOC).unwrap();
        let png = to_png(&s, &ExportOptions { embed_scene: true, scale: 2.0, ..Default::default() }).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert!(img.width() > 200 && img.height() > 200);
        // The rectangle's solid fill is painted.
        let p = img.get_pixel((60.0 - 10.0 + 10.0) as u32 * 2, (50.0 - 20.0 + 10.0) as u32 * 2);
        assert!(p[2] > 200 && p[0] < 200, "fill pixel {p:?}");
        let json = decode_embedded_png(&png).expect("payload");
        assert_eq!(Scene::parse(&json).unwrap().elements().count(), 2);
        // Without embedding there's no payload.
        let plain = to_png(&s, &ExportOptions::default()).unwrap();
        assert!(decode_embedded_png(&plain).is_none());
    }
}
