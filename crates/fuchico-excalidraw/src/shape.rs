//! Element → drawable geometry, in element-local coordinates, mirroring
//! Excalidraw's `ShapeCache.generateElementShape` (rough.js options, corner
//! radii, diamond points, arrowheads) on top of `roughr`, plus the
//! perfect-freehand style outline used for `freedraw` and glyph outlines for
//! text.

use std::hash::{Hash, Hasher};

use euclid::default::Point2D;
use palette::Srgba;
use roughr::core::{Drawable, FillStyle, OpSetType, OpType, Options};
use roughr::generator::Generator;

use crate::color;
use crate::model::Element;
use crate::text;

/// A path operation in element-local coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// Which colour a layer is painted with (resolved at paint time so theme and
/// opacity changes don't invalidate the cache).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ink {
    Stroke,
    Background,
    /// The canvas background (outline arrowheads).
    Canvas,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Stroke {
        width: f32,
        dash: Option<[f32; 2]>,
    },
    /// Even-odd fill (rough.js paths, polygons and curves; glyphs).
    Fill,
    /// Non-zero fill: rough.js fills ellipses and rectangles this way, and
    /// their two overlapping sketch passes would cancel under even-odd.
    FillNonZero,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub paint: Paint,
    pub ink: Ink,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub layers: Vec<Layer>,
}

const PLACEHOLDER: Srgba = Srgba::new(0.0, 0.0, 0.0, 1.0);

/// A hash of everything that affects an element's local-space geometry
/// (position and rotation excluded, so moving never regenerates).
pub fn shape_key(el: &Element) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    el.kind.hash(&mut h);
    for v in [
        el.width(),
        el.height(),
        el.stroke_width(),
        el.roughness(),
        el.font_size(),
        el.line_height(),
    ] {
        v.to_bits().hash(&mut h);
    }
    el.seed().hash(&mut h);
    el.font_family().hash(&mut h);
    el.stroke_style().hash(&mut h);
    el.fill_style().hash(&mut h);
    color::is_transparent(el.background_color()).hash(&mut h);
    el.text().hash(&mut h);
    el.text_align().hash(&mut h);
    format!("{:?}", el.f.roundness).hash(&mut h);
    format!("{:?}{:?}", el.f.start_arrowhead, el.f.end_arrowhead).hash(&mut h);
    if let Some(p) = el.f.points.get() {
        for q in p {
            q[0].0.to_bits().hash(&mut h);
            q[1].0.to_bits().hash(&mut h);
        }
    }
    if let Some(p) = el.f.pressures.get() {
        for q in p {
            q.0.to_bits().hash(&mut h);
        }
    }
    el.f.simulate_pressure.get().hash(&mut h);
    h.finish()
}

fn adjust_roughness(el: &Element) -> f32 {
    let r = el.roughness();
    let (w, h) = (el.width().abs(), el.height().abs());
    let max = w.max(h);
    let min = w.min(h);
    if (min >= 20.0 && max >= 50.0) || min >= 15.0 || (el.is_linear() && max >= 50.0) {
        return r as f32;
    }
    (r / if max < 10.0 { 3.0 } else { 2.0 }).min(2.5) as f32
}

fn dash(el: &Element) -> Option<[f32; 2]> {
    let sw = el.stroke_width() as f32;
    match el.stroke_style() {
        "dashed" => Some([8.0, 8.0 + sw]),
        "dotted" => Some([1.5, 6.0 + sw]),
        _ => None,
    }
}

fn fill_style(s: &str) -> FillStyle {
    match s {
        "solid" => FillStyle::Solid,
        "cross-hatch" => FillStyle::CrossHatch,
        "zigzag" => FillStyle::ZigZag,
        "zigzag-line" => FillStyle::ZigZagLine,
        "dots" => FillStyle::Dots,
        "dashed" => FillStyle::Dashed,
        _ => FillStyle::Hachure,
    }
}

/// Excalidraw's `generateRoughOptions`.
fn rough_options(el: &Element, continuous_path: bool) -> Options {
    let sw = el.stroke_width() as f32;
    let solid = el.stroke_style() == "solid";
    let mut o = Options {
        seed: Some(el.seed()),
        disable_multi_stroke: Some(!solid),
        stroke_width: Some(if solid { sw } else { sw + 0.5 }),
        fill_weight: Some(sw / 2.0),
        hachure_gap: Some(sw * 4.0),
        roughness: Some(adjust_roughness(el)),
        stroke: Some(PLACEHOLDER),
        preserve_vertices: Some(continuous_path || el.roughness() < 2.0),
        ..Options::default()
    };
    let filled = !color::is_transparent(el.background_color());
    match el.kind.as_str() {
        "rectangle" | "diamond" | "ellipse" | "embeddable" | "iframe" => {
            o.fill_style = Some(fill_style(el.fill_style()));
            if filled {
                o.fill = Some(PLACEHOLDER);
            }
            if el.kind == "ellipse" {
                o.curve_fitting = Some(1.0);
            }
        }
        "line" | "freedraw" if is_path_a_loop(&el.points()) && filled => {
            o.fill_style = Some(fill_style(el.fill_style()));
            o.fill = Some(PLACEHOLDER);
        }
        _ => {}
    }
    o
}

pub fn is_path_a_loop(points: &[[f64; 2]]) -> bool {
    if points.len() >= 3 {
        let (a, b) = (points[0], points[points.len() - 1]);
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() <= 8.0
    } else {
        false
    }
}

/// Excalidraw's `getCornerRadius`.
pub fn corner_radius(x: f64, el: &Element) -> f64 {
    let Some(r) = el.f.roundness.get() else {
        return 0.0;
    };
    let kind = r.kind.0 as i64;
    if kind == 3 {
        let fixed = r.value.get().map(|v| v.0).unwrap_or(32.0);
        let cutoff = fixed / 0.25;
        if x <= cutoff {
            return x * 0.25;
        }
        return fixed;
    }
    x * 0.25
}

/// Excalidraw's `getDiamondPoints` → (top, right, bottom, left).
pub fn diamond_points(el: &Element) -> [[f64; 2]; 4] {
    let (w, h) = (el.width(), el.height());
    let top_x = (w / 2.0).floor() + 1.0;
    let right_y = (h / 2.0).floor() + 1.0;
    [[top_x, 0.0], [w, right_y], [top_x, h], [0.0, right_y]]
}

fn sets_to_layers(
    d: Drawable<f32>,
    el: &Element,
    stroke_ink: Ink,
    fill_ink: Ink,
    out: &mut Vec<Layer>,
) {
    let shape = d.shape.clone();
    let sw = d.options.stroke_width.unwrap_or(1.0);
    let fw = {
        let f = d.options.fill_weight.unwrap_or(-1.0);
        if f < 0.0 { sw / 2.0 } else { f }
    };
    for set in d.sets {
        let mut ops = Vec::with_capacity(set.ops.len());
        for op in &set.ops {
            let v = &op.data;
            match op.op {
                OpType::Move => ops.push(Op::Move([v[0], v[1]])),
                OpType::LineTo => ops.push(Op::Line([v[0], v[1]])),
                OpType::BCurveTo => ops.push(Op::Cubic([v[0], v[1]], [v[2], v[3]], [v[4], v[5]])),
            }
        }
        let (paint, ink) = match set.op_set_type {
            OpSetType::Path => (
                Paint::Stroke {
                    width: sw,
                    dash: dash(el),
                },
                stroke_ink,
            ),
            OpSetType::FillSketch => (
                Paint::Stroke {
                    width: fw,
                    dash: None,
                },
                fill_ink,
            ),
            OpSetType::FillPath => {
                ops.push(Op::Close);
                // rough.js: `curve`/`polygon`/`path` fill even-odd, the rest
                // (ellipse, rectangle, …) non-zero.
                let paint = if matches!(shape.as_str(), "curve" | "polygon" | "path") {
                    Paint::Fill
                } else {
                    Paint::FillNonZero
                };
                (paint, fill_ink)
            }
        };
        out.push(Layer { paint, ink, ops });
    }
}

fn pt(p: [f64; 2]) -> Point2D<f32> {
    Point2D::new(p[0] as f32, p[1] as f32)
}

fn fmt(v: f64) -> String {
    format!("{v:.3}")
}

/// Builds the drawable shape of `el` in element-local coordinates.
pub fn generate(el: &Element) -> Shape {
    let g = Generator::default();
    let mut layers = Vec::new();
    let (w, h) = (el.width(), el.height());
    match el.kind.as_str() {
        "rectangle" | "embeddable" | "iframe" => {
            if el.f.roundness.get().is_some() {
                let r = corner_radius(w.abs().min(h.abs()), el);
                let d = format!(
                    "M {} 0 L {} 0 Q {} 0, {} {} L {} {} Q {} {}, {} {} L {} {} Q 0 {}, 0 {} L 0 {} Q 0 0, {} 0",
                    fmt(r),
                    fmt(w - r),
                    fmt(w),
                    fmt(w),
                    fmt(r),
                    fmt(w),
                    fmt(h - r),
                    fmt(w),
                    fmt(h),
                    fmt(w - r),
                    fmt(h),
                    fmt(r),
                    fmt(h),
                    fmt(h),
                    fmt(h - r),
                    fmt(r),
                    fmt(r)
                );
                let d = g.path::<f32>(d, &Some(rough_options(el, true)));
                sets_to_layers(d, el, Ink::Stroke, Ink::Background, &mut layers);
            } else {
                let d = g.rectangle::<f32>(
                    0.0,
                    0.0,
                    w as f32,
                    h as f32,
                    &Some(rough_options(el, false)),
                );
                sets_to_layers(d, el, Ink::Stroke, Ink::Background, &mut layers);
            }
        }
        "diamond" => {
            let [t, r, b, l] = diamond_points(el);
            if el.f.roundness.get().is_some() {
                let vr = corner_radius((t[0] - l[0]).abs(), el);
                let hr = corner_radius((r[1] - t[1]).abs(), el);
                let d = format!(
                    "M {} {} L {} {} C {} {}, {} {}, {} {} L {} {} C {} {}, {} {}, {} {} L {} {} C {} {}, {} {}, {} {} L {} {} C {} {}, {} {}, {} {}",
                    fmt(t[0] + vr),
                    fmt(t[1] + hr),
                    fmt(r[0] - vr),
                    fmt(r[1] - hr),
                    fmt(r[0]),
                    fmt(r[1]),
                    fmt(r[0]),
                    fmt(r[1]),
                    fmt(r[0] - vr),
                    fmt(r[1] + hr),
                    fmt(b[0] + vr),
                    fmt(b[1] - hr),
                    fmt(b[0]),
                    fmt(b[1]),
                    fmt(b[0]),
                    fmt(b[1]),
                    fmt(b[0] - vr),
                    fmt(b[1] - hr),
                    fmt(l[0] + vr),
                    fmt(l[1] + hr),
                    fmt(l[0]),
                    fmt(l[1]),
                    fmt(l[0]),
                    fmt(l[1]),
                    fmt(l[0] + vr),
                    fmt(l[1] - hr),
                    fmt(t[0] - vr),
                    fmt(t[1] + hr),
                    fmt(t[0]),
                    fmt(t[1]),
                    fmt(t[0]),
                    fmt(t[1]),
                    fmt(t[0] + vr),
                    fmt(t[1] + hr),
                );
                let d = g.path::<f32>(d, &Some(rough_options(el, true)));
                sets_to_layers(d, el, Ink::Stroke, Ink::Background, &mut layers);
            } else {
                let d = g.polygon::<f32>(
                    &[pt(t), pt(r), pt(b), pt(l)],
                    &Some(rough_options(el, false)),
                );
                sets_to_layers(d, el, Ink::Stroke, Ink::Background, &mut layers);
            }
        }
        "ellipse" => {
            let d = g.ellipse::<f32>(
                (w / 2.0) as f32,
                (h / 2.0) as f32,
                w as f32,
                h as f32,
                &Some(rough_options(el, false)),
            );
            sets_to_layers(d, el, Ink::Stroke, Ink::Background, &mut layers);
        }
        "line" | "arrow" => linear(el, &g, &mut layers),
        "freedraw" => freedraw(el, &g, &mut layers),
        "text" => {
            let ops = text::outline_block(
                el.text(),
                el.font_family(),
                el.font_size(),
                el.line_height(),
                el.text_align(),
                w,
            );
            layers.push(Layer {
                paint: Paint::Fill,
                ink: Ink::Stroke,
                ops,
            });
        }
        _ => {}
    }
    Shape { layers }
}

fn linear(el: &Element, g: &Generator, layers: &mut Vec<Layer>) {
    let points = el.points();
    if points.len() < 2 {
        return;
    }
    let opts = rough_options(el, false);
    let pts: Vec<Point2D<f32>> = points.iter().map(|p| pt(*p)).collect();
    let d = if el.f.roundness.get().is_none() {
        if opts.fill.is_some() {
            g.polygon::<f32>(&pts, &Some(opts.clone()))
        } else {
            g.linear_path::<f32>(&pts, false, &Some(opts.clone()))
        }
    } else {
        g.curve::<f32>(&pts, &Some(opts.clone()))
    };
    // The stroke op set (the one arrowheads are derived from).
    let stroke_ops: Vec<(OpType, Vec<f32>)> = d
        .sets
        .iter()
        .find(|s| s.op_set_type == OpSetType::Path)
        .map(|s| {
            s.ops
                .iter()
                .map(|o| (o.op.clone(), o.data.clone()))
                .collect()
        })
        .unwrap_or_default();
    sets_to_layers(d, el, Ink::Stroke, Ink::Background, layers);

    if el.kind != "arrow" {
        return;
    }
    for (pos, head) in [
        ("start", el.f.start_arrowhead.get()),
        ("end", el.f.end_arrowhead.get()),
    ] {
        let Some(head) = head else { continue };
        let Some(hp) = arrowhead_points(el, &points, &stroke_ops, pos, head) else {
            continue;
        };
        let mut o = opts.clone();
        o.stroke_line_dash = None;
        o.fill = None;
        // arrowheads are always solid-stroked
        let solid_dash: Option<[f32; 2]> = None;
        let mut push = |d: Drawable<f32>, fill_ink: Ink| {
            let start = layers.len();
            sets_to_layers(d, el, Ink::Stroke, fill_ink, layers);
            for l in &mut layers[start..] {
                if let Paint::Stroke { dash, .. } = &mut l.paint {
                    *dash = solid_dash;
                }
            }
        };
        match head.as_str() {
            "dot" | "circle" | "circle_outline" => {
                let mut o2 = o.clone();
                o2.fill = Some(PLACEHOLDER);
                o2.fill_style = Some(FillStyle::Solid);
                o2.roughness = Some(o.roughness.unwrap_or(0.0).min(0.5));
                let fill_ink = if head == "circle_outline" {
                    Ink::Canvas
                } else {
                    Ink::Stroke
                };
                push(g.circle::<f32>(hp[0], hp[1], hp[2], &Some(o2)), fill_ink);
            }
            "triangle" | "triangle_outline" => {
                let mut o2 = o.clone();
                o2.fill = Some(PLACEHOLDER);
                o2.fill_style = Some(FillStyle::Solid);
                o2.roughness = Some(o.roughness.unwrap_or(0.0).min(1.0));
                let fill_ink = if head == "triangle_outline" {
                    Ink::Canvas
                } else {
                    Ink::Stroke
                };
                let poly = [
                    Point2D::new(hp[0], hp[1]),
                    Point2D::new(hp[2], hp[3]),
                    Point2D::new(hp[4], hp[5]),
                    Point2D::new(hp[0], hp[1]),
                ];
                push(g.polygon::<f32>(&poly, &Some(o2)), fill_ink);
            }
            "diamond" | "diamond_outline" => {
                let mut o2 = o.clone();
                o2.fill = Some(PLACEHOLDER);
                o2.fill_style = Some(FillStyle::Solid);
                o2.roughness = Some(o.roughness.unwrap_or(0.0).min(1.0));
                let fill_ink = if head == "diamond_outline" {
                    Ink::Canvas
                } else {
                    Ink::Stroke
                };
                let poly = [
                    Point2D::new(hp[0], hp[1]),
                    Point2D::new(hp[2], hp[3]),
                    Point2D::new(hp[4], hp[5]),
                    Point2D::new(hp[6], hp[7]),
                    Point2D::new(hp[0], hp[1]),
                ];
                push(g.polygon::<f32>(&poly, &Some(o2)), fill_ink);
            }
            _ => {
                // arrow, bar and crowfoot variants: two strokes into the tip.
                push(
                    g.line::<f32>(hp[2], hp[3], hp[0], hp[1], &Some(o.clone())),
                    Ink::Stroke,
                );
                push(
                    g.line::<f32>(hp[4], hp[5], hp[0], hp[1], &Some(o.clone())),
                    Ink::Stroke,
                );
            }
        }
    }
}

fn arrowhead_size(head: &str) -> f32 {
    match head {
        "arrow" => 25.0,
        "diamond" | "diamond_outline" => 12.0,
        h if h.starts_with("crowfoot") => 20.0,
        _ => 15.0,
    }
}

fn rotate(p: [f32; 2], c: [f32; 2], a: f32) -> [f32; 2] {
    let (s, co) = a.sin_cos();
    let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + dx * co - dy * s, c[1] + dx * s + dy * co]
}

/// Port of Excalidraw's `getArrowheadPoints`.
fn arrowhead_points(
    el: &Element,
    points: &[[f64; 2]],
    ops: &[(OpType, Vec<f32>)],
    position: &str,
    head: &str,
) -> Option<Vec<f32>> {
    if ops.len() < 2 {
        return None;
    }
    let index = if position == "start" {
        1
    } else {
        ops.len() - 1
    };
    let (op, data) = &ops[index];
    if *op != OpType::BCurveTo || data.len() < 6 {
        return None;
    }
    let p3 = [data[4], data[5]];
    let p2 = [data[2], data[3]];
    let p1 = [data[0], data[1]];
    let p0 = match &ops[index - 1] {
        (OpType::Move, d) => [d[0], d[1]],
        (OpType::BCurveTo, d) => [d[4], d[5]],
        _ => [0.0, 0.0],
    };
    let eq = |t: f32, i: usize| {
        p0[i] * (1.0 - t).powi(3)
            + 3.0 * p1[i] * t * (1.0 - t).powi(2)
            + 3.0 * p2[i] * t * t * (1.0 - t)
            + p3[i] * t.powi(3)
    };
    let [x2, y2] = if position == "start" { p0 } else { p3 };
    let (x1, y1) = (eq(0.3, 0), eq(0.3, 1));
    let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    if dist == 0.0 {
        return None;
    }
    let (nx, ny) = ((x2 - x1) / dist, (y2 - y1) / dist);
    let size = arrowhead_size(head);
    let n = points.len();
    let (c, p) = if position == "end" {
        (points[n - 1], points[n - 2])
    } else {
        (points[0], points[1])
    };
    let length = (((c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2)).sqrt()) as f32;
    let mult = if head.starts_with("diamond") {
        0.25
    } else {
        0.5
    };
    let min_size = size.min(length * mult);
    let (xs, ys) = (x2 - nx * min_size, y2 - ny * min_size);
    if matches!(head, "dot" | "circle" | "circle_outline") {
        let diameter =
            ((ys - y2).powi(2) + (xs - x2).powi(2)).sqrt() + el.stroke_width() as f32 - 2.0;
        return Some(vec![x2, y2, diameter.max(1.0)]);
    }
    let angle: f32 = match head {
        "bar" => 90.0,
        "arrow" => 20.0,
        _ => 25.0,
    };
    let a = angle.to_radians();
    let p3 = rotate([xs, ys], [x2, y2], -a);
    let p4 = rotate([xs, ys], [x2, y2], a);
    if head.starts_with("diamond") {
        let o = if position == "start" {
            let q = points[1];
            rotate(
                [x2 + min_size * 2.0, y2],
                [x2, y2],
                (q[1] as f32 - y2).atan2(q[0] as f32 - x2),
            )
        } else {
            let q = points[n - 2];
            rotate(
                [x2 - min_size * 2.0, y2],
                [x2, y2],
                (y2 - q[1] as f32).atan2(x2 - q[0] as f32),
            )
        };
        return Some(vec![x2, y2, p3[0], p3[1], o[0], o[1], p4[0], p4[1]]);
    }
    Some(vec![x2, y2, p3[0], p3[1], p4[0], p4[1]])
}

// ------------------------------------------------------------- freedraw

/// perfect-freehand `getStroke` with Excalidraw's options
/// (`size = strokeWidth * 4.25`, thinning 0.6, streamline 0.5, sine easing).
pub fn freedraw_outline(el: &Element) -> Vec<[f32; 2]> {
    let pts = el.points();
    let simulate = el.f.simulate_pressure.get().copied().unwrap_or(true);
    let pressures: Vec<f64> =
        el.f.pressures
            .get()
            .map(|v| v.iter().map(|n| n.0).collect())
            .unwrap_or_default();
    let size = el.stroke_width() * 4.25;
    let thinning = 0.6;
    let streamline = 0.5;
    let easing = |t: f64| (t * std::f64::consts::PI / 2.0).sin();
    let radius_for = |p: f64| size * easing(0.5 - thinning * (0.5 - p));

    if pts.is_empty() {
        return Vec::new();
    }
    // Streamlined points with pressure.
    let t = 0.15 + (1.0 - streamline) * 0.85;
    let mut sp: Vec<([f64; 2], f64, f64)> = Vec::new(); // (point, pressure, distance)
    let pr = |i: usize| -> f64 {
        if simulate {
            0.5
        } else {
            pressures.get(i).copied().unwrap_or(0.5)
        }
    };
    sp.push((pts[0], pr(0), 0.0));
    for (i, p) in pts.iter().enumerate().skip(1) {
        let prev = sp.last().unwrap().0;
        let q = if i == pts.len() - 1 {
            *p
        } else {
            [
                prev[0] + (p[0] - prev[0]) * t,
                prev[1] + (p[1] - prev[1]) * t,
            ]
        };
        let d = ((q[0] - prev[0]).powi(2) + (q[1] - prev[1]).powi(2)).sqrt();
        if d < 0.5 && i != pts.len() - 1 {
            continue;
        }
        sp.push((q, pr(i), d));
    }

    if sp.len() == 1 {
        // A dot.
        let r = radius_for(sp[0].1).max(0.5);
        let c = sp[0].0;
        return (0..16)
            .map(|k| {
                let a = k as f64 / 16.0 * std::f64::consts::TAU;
                [(c[0] + r * a.cos()) as f32, (c[1] + r * a.sin()) as f32]
            })
            .collect();
    }

    const RATE: f64 = 0.275;
    let mut prev_pressure = sp.iter().take(10).fold(sp[0].1, |acc, (_, p, d)| {
        let mut pressure = *p;
        if simulate {
            let s = (d / size).min(1.0);
            let r = (1.0 - s).min(1.0);
            pressure = (acc + (r - acc) * (s * RATE)).min(1.0);
        }
        (acc + pressure) / 2.0
    });

    let mut left: Vec<[f64; 2]> = Vec::new();
    let mut right: Vec<[f64; 2]> = Vec::new();
    let mut radii = Vec::new();
    for i in 0..sp.len() {
        let (p, pin, d) = sp[i];
        let mut pressure = pin;
        if simulate {
            let s = (d / size).min(1.0);
            let r = (1.0 - s).min(1.0);
            pressure = (prev_pressure + (r - prev_pressure) * (s * RATE)).min(1.0);
        }
        prev_pressure = pressure;
        let radius = radius_for(pressure).max(0.25);
        radii.push(radius);
        let a = if i + 1 < sp.len() { sp[i + 1].0 } else { p };
        let b = if i > 0 { sp[i - 1].0 } else { p };
        let (mut vx, mut vy) = (a[0] - b[0], a[1] - b[1]);
        let len = (vx * vx + vy * vy).sqrt();
        if len > 0.0 {
            vx /= len;
            vy /= len;
        } else {
            vx = 1.0;
            vy = 0.0;
        }
        let (px, py) = (-vy, vx);
        left.push([p[0] + px * radius, p[1] + py * radius]);
        right.push([p[0] - px * radius, p[1] - py * radius]);
    }
    let cap = |c: [f64; 2], from: [f64; 2], r: f64, out: &mut Vec<[f64; 2]>| {
        let a0 = (from[1] - c[1]).atan2(from[0] - c[0]);
        for k in 1..12 {
            let a = a0 + std::f64::consts::PI * k as f64 / 12.0;
            out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
        }
    };
    let mut poly = left.clone();
    let n = sp.len();
    cap(sp[n - 1].0, left[n - 1], radii[n - 1], &mut poly);
    poly.extend(right.iter().rev());
    cap(sp[0].0, right[0], radii[0], &mut poly);
    poly.into_iter()
        .map(|p| [p[0] as f32, p[1] as f32])
        .collect()
}

fn freedraw(el: &Element, g: &Generator, layers: &mut Vec<Layer>) {
    let pts = el.points();
    if !color::is_transparent(el.background_color()) && is_path_a_loop(&pts) {
        let mut o = rough_options(el, false);
        o.stroke = None;
        o.fill = Some(PLACEHOLDER);
        let poly: Vec<Point2D<f32>> = pts.iter().map(|p| pt(*p)).collect();
        let d = g.polygon::<f32>(&poly, &Some(o));
        sets_to_layers(d, el, Ink::Stroke, Ink::Background, layers);
    }
    let outline = freedraw_outline(el);
    if outline.len() < 3 {
        return;
    }
    // Smooth the outline with midpoint quadratics like `getSvgPathFromStroke`.
    let mut ops = vec![Op::Move(outline[0])];
    for i in 0..outline.len() {
        let a = outline[i];
        let b = outline[(i + 1) % outline.len()];
        ops.push(Op::Quad(a, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]));
    }
    ops.push(Op::Close);
    layers.push(Layer {
        paint: Paint::Fill,
        ink: Ink::Stroke,
        ops,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(json: &str) -> Element {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn generates_every_kind() {
        for (kind, extra) in [
            ("rectangle", r##""roundness":{"type":3}"##),
            (
                "rectangle",
                r##""roundness":null,"backgroundColor":"#ffc9c9","fillStyle":"hachure""##,
            ),
            ("diamond", r##""roundness":{"type":2}"##),
            (
                "diamond",
                r##""roundness":null,"backgroundColor":"#a5d8ff","fillStyle":"cross-hatch""##,
            ),
            (
                "ellipse",
                r##""backgroundColor":"#b2f2bb","fillStyle":"solid""##,
            ),
            (
                "line",
                r##""points":[[0,0],[100,50],[30,80]],"roundness":{"type":2}"##,
            ),
            (
                "arrow",
                r##""points":[[0,0],[100,50]],"endArrowhead":"arrow","startArrowhead":"dot""##,
            ),
            (
                "arrow",
                r##""points":[[0,0],[100,50]],"endArrowhead":"triangle","startArrowhead":"diamond""##,
            ),
            (
                "freedraw",
                r##""points":[[0,0],[10,5],[20,15],[40,20]],"pressures":[],"simulatePressure":true"##,
            ),
            ("text", r##""text":"Hi","fontSize":20,"fontFamily":5"##),
        ] {
            let e = el(&format!(
                r##"{{"id":"a","type":"{kind}","x":0,"y":0,"width":100,"height":80,"seed":42,"strokeWidth":2,"strokeStyle":"dashed",{extra}}}"##
            ));
            let s = generate(&e);
            assert!(!s.layers.is_empty(), "{kind} {extra}");
            // Deterministic per seed.
            let s2 = generate(&e);
            assert_eq!(
                format!("{:?}", s.layers[0].ops),
                format!("{:?}", s2.layers[0].ops)
            );
        }
    }

    /// rough.js fills an ellipse with two overlapping passes; even-odd would
    /// cancel them out (the solid ellipse used to render unfilled).
    #[test]
    fn solid_ellipse_fills_non_zero() {
        let e = el(
            r##"{"id":"a","type":"ellipse","x":0,"y":0,"width":180,"height":110,"seed":1002,"backgroundColor":"#ffc9c9","fillStyle":"solid"}"##,
        );
        let s = generate(&e);
        assert!(s.layers.iter().any(|l| l.paint == Paint::FillNonZero));
        let r = el(
            r##"{"id":"a","type":"rectangle","x":0,"y":0,"width":180,"height":110,"seed":1,"roundness":{"type":3},"backgroundColor":"#ffc9c9","fillStyle":"solid"}"##,
        );
        assert!(generate(&r).layers.iter().any(|l| l.paint == Paint::Fill));
    }
}
